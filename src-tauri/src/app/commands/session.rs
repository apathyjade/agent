//! 会话 IPC 命令：会话 CRUD、消息收发与「回合」装配。
//!
//! 这里是整个应用最核心的对外接口。两个发送入口共用
//! [`prepare_turn`] 完成路由、模型解析与提示词组装，再按
//! [`PathSelection`] 分派到 fast / deep 两条执行路径——流式与非流式的区别
//! 只在于是否把中间过程 emit 给前端。
//!
//! 边界：本模块负责「一个回合」的编排，具体推理在 `agent::fast_loop` 与
//! `agent::orchestrator`；本模块不实现工具、不直接调用 LLM provider。

use chrono::Utc;
use serde::Serialize;
use tauri::{Emitter, Manager, State};
use uuid::Uuid;

use crate::agent::fast_loop::ToolLoop;
use crate::agent::orchestrator::plan_types::{
    CompletedStep, ExecutionHandle, ExecutionLogEntry, PlanFailure, PlanProgressEvent, PlanRunMode,
};
use crate::agent::orchestrator::planner::LlmPlanner;
use crate::agent::orchestrator::runtime::ExecutionRuntime;
use crate::app::commands::intent::{MODE_AUTO, MODE_PINNED};
use crate::app::commands::{StreamChunk, ToolCallEvent};
use crate::app::state::AppState;
use crate::core::db::models::{Message as DbMessage, Session as DbSession};
use crate::core::error::{AppError, Result};
use crate::features::persona::{PersonaInfo, PersonaResolution};
use crate::llm::intent::{
    DecisionSource, ExecutionPolicy, IntentResult, PathSelection, RouteContext, RouteDecision,
};
use rig::completion::Message;

/// 解析持久化的工具调用 JSON（`[{id, name, arguments}]` 或 Rig 的 ToolCall 形态）。
fn parse_tool_calls(raw: Option<&str>) -> Vec<rig::message::ToolCall> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<rig::message::ToolCall>>(raw).unwrap_or_default()
}

/// 把数据库消息还原成 Rig 的对话消息。
///
/// - assistant 行还原 `AssistantContent::ToolCall`；
/// - tool 行用真实工具名还原 `Message::tool_result`；
/// - 没有对应 assistant 调用的 tool 行直接跳过（旧数据可能残留孤立行）；
/// - user / system 原样还原。
fn history_messages(messages: &[DbMessage]) -> Vec<Message> {
    use rig::message::{AssistantMessage, AssistantContent, CallId, ToolName};
    use std::collections::HashSet;

    let mut out: Vec<Message> = Vec::new();
    let mut pending_calls: HashSet<String> = HashSet::new();

    for message in messages {
        match message.role.as_str() {
            "assistant" => {
                let mut content: Vec<AssistantContent> = Vec::new();
                if !message.content.is_empty() {
                    content.push(AssistantContent::text(message.content.clone()));
                }
                for call in parse_tool_calls(message.tool_calls.as_deref()) {
                    pending_calls.insert(call.id.to_string());
                    content.push(AssistantContent::ToolCall(call));
                }
                if !content.is_empty() {
                    out.push(Message::Assistant(AssistantMessage::new(content)));
                }
            }
            "tool" => {
                let Some(call_id) = message.tool_call_id.clone() else {
                    continue;
                };
                // 孤立的工具结果无法对应任何调用，喂给模型只会报错。
                if !pending_calls.remove(&call_id) {
                    continue;
                }
                let name = message
                    .tool_name
                    .clone()
                    .unwrap_or_else(|| "tool".to_string());
                if let Ok(name) = ToolName::new(name) {
                    out.push(Message::tool_result(
                        CallId::from_wire(call_id),
                        name,
                        message.content.clone(),
                    ));
                }
            }
            "system" => out.push(Message::system(message.content.clone())),
            _ => out.push(Message::user(message.content.clone())),
        }
    }

    out
}

/// 把一轮 transcript 展开成待落库的消息行。
///
/// 只遍历 **content 变体**：`Assistant` 行写文本 + `tool_calls`；`User` 里的
/// `ToolResult` 写 `role="tool"` 行；本轮 prompt / 重问反馈跳过。
fn transcript_rows(session_id: &str, messages: &[Message]) -> Vec<DbMessage> {
    use rig::message::{AssistantContent, UserContent};

    let mut rows: Vec<DbMessage> = Vec::new();
    // 同一轮的多行几乎同时创建；给每行加一点微秒偏移，保证
    // `ORDER BY created_at` 还原出 transcript 的顺序。
    let base = Utc::now();

    for message in messages {
        match message {
            Message::Assistant(assistant) => {
                let mut text = String::new();
                let mut calls = Vec::new();
                for content in &assistant.content {
                    match content {
                        AssistantContent::Text(part) => text.push_str(&part.text),
                        AssistantContent::ToolCall(call) => calls.push(call.clone()),
                        _ => {}
                    }
                }
                if text.is_empty() && calls.is_empty() {
                    continue;
                }
                rows.push(DbMessage {
                    id: Uuid::new_v4().to_string(),
                    session_id: session_id.to_string(),
                    role: "assistant".to_string(),
                    content: text,
                    tool_calls: if calls.is_empty() {
                        None
                    } else {
                        serde_json::to_string(&calls).ok()
                    },
                    tool_call_id: None,
                    tool_name: None,
                    tokens: None,
                    created_at: (base + chrono::Duration::microseconds(rows.len() as i64))
                        .to_rfc3339(),
                });
            }
            Message::User { content } => {
                for item in content {
                    let UserContent::ToolResult(result) = item else {
                        continue;
                    };
                    let text = result
                        .content
                        .iter()
                        .filter_map(|part| part.as_text())
                        .collect::<Vec<_>>()
                        .join("\n");
                    rows.push(DbMessage {
                        id: Uuid::new_v4().to_string(),
                        session_id: session_id.to_string(),
                        role: "tool".to_string(),
                        content: text,
                        tool_calls: None,
                        tool_call_id: Some(result.call.to_string()),
                        tool_name: Some(result.name.to_string()),
                        tokens: None,
                        created_at: (base + chrono::Duration::microseconds(rows.len() as i64))
                            .to_rfc3339(),
                    });
                }
            }
            _ => {}
        }
    }

    rows
}

/// 把 transcript 行按顺序写入数据库；单行失败只记日志，不中断整轮。
async fn persist_transcript_rows(state: &AppState, rows: &[DbMessage]) {
    let db = state.db.lock().await;
    for row in rows {
        if let Err(e) = db.insert_message(row) {
            log::error!(
                "failed to persist {} message for session '{}': {}",
                row.role,
                row.session_id,
                e
            );
        }
    }
}

/// 组装一条最终的 assistant 行（transcript 为空时的兜底）。
fn assistant_row(session_id: &str, content: String) -> DbMessage {
    DbMessage {
        id: Uuid::new_v4().to_string(),
        session_id: session_id.to_string(),
        role: "assistant".to_string(),
        content,
        tool_calls: None,
        tool_call_id: None,
        tool_name: None,
        tokens: None,
        created_at: Utc::now().to_rfc3339(),
    }
}

/// 用户未配置系统提示词时使用的默认值。
/// 引导模型给出清晰、结构化、高质量的回答。
const DEFAULT_SYSTEM_PROMPT: &str = "You are a helpful, respectful AI assistant. \
Provide clear, accurate, and well-structured responses. \
Use the same language as the user's input. \
When responding in Chinese, ensure proper grammar and natural phrasing. \
If you don't know something, say so rather than making up information.";

/// 交给 planner 的上下文块字符上限。
const DEEP_CONTEXT_CHAR_LIMIT: usize = 4000;
/// 深度路径纳入上下文的持久化消息条数。
const DEEP_CONTEXT_MESSAGE_LIMIT: usize = 6;
/// 写入提示词的记忆条数（与 fast 路径保持一致）。
const MEMORY_PROMPT_LIMIT: i64 = 5;
/// 路由只看最近 6 条历史，更早的对话不参与分类。
const ROUTE_HISTORY_LIMIT: usize = 6;

/// 新建会话。
///
/// `persona_id` / `project_id` 为 `None` 时表示不绑定。`intent_domain_mode`
/// 固定从 `auto` 起步——领域先验由首轮路由学习，而不是创建时指定。
/// 返回值是**落库后重新读取**的行：`create_session` 会在 SQL 里继承默认
/// 领域，内存中的结构体拿不到这个值。
#[tauri::command]
pub async fn create_session(
    state: State<'_, AppState>,
    title: String,
    model_id: String,
    system_prompt: Option<String>,
    persona_id: Option<String>,
    project_id: Option<String>,
) -> Result<DbSession> {
    let now = Utc::now().to_rfc3339();
    let sess = DbSession {
        id: Uuid::new_v4().to_string(),
        title,
        model_id,
        system_prompt,
        persona_id,
        config: None,
        title_source: String::from("manual"),
        archived: false,
        created_at: now.clone(),
        updated_at: now,
        mode: "chat".to_string(),
        execution_status: r#"{"type":"idle"}"#.to_string(),
        active_plan_id: None,
        project_id,
        intent_domain_id: None,
        intent_domain_mode: MODE_AUTO.to_string(),
    };

    let db = state.db.lock().await;
    db.create_session(&sess)?;
    // `create_session` 在 SQL 里解析继承来的默认领域，内存里的值会是 `None`。
    // 重新读一次，让调用方（会话头部选择器）拿到真实的领域状态。
    let stored = db.get_session(&sess.id)?.unwrap_or(sess);

    Ok(stored)
}

/// 列出会话，默认不含已归档。
///
/// - `include_archived`：`Some(true)` 时连同归档会话一起返回，`None`/`false` 只返回未归档。
/// - `project_id`：`Some(id)` 时只返回该项目下的会话，`None` 表示不按项目过滤。
#[tauri::command]
pub async fn list_sessions(
    state: State<'_, AppState>,
    include_archived: Option<bool>,
    project_id: Option<String>,
) -> Result<Vec<DbSession>> {
    let db = state.db.lock().await;
    let all = if let Some(pid) = project_id {
        db.get_sessions_by_project(&pid)?
    } else {
        db.list_sessions()?
    };
    if include_archived.unwrap_or(false) {
        Ok(all)
    } else {
        Ok(all.into_iter().filter(|s| !s.archived).collect())
    }
}

/// 把会话标记为已归档（不删除数据）。
#[tauri::command]
pub async fn archive_session(state: State<'_, AppState>, id: String) -> Result<()> {
    let db = state.db.lock().await;
    db.set_session_archived(&id, true)
}

/// 取消会话的归档标记。
#[tauri::command]
pub async fn unarchive_session(state: State<'_, AppState>, id: String) -> Result<()> {
    let db = state.db.lock().await;
    db.set_session_archived(&id, false)
}

/// 只列出已归档会话，供归档视图使用。
#[tauri::command]
pub async fn list_archived_sessions(state: State<'_, AppState>) -> Result<Vec<DbSession>> {
    let db = state.db.lock().await;
    let all = db.list_sessions()?;
    Ok(all.into_iter().filter(|s| s.archived).collect())
}

/// 按 id 读取单个会话；不存在时返回 `Ok(None)`（而不是错误），
/// 便于前端在会话被删除后做幂等处理。
#[tauri::command]
pub async fn get_session(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<DbSession>> {
    let db = state.db.lock().await;
    db.get_session(&id)
}

/// 永久删除会话及其消息。
#[tauri::command]
pub async fn delete_session(state: State<'_, AppState>, id: String) -> Result<()> {
    let db = state.db.lock().await;
    db.delete_session(&id)
}

/// 修改会话标题（会同时把 `title_source` 标记为手动）。
#[tauri::command]
pub async fn update_session_title(
    state: State<'_, AppState>,
    id: String,
    title: String,
) -> Result<()> {
    let db = state.db.lock().await;
    db.update_session_title(&id, &title)
}

/// 修改会话使用的模型 id。
#[tauri::command]
pub async fn update_session_model(
    state: State<'_, AppState>,
    id: String,
    model_id: String,
) -> Result<()> {
    let db = state.db.lock().await;
    db.update_session_model(&id, &model_id)
}

/// 覆盖会话的系统提示词。传空串表示回到内置默认提示词。
#[tauri::command]
pub async fn update_session_system_prompt(
    state: State<'_, AppState>,
    id: String,
    system_prompt: String,
) -> Result<()> {
    let db = state.db.lock().await;
    db.update_session_system_prompt(&id, &system_prompt)
}

/// 覆盖会话的 JSON 配置（当前用于 `enabled_tools` 白名单）。
///
/// `config` 是序列化后的 JSON 字符串，后端不校验结构，读取方按需解析。
#[tauri::command]
pub async fn update_session_config(
    state: State<'_, AppState>,
    id: String,
    config: String,
) -> Result<()> {
    let db = state.db.lock().await;
    db.update_session_config(&id, &config)
}

/// 清空会话内的全部消息，保留会话本身。
#[tauri::command]
pub async fn clear_session(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<()> {
    let db = state.db.lock().await;
    db.clear_messages(&session_id)
}

/// 发送一条消息并同步等待完整回复（非流式）。
///
/// 返回落库后的 assistant 消息。前端拿不到任何中间过程，适合脚本化调用；
/// 桌面聊天窗口应使用 [`send_message_stream`]。
///
/// `tools_enabled`：`None` 等价于 `true`。
/// `active_persona_id`：`Some(id)` 覆盖会话绑定的人设，`None` 沿用会话设置。
#[tauri::command]
pub async fn send_message(
    state: State<'_, AppState>,
    session_id: String,
    content: String,
    tools_enabled: Option<bool>,
    active_persona_id: Option<String>,
) -> Result<DbMessage> {
    let turn = prepare_turn(
        state.inner(),
        &session_id,
        &content,
        tools_enabled,
        active_persona_id,
    )
    .await?;

    let assistant_msg = match turn.path {
        PathSelection::Fast => {
            let outcome = execute_fast_non_streaming(state.inner(), &turn).await?;
            let mut rows = transcript_rows(&session_id, &outcome.messages);

            // transcript 的最后一条 assistant 就是最终回答；没有 transcript
            // （纯文本回答）时才补一条兜底行。
            let final_row = match rows.last() {
                Some(row) if row.role == "assistant" => rows.pop().expect("刚检查过非空"),
                _ => assistant_row(&session_id, outcome.output.clone()),
            };

            persist_transcript_rows(state.inner(), &rows).await;
            {
                let db = state.db.lock().await;
                db.insert_message(&final_row)?;
            }
            final_row
        }
        PathSelection::Deep => {
            let content = execute_deep_non_streaming(state.inner(), &turn).await?;
            let row = assistant_row(&session_id, content);
            let db = state.db.lock().await;
            db.insert_message(&row)?;
            row
        }
    };

    spawn_lifecycle_hooks(state.inner(), &session_id, &turn.session.model_id);

    Ok(assistant_msg)
}

/// 发送一条消息，把回复与执行过程通过 `stream_chunk` / `execution_log` /
/// `plan_progress` / `intent_resolved` 事件增量推给前端。
///
/// 返回最终拼接好的完整回复文本；调用方通常只把它当作「回合结束」的信号。
/// 与 [`send_message`] 共用 [`prepare_turn`]，两条路径的路由与提示词不会
/// 漂移。参数语义同 `send_message`。
#[tauri::command]
pub async fn send_message_stream(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    session_id: String,
    content: String,
    tools_enabled: Option<bool>,
    active_persona_id: Option<String>,
) -> Result<String> {
    // 路由真正完成时才 emit `intent_resolved`；不要在工作开始前就先播报
    // 一个「分类中」的阶段。

    // 两个入口共用同一个解析器：流式与非流式的路由、配方、模型与上下文
    // 不可能出现差异。
    let turn = prepare_turn(
        state.inner(),
        &session_id,
        &content,
        tools_enabled,
        active_persona_id,
    )
    .await?;

    // 调试视图：把组装好的请求存一份（fast 路径形状；deep 路径通过 planner
    // 消费同一份会话/系统配置）。
    let messages_json = serde_json::to_string(&turn.fast_messages).unwrap_or_default();
    let _ = app_handle.emit(
        "debug_messages",
        serde_json::to_value(&turn.fast_messages).unwrap_or_default(),
    );
    {
        let db = state.db.lock().await;
        let _ = db.set_setting(&format!("request_ctx:{}", session_id), &messages_json);
    }

    let full_content = match turn.path {
        PathSelection::Deep => {
            emit_execution_log(
                &app_handle,
                "info",
                "planner",
                format!("路由到深度执行: intent={} ({})", turn.route.chosen.id, turn.route.chosen.name),
            );
            emit_phase(&app_handle, "deep_think");
            execute_deep_streaming(state.inner(), &app_handle, &turn).await?
        }
        PathSelection::Fast => execute_fast_streaming(state.inner(), &app_handle, &turn).await?,
    };

    let db = state.db.lock().await;
    let assistant_msg = DbMessage {
        id: Uuid::new_v4().to_string(),
        session_id: session_id.clone(),
        role: "assistant".to_string(),
        content: full_content.clone(),
        tool_calls: None,
        tool_call_id: None,
        tool_name: None,
        tokens: None,
        created_at: Utc::now().to_rfc3339(),
    };

    db.insert_message(&assistant_msg)?;
    drop(db);

    spawn_lifecycle_hooks(state.inner(), &session_id, &turn.session.model_id);

    Ok(full_content)
}

/// 按时间顺序读取会话的全部消息，供聊天窗口回填历史。
#[tauri::command]
pub async fn get_messages(state: State<'_, AppState>, session_id: String) -> Result<Vec<DbMessage>> {
    let db = state.db.lock().await;
    db.get_messages(&session_id)
}

/// 覆盖保存「上一次请求上下文」的调试快照（key 为 `request_ctx:<session_id>`）。
///
/// 仅用于排查提示词问题，不参与推理。
#[tauri::command]
pub async fn save_request_context(
    state: State<'_, AppState>,
    session_id: String,
    messages_json: String,
) -> Result<()> {
    let db = state.db.lock().await;
    db.set_setting(&format!("request_ctx:{}", session_id), &messages_json)
}

/// 读取调试用的请求上下文快照；从未保存过时返回 `None`。
#[tauri::command]
pub async fn get_request_context(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Option<String>> {
    let db = state.db.lock().await;
    db.get_setting(&format!("request_ctx:{}", session_id))
}

// ── 共享的回合准备 ────────────────────────────────────────────────────────

/// 流式与非流式两个入口运行一个回合所需的全部输入。
///
/// 只构造一次，才能保证路由、意图配方、模型解析与提示词组装在两条路径上
/// 完全一致。
pub(crate) struct PreparedTurn {
    pub session: DbSession,
    pub messages: Vec<DbMessage>,
    pub route: RouteDecision,
    pub path: PathSelection,
    pub tools_enabled: bool,
    pub allowed_tools: Option<Vec<String>>,
    pub model_id: String,
    pub max_iterations: Option<usize>,
    /// 生效的会话系统提示词（会话值或内置默认值）。
    pub session_system_prompt: String,
    /// 解析出人设时的 `Your current role: ...` 块。
    pub persona_block: Option<String>,
    /// 本轮的 memory 上下文；每回合只注入一次。
    pub memory_prompt: Option<String>,
    /// 组装好的 fast 路径消息（system/persona/appendix/memory/history）。
    pub fast_messages: Vec<Message>,
    /// 原始用户输入，planner 需要它作为目标。
    pub user_content: String,
}

impl PreparedTurn {
    /// 意图附录按既有 system 块的形式渲染（若配置了）。
    fn intent_appendix(&self) -> Option<String> {
        intent_appendix(&self.route.chosen)
    }

    /// 会话系统提示词 + 人设 + 意图附录。
    ///
    /// planner 接口没有独立的 system-prompt 参数，所以这段内容作为 planner
    /// 上下文的首段传入。
    fn planner_system_block(&self) -> String {
        let mut parts = vec![self.session_system_prompt.clone()];
        if let Some(persona) = &self.persona_block {
            parts.push(persona.clone());
        }
        if let Some(appendix) = self.intent_appendix() {
            parts.push(appendix);
        }
        parts.join("\n\n")
    }

    /// 会话系统提示词 + 意图附录；人设只给 planner 用。
    fn agent_task_system_prompt(&self) -> String {
        let mut parts = vec![self.session_system_prompt.clone()];
        if let Some(appendix) = self.intent_appendix() {
            parts.push(appendix);
        }
        parts.join("\n\n")
    }

    /// 有界的 planner 上下文：配置、记忆与最近若干条消息。
    fn deep_context(&self) -> String {
        build_deep_context(
            Some(&self.planner_system_block()),
            self.memory_prompt.as_deref(),
            &self.messages,
            DEEP_CONTEXT_CHAR_LIMIT,
        )
    }
}

/// 解析一个回合：把用户消息落库、路由、套用意图配方并组装提示词。
///
/// 只做编排，每一步都委托给下面按阶段拆分的 helper——这样「路由」与
/// 「组装」的边界在代码里就是可见的，改动某一阶段不会牵连其他阶段。
pub(crate) async fn prepare_turn(
    state: &AppState,
    session_id: &str,
    content: &str,
    tools_enabled: Option<bool>,
    active_persona_id: Option<String>,
) -> Result<PreparedTurn> {
    let inputs = load_turn_inputs(state, session_id, content).await?;
    let route = route_turn(state, session_id, &inputs, content).await;

    let session_system_prompt = resolve_system_prompt(&inputs.session);
    let (persona_block, persona_model) =
        resolve_persona_context(state, &inputs.session, content, active_persona_id).await;
    let (model_id, skipped_models) =
        resolve_turn_model(state, &inputs.session, &route, persona_model.as_deref()).await;
    report_skipped_models(state, &skipped_models);

    let allowed_tools = resolve_allowed_tools(state, &inputs.session, &route);
    let memory_prompt = load_memory_prompt(state, content).await;
    let fast_messages = assemble_fast_messages(
        &session_system_prompt,
        persona_block.as_deref(),
        &route,
        memory_prompt.as_deref(),
        &inputs.messages,
    );
    let fast_messages =
        compress_context_if_needed(state, session_id, &model_id, fast_messages).await?;

    let policy = build_execution_policy(&route, allowed_tools, model_id);

    Ok(PreparedTurn {
        session: inputs.session,
        messages: inputs.messages,
        path: policy.path,
        tools_enabled: tools_enabled.unwrap_or(true),
        allowed_tools: policy.allowed_tools,
        model_id: policy.model_id,
        max_iterations: policy.max_iterations,
        session_system_prompt,
        persona_block,
        memory_prompt,
        fast_messages,
        route,
        user_content: content.to_string(),
    })
}

/// 一个回合的原始输入：会话、包含本轮用户消息的历史、以及用户消息 id。
struct TurnInputs {
    session: DbSession,
    messages: Vec<DbMessage>,
    user_message_id: String,
}

/// 读出会话与完整消息列表，并把本轮用户消息先落库。
///
/// 必须先写后读：`messages` 里要包含本轮用户输入，fast 与 deep 两条路径都
/// 直接复用这份列表。
async fn load_turn_inputs(
    state: &AppState,
    session_id: &str,
    content: &str,
) -> Result<TurnInputs> {
    let db = state.db.lock().await;

    let session = db
        .get_session(session_id)?
        .ok_or_else(|| AppError::NotFound("Session not found".to_string()))?;

    let user_msg = DbMessage {
        id: Uuid::new_v4().to_string(),
        session_id: session_id.to_string(),
        role: "user".to_string(),
        content: content.to_string(),
        tool_calls: None,
        tool_call_id: None,
        tool_name: None,
        tokens: None,
        created_at: Utc::now().to_rfc3339(),
    };

    db.insert_message(&user_msg)?;
    let messages = db.get_messages(session_id)?;

    Ok(TurnInputs {
        session,
        messages,
        user_message_id: user_msg.id,
    })
}

/// 用会话持久化的领域先验路由本轮消息，并把命中领域写回为下一轮先验。
///
/// 写回失败只记日志：路由本身已经成功，不该因为一次先验更新失败就让整条
/// 消息发送失败。
async fn route_turn(
    state: &AppState,
    session_id: &str,
    inputs: &TurnInputs,
    content: &str,
) -> RouteDecision {
    let pinned = inputs.session.intent_domain_mode == MODE_PINNED;
    let project_root: Option<std::path::PathBuf> = {
        let db = state.db.lock().await;
        inputs
            .session
            .project_id
            .as_deref()
            .and_then(|id| db.get_project(id).ok().flatten())
            .map(|project| std::path::PathBuf::from(project.path))
    };
    let recent_messages = recent_history(&inputs.messages, &inputs.user_message_id);
    let route = state
        .intent_router
        .route(RouteContext {
            session_id,
            content,
            recent_messages: &recent_messages,
            prior_domain: inputs.session.intent_domain_id.as_deref(),
            pinned,
            project_root: project_root.as_deref(),
        })
        .await;

    if let PriorWriteBack::Persist { domain_id } = prior_write_back(
        &inputs.session.intent_domain_mode,
        &route.chosen.domain_id,
        &route.chosen.domain_name,
    ) {
        let db = state.db.lock().await;
        if let Err(error) = db.set_session_intent_domain(session_id, Some(&domain_id), MODE_AUTO) {
            log::warn!(
                "failed to persist intent domain prior for session '{}': {}",
                session_id,
                error
            );
        }
    }

    emit_intent_resolved(state, session_id, &route);
    route
}

/// 路由只需要最近几条历史，且要排除本轮用户输入（它单独作为 `content`）。
fn recent_history(messages: &[DbMessage], user_message_id: &str) -> Vec<DbMessage> {
    let mut recent: Vec<DbMessage> = messages
        .iter()
        .filter(|message| message.id != user_message_id)
        .cloned()
        .collect();
    if recent.len() > ROUTE_HISTORY_LIMIT {
        recent = recent.split_off(recent.len() - ROUTE_HISTORY_LIMIT);
    }
    recent
}

/// 会话系统提示词；未配置或为空时回退到内置默认值。
fn resolve_system_prompt(session: &DbSession) -> String {
    session
        .system_prompt
        .as_ref()
        .filter(|s| !s.is_empty())
        .cloned()
        .unwrap_or_else(|| DEFAULT_SYSTEM_PROMPT.to_string())
}

/// 解析本轮生效的人设。
///
/// 返回 `(人设提示词块, 人设指定的模型名)`；未命中人设时两者都是 `None`。
/// `active_persona_id` 优先于会话绑定的人设。
async fn resolve_persona_context(
    state: &AppState,
    session: &DbSession,
    content: &str,
    active_persona_id: Option<String>,
) -> (Option<String>, Option<String>) {
    let effective_persona_id = active_persona_id.or_else(|| session.persona_id.clone());
    let project_dir = state
        .app_handle
        .path()
        .resource_dir()
        .ok()
        .and_then(|p| p.parent().map(|pp| pp.to_string_lossy().to_string()));
    let active_persona = state
        .persona
        .resolve(content, project_dir.as_deref(), effective_persona_id.as_deref())
        .await;

    (
        persona_block(&active_persona),
        persona_model_name(&active_persona),
    )
}

/// 模型优先级：意图 > 人设 > 会话，跳过未注册的候选。
///
/// 返回最终模型 id 与被跳过的候选列表（由调用方记日志）。
async fn resolve_turn_model(
    state: &AppState,
    session: &DbSession,
    route: &RouteDecision,
    persona_model: Option<&str>,
) -> (String, Vec<String>) {
    let providers = state.providers.lock().await;
    let default_model = providers.default_model_id().to_string();
    let resolution = resolve_model_id(
        route.chosen.config.model_id.as_deref(),
        persona_model,
        &session.model_id,
        Some(default_model.as_str()).filter(|id| !id.is_empty()),
        |id| providers.is_registered(id),
    );
    (resolution.model_id, resolution.skipped)
}

/// 把被跳过的模型同时写进服务端日志与前端执行日志。
fn report_skipped_models(state: &AppState, skipped_models: &[String]) {
    for skipped in skipped_models {
        log::warn!(
            "model '{}' is not registered; falling back to the next candidate",
            skipped
        );
        emit_execution_log(
            &state.app_handle,
            "warn",
            "intent",
            format!("模型 {} 不可用，已回退", skipped),
        );
    }
}

/// 工具白名单：会话配置与意图白名单取交集。
fn resolve_allowed_tools(
    state: &AppState,
    session: &DbSession,
    route: &RouteDecision,
) -> Option<Vec<String>> {
    let session_allowed_tools: Option<Vec<String>> = session.config.as_ref().and_then(|c| {
        serde_json::from_str::<serde_json::Value>(c)
            .ok()
            .and_then(|v| v.get("enabled_tools").cloned())
            .and_then(|v| serde_json::from_value(v).ok())
    });
    state.intent_router.resolve_tools(
        session_allowed_tools,
        route.chosen.config.enabled_tools.as_ref(),
    )
}

/// 本轮记忆上下文；召回失败时静默降级为「无记忆」。
async fn load_memory_prompt(state: &AppState, content: &str) -> Option<String> {
    state
        .memory
        .build_context_prompt(content, MEMORY_PROMPT_LIMIT)
        .await
        .ok()
        .flatten()
}

/// 组装 fast 路径消息：系统提示词、人设、意图附录、记忆、历史。
fn assemble_fast_messages(
    system_prompt: &str,
    persona: Option<&str>,
    route: &RouteDecision,
    memory: Option<&str>,
    messages: &[DbMessage],
) -> Vec<Message> {
    let mut fast_messages: Vec<Message> = Vec::new();
    fast_messages.push(Message::system(system_prompt.to_string()));
    if let Some(persona) = persona {
        fast_messages.push(Message::system(persona.to_string()));
    }
    if let Some(appendix) = intent_appendix(&route.chosen) {
        fast_messages.push(Message::system(appendix));
    }
    if let Some(memory) = memory {
        fast_messages.push(Message::system(memory.to_string()));
    }
    fast_messages.extend(history_messages(messages));
    fast_messages
}

/// 模型配置了上下文窗口时压缩历史；未配置则原样返回。
async fn compress_context_if_needed(
    state: &AppState,
    session_id: &str,
    model_id: &str,
    fast_messages: Vec<Message>,
) -> Result<Vec<Message>> {
    let context_window = state
        .config
        .lock()
        .await
        .get_model(model_id)
        .and_then(|m| m.context_window)
        .map(|v| v as usize);

    let Some(ctx) = context_window else {
        return Ok(fast_messages);
    };

    let db = state.db.lock().await;
    crate::features::session::lifecycle::compactor::compress_context(
        &db,
        session_id,
        fast_messages,
        ctx,
    )
}

// ── 模型解析 ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelResolution {
    pub model_id: String,
    /// 因为未注册而被跳过的非空候选。
    pub skipped: Vec<String>,
}

/// 解析本回合使用的模型。
///
/// 优先级：`intent.model_id` > 人设 `model_name` > `session.model_id`。
/// provider 注册表不认识的候选会被跳过（并回传，供调用方记日志），而不是
/// 让整条消息失败。注册表默认模型是最后兜底，这样会话里残留的旧模型名
/// 不会让消息发不出去。
pub(crate) fn resolve_model_id(
    intent_model: Option<&str>,
    persona_model: Option<&str>,
    session_model: &str,
    default_model: Option<&str>,
    is_registered: impl Fn(&str) -> bool,
) -> ModelResolution {
    let mut skipped: Vec<String> = Vec::new();

    let candidates = [intent_model, persona_model, Some(session_model)];
    for candidate in candidates.iter().flatten() {
        let candidate = candidate.trim();
        if candidate.is_empty() {
            continue;
        }
        if is_registered(candidate) {
            return ModelResolution {
                model_id: candidate.to_string(),
                skipped,
            };
        }
        if !skipped.iter().any(|s| s == candidate) {
            skipped.push(candidate.to_string());
        }
    }

    if let Some(default) = default_model.map(str::trim).filter(|id| !id.is_empty()) {
        if is_registered(default) {
            return ModelResolution {
                model_id: default.to_string(),
                skipped,
            };
        }
    }

    // 一个都没注册：保留最后一个非空候选，让 provider 层报出精确错误，
    // 而不是拿一个空 model id 去请求。
    let fallback = candidates
        .iter()
        .flatten()
        .map(|c| c.trim())
        .filter(|c| !c.is_empty())
        .next_back()
        .unwrap_or("")
        .to_string();

    ModelResolution {
        model_id: fallback,
        skipped,
    }
}

// ── 领域先验回写 ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PriorWriteBack {
    Persist { domain_id: String },
    Skip,
}

/// 判断本次路由结果是否要成为会话的下一轮先验。
///
/// 只有 `auto` 会话才学习领域：`pinned` 会话的领域由用户决定，不能被路由
/// 覆盖；合成结果（`domain_name` 为空）表示没有选中任何领域。
pub(crate) fn prior_write_back(mode: &str, domain_id: &str, domain_name: &str) -> PriorWriteBack {
    let is_auto = mode != MODE_PINNED;
    let has_real_domain = !domain_id.trim().is_empty() && !domain_name.trim().is_empty();

    if is_auto && has_real_domain {
        PriorWriteBack::Persist {
            domain_id: domain_id.to_string(),
        }
    } else {
        PriorWriteBack::Skip
    }
}

// ── 深度上下文 ────────────────────────────────────────────────────────────

/// 组装 planner 上下文：回合配置、记忆、最近
/// [`DEEP_CONTEXT_MESSAGE_LIMIT`] 条持久化消息，整体不超过 `limit` 字符。
///
/// 配置块在构造上就不会被截断：历史预算等于 `limit` 减去固定段落，所以
/// 截断丢掉的是旧对话文本，而不是会话指令。
pub(crate) fn build_deep_context(
    system_block: Option<&str>,
    memory: Option<&str>,
    messages: &[DbMessage],
    limit: usize,
) -> String {
    let mut fixed = String::new();

    if let Some(block) = system_block.map(str::trim).filter(|b| !b.is_empty()) {
        fixed.push_str("## Turn configuration\n");
        fixed.push_str(block);
        fixed.push_str("\n\n");
    }

    if let Some(memory) = memory.map(str::trim).filter(|m| !m.is_empty()) {
        fixed.push_str(memory);
        fixed.push_str("\n\n");
    }

    let history: Vec<String> = messages
        .iter()
        .rev()
        .take(DEEP_CONTEXT_MESSAGE_LIMIT)
        .rev()
        .map(|m| format!("{}: {}", m.role, m.content))
        .collect();
    let history_text = history.join("\n");

    let remaining = limit.saturating_sub(fixed.chars().count());
    let history_text = truncate_tail(&history_text, remaining);

    let mut block = fixed;
    block.push_str(&history_text);
    truncate_tail(&block, limit)
}

/// 截断时保留末尾 `limit` 个字符（即最新的上下文）。
fn truncate_tail(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut chars: Vec<char> = text.chars().rev().take(limit).collect();
    chars.reverse();
    chars.into_iter().collect()
}

// ── 深度失败的降级选择 ────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeepFailureAction {
    /// 什么都还没执行：可以安全地退回 fast 路径回答。
    FallbackToFast,
    /// 步骤已经跑过（产生了副作用）：只能返回已有结果。
    ReturnPartial,
}

/// 执行前失败可以降级；执行中失败绝不能重放副作用。
pub(crate) fn deep_failure_action(execution_started: bool) -> DeepFailureAction {
    if execution_started {
        DeepFailureAction::ReturnPartial
    } else {
        DeepFailureAction::FallbackToFast
    }
}

// ── 执行辅助 ──────────────────────────────────────────────────────────────

enum DeepAttempt {
    Completed { text: String },
    Partial { text: String, failed_step: String },
    PlannerFailed { error: String },
}

/// fast 路径的非流式执行：一次性拿到完整回答。
async fn execute_fast_non_streaming(
    state: &AppState,
    turn: &PreparedTurn,
) -> Result<crate::llm::provider::TurnOutcome> {
    let mut agent = ToolLoop::new(state.providers.clone(), state.tools.clone());
    if let Some(max_iter) = turn.max_iterations {
        agent = agent.with_max_iterations(max_iter);
    }

    agent
        .run(
            &turn.model_id,
            turn.fast_messages.clone(),
            turn.tools_enabled,
            turn.allowed_tools.clone(),
        )
        .await
}

/// deep 路径的非流式执行：planner 失败则降级到 fast，执行失败则返回部分结果。
async fn execute_deep_non_streaming(state: &AppState, turn: &PreparedTurn) -> Result<String> {
    let attempt = run_deep_turn(state, turn, &state.app_handle, false).await;

    match attempt {
        DeepAttempt::PlannerFailed { error } => {
            log::error!("deep planning failed, falling back to fast path: {}", error);
            emit_execution_log(
                &state.app_handle,
                "error",
                "planner",
                format!("计划生成失败，降级到普通聊天模式: {}", error),
            );
            // 降级分支的判定与流式路径共用，避免「规划失败 vs 步骤失败」
            // 两条规则各写一份后走偏。
            match deep_failure_action(false) {
                DeepFailureAction::FallbackToFast => execute_fast_non_streaming(state, turn)
                    .await
                    .map(|outcome| outcome.output),
                DeepFailureAction::ReturnPartial => unreachable!(),
            }
        }
        DeepAttempt::Completed { text } => Ok(text),
        DeepAttempt::Partial { text, failed_step } => {
            log::warn!("deep execution failed at step '{}'; returning partial result", failed_step);
            Ok(text)
        }
    }
}

/// 根据工具是否失败，计算上报给前端的工具调用状态。
///
/// 状态值与前端 `ChatArea.tsx` 的渲染分支一一对应：
/// `completed` 显示绿色对勾，`failed` 显示红色叉号。
fn tool_status(is_error: bool) -> &'static str {
    if is_error { "failed" } else { "completed" }
}

/// fast 路径的流式执行：把增量文本、工具调用与阶段变化逐条推给前端。
///
/// 返回拼接好的完整回答；流式 chunk 只用于展示，不参与落库。
async fn execute_fast_streaming(
    state: &AppState,
    app_handle: &tauri::AppHandle,
    turn: &PreparedTurn,
) -> Result<String> {
    let mut agent = ToolLoop::new(state.providers.clone(), state.tools.clone());
    if let Some(max_iter) = turn.max_iterations {
        agent = agent.with_max_iterations(max_iter);
    }

    let mut stream = agent
        .run_stream(
            &turn.model_id,
            turn.fast_messages.clone(),
            turn.tools_enabled,
            turn.allowed_tools.clone(),
        )
        .await?;

    let mut full_content = String::new();
    let sess_id_for_messages = turn.session.id.clone();

    // 阶段：thinking —— 模型正在生成首轮回复。
    emit_phase(app_handle, "thinking");

    while let Some(chunk) = stream.recv().await {
        match chunk {
            crate::agent::fast_loop::StreamEvent::Content(text) => {
                full_content.push_str(&text);
                emit_content(app_handle, text);
            }
            crate::agent::fast_loop::StreamEvent::ToolCall(tool_call) => {
                // 先把 assistant 的工具调用落库，保证顺序是
                // assistant(call) → tool(result) → assistant(final)。
                persist_tool_call(state, &sess_id_for_messages, &tool_call).await;
                emit_tool_status(
                    app_handle,
                    ToolCallEvent {
                        id: tool_call.id,
                        name: tool_call.name,
                        status: "calling".to_string(),
                        result: None,
                        is_error: false,
                        arguments: Some(tool_call.arguments),
                    },
                    Some("executing_tool"),
                );
            }
            crate::agent::fast_loop::StreamEvent::ToolResult(tool_result) => {
                // 工具结果落库后才能进入会话历史。
                persist_tool_result(state, &sess_id_for_messages, &tool_result).await;

                // 用结构化错误标志判定失败，而不是匹配没有生产者的字符串前缀。
                emit_tool_status(
                    app_handle,
                    ToolCallEvent {
                        id: tool_result.call_id,
                        name: tool_result.name,
                        status: tool_status(tool_result.is_error).to_string(),
                        result: Some(tool_result.result),
                        is_error: tool_result.is_error,
                        arguments: None,
                    },
                    None,
                );
                // 工具结果返回后模型会继续思考 —— 重新上报 thinking 阶段。
                emit_phase(app_handle, "thinking");
            }
            crate::agent::fast_loop::StreamEvent::Error(message) => {
                log::error!("fast streaming failed: {}", message);
                emit_done(app_handle, String::new(), "failed");
                break;
            }
            crate::agent::fast_loop::StreamEvent::Done => {
                emit_done(app_handle, String::new(), "completed");
                break;
            }
        }
    }

    Ok(full_content)
}

/// 把一次 assistant 工具调用写入消息表（文本为空，只有 `tool_calls`）。
///
/// 先于工具结果落库，历史顺序才能保持
/// `assistant(call)` → `tool(result)` → `assistant(final)`。
async fn persist_tool_call(
    state: &AppState,
    session_id: &str,
    call: &crate::agent::fast_loop::ToolCallInfo,
) {
    use rig::message::{ToolCall, ToolFunction, ToolName};

    let Ok(name) = ToolName::new(call.name.clone()) else {
        log::warn!("skipping persistence of tool call with invalid name: {}", call.name);
        return;
    };
    let tool_call = ToolCall::from_wire(
        call.id.clone(),
        ToolFunction::new(name, call.arguments.clone()),
    );

    let row = DbMessage {
        id: Uuid::new_v4().to_string(),
        session_id: session_id.to_string(),
        role: "assistant".to_string(),
        content: String::new(),
        tool_calls: serde_json::to_string(&vec![tool_call]).ok(),
        tool_call_id: None,
        tool_name: None,
        tokens: None,
        created_at: Utc::now().to_rfc3339(),
    };

    let db = state.db.lock().await;
    if let Err(e) = db.insert_message(&row) {
        log::error!("failed to persist tool call '{}': {}", call.id, e);
    }
}

/// 把一条工具结果写入消息表。
///
/// 写入失败不中断流式响应，但必须留下日志：静默丢弃会让工具输出从会话
/// 历史里消失，模型下一轮就看不到自己刚查到的内容。
async fn persist_tool_result(
    state: &AppState,
    session_id: &str,
    tool_result: &crate::agent::fast_loop::ToolResultInfo,
) {
    let db = state.db.lock().await;
    let tool_msg = DbMessage {
        id: Uuid::new_v4().to_string(),
        session_id: session_id.to_string(),
        role: "tool".to_string(),
        content: tool_result.result.clone(),
        tool_calls: None,
        tool_call_id: Some(tool_result.call_id.clone()),
        tool_name: Some(tool_result.name.clone()),
        tokens: None,
        created_at: Utc::now().to_rfc3339(),
    };
    if let Err(e) = db.insert_message(&tool_msg) {
        log::error!(
            "failed to persist tool result for call '{}': {}",
            tool_msg.tool_call_id.as_deref().unwrap_or("<none>"),
            e
        );
    }
}

/// 发送一个只携带阶段名的空 chunk（文本与工具调用字段都为空）。
fn emit_phase(app_handle: &tauri::AppHandle, phase: &str) {
    let _ = app_handle.emit("stream_chunk", StreamChunk {
        content: String::new(),
        done: false,
        tool_calls: None,
        phase: Some(phase.to_string()),
    });
}

/// 发送一个增量文本 chunk，不改变当前阶段。
fn emit_content(app_handle: &tauri::AppHandle, text: String) {
    let _ = app_handle.emit("stream_chunk", StreamChunk {
        content: text,
        done: false,
        tool_calls: None,
        phase: None,
    });
}

/// 发送一次工具调用状态变化，`phase` 为 `None` 时保持前端当前阶段。
fn emit_tool_status(
    app_handle: &tauri::AppHandle,
    tool_call: ToolCallEvent,
    phase: Option<&str>,
) {
    let _ = app_handle.emit("stream_chunk", StreamChunk {
        content: String::new(),
        done: false,
        tool_calls: Some(vec![tool_call]),
        phase: phase.map(str::to_string),
    });
}

/// 发送结束 chunk；`phase` 取 `completed` 或 `failed`。
fn emit_done(app_handle: &tauri::AppHandle, content: String, phase: &str) {
    let _ = app_handle.emit("stream_chunk", StreamChunk {
        content,
        done: true,
        tool_calls: None,
        phase: Some(phase.to_string()),
    });
}

/// deep 路径的流式执行：计划执行结果以 `plan_progress` 事件推送，
/// 最终文本作为一个结束 chunk 发出。
async fn execute_deep_streaming(
    state: &AppState,
    app_handle: &tauri::AppHandle,
    turn: &PreparedTurn,
) -> Result<String> {
    let attempt = run_deep_turn(state, turn, app_handle, true).await;

    match attempt {
        DeepAttempt::PlannerFailed { error } => {
            log::error!("deep planning failed, falling back to fast path: {}", error);
            emit_execution_log(
                app_handle,
                "error",
                "planner",
                format!("计划生成失败，降级到普通聊天模式: {}", error),
            );
            match deep_failure_action(false) {
                DeepFailureAction::FallbackToFast => {
                    execute_fast_streaming(state, app_handle, turn).await
                }
                DeepFailureAction::ReturnPartial => unreachable!(),
            }
        }
        DeepAttempt::Completed { text } => {
            emit_done(app_handle, text.clone(), "completed");
            Ok(text)
        }
        DeepAttempt::Partial { text, failed_step } => {
            log::warn!("deep execution failed at step '{}'; returning partial result", failed_step);
            // runtime 已经发过 `plan_failed`；这里只是把聊天回合本身标记为
            // 失败，不重跑任何步骤。
            emit_done(app_handle, text.clone(), "failed");
            Ok(text)
        }
    }
}

/// 以 `Interactive` 作用域规划并执行一个深度回合。
///
/// 执行期间注册 `ExecutionHandle`，让现有的 cancel 命令能停掉它；无论成功
/// 失败都会移除句柄，避免 `active_executions` 泄漏。
async fn run_deep_turn(
    state: &AppState,
    turn: &PreparedTurn,
    app_handle: &tauri::AppHandle,
    forward_progress: bool,
) -> DeepAttempt {
    let plan = match plan_deep_turn(state, turn).await {
        Ok(plan) => plan,
        Err(error) => return DeepAttempt::PlannerFailed { error },
    };

    let total_steps = plan.steps.len();
    emit_execution_log(
        app_handle,
        "info",
        "planner",
        format!("计划生成成功: {} 步", total_steps),
    );

    // 不 emit `plan_generated`：聊天回合不能弹出计划确认框，进度事件才是
    // 唯一的 UI 信号。
    let handle = ExecutionHandle::new(turn.session.id.clone(), plan.id.clone());
    let cancel_flag = handle.cancel_flag.clone();
    let pause_flag = handle.pause_flag.clone();
    state
        .active_executions
        .lock()
        .await
        .insert(turn.session.id.clone(), handle);

    let runtime = ExecutionRuntime::new(
        state.providers.clone(),
        state.tools.clone(),
        state.db.clone(),
    )
    .with_app_handle(app_handle.clone())
    .with_tool_policy(turn.allowed_tools.clone(), turn.tools_enabled)
    .with_system_prompt(Some(turn.agent_task_system_prompt()))
    .with_max_iterations(turn.max_iterations);

    let (event_tx, event_rx) = tokio::sync::mpsc::channel::<PlanProgressEvent>(32);
    let forwarder = spawn_progress_forwarder(app_handle, event_rx, forward_progress);

    let result = runtime
        .run(plan, event_tx, cancel_flag, pause_flag, PlanRunMode::Interactive)
        .await;

    if let Some(forwarder) = forwarder {
        let _ = forwarder.await;
    }
    state
        .active_executions
        .lock()
        .await
        .remove(&turn.session.id);

    match result {
        Ok(outcome) => DeepAttempt::Completed {
            text: deep_success_text(&outcome.completed, total_steps),
        },
        Err(failure) => {
            match deep_failure_action(true) {
                DeepFailureAction::ReturnPartial => {
                    emit_execution_log(
                        app_handle,
                        "error",
                        "execution",
                        format!(
                            "执行在第「{}」步失败: {}",
                            failure.failed_step, failure.message
                        ),
                    );
                    DeepAttempt::Partial {
                        text: deep_partial_text(&failure),
                        failed_step: failure.failed_step,
                    }
                }
                DeepFailureAction::FallbackToFast => unreachable!(),
            }
        }
    }
}

/// 让 planner 生成一个深度执行计划。
///
/// 失败时按 [`deep_failure_action`] 判定：规划阶段还没执行任何步骤，因此
/// 一律允许调用方降级到 fast 路径。
async fn plan_deep_turn(
    state: &AppState,
    turn: &PreparedTurn,
) -> std::result::Result<
    crate::agent::orchestrator::plan_types::ExecutionPlan,
    String,
> {
    let planner = LlmPlanner::new(state.providers.clone(), state.tools.clone());
    let context = turn.deep_context();

    match planner
        .generate_plan_with_policy(
            &turn.user_content,
            &turn.session.id,
            Some(&turn.model_id),
            turn.allowed_tools.as_deref(),
            turn.tools_enabled,
            Some(&context),
        )
        .await
    {
        Ok(plan) => Ok(plan),
        Err(error) => match deep_failure_action(false) {
            DeepFailureAction::FallbackToFast => Err(error.to_string()),
            DeepFailureAction::ReturnPartial => unreachable!(),
        },
    }
}

/// 把执行进度事件转发给前端；非流式模式下直接丢弃接收端。
///
/// 返回转发任务的句柄，调用方必须 await 它，否则最后的进度事件可能在
/// 回合结束前丢掉。
fn spawn_progress_forwarder(
    app_handle: &tauri::AppHandle,
    event_rx: tokio::sync::mpsc::Receiver<PlanProgressEvent>,
    forward_progress: bool,
) -> Option<tokio::task::JoinHandle<()>> {
    if !forward_progress {
        drop(event_rx);
        return None;
    }

    let handle = app_handle.clone();
    let mut rx = event_rx;
    Some(tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            let _ = handle.emit("plan_progress", event);
        }
    }))
}

/// 为已完成的计划挑选要展示给用户的助手文本。
///
/// 优先取最后一个产出了文本的步骤（最终答案通常在那里）；都没有时退化为
/// 紧凑的步骤摘要。
fn deep_success_text(completed: &[CompletedStep], total_steps: usize) -> String {
    if let Some(text) = completed.iter().rev().find_map(|step| match &step.result {
        serde_json::Value::String(text) if !text.trim().is_empty() => Some(text.clone()),
        _ => None,
    }) {
        return text;
    }

    if completed.is_empty() {
        return format!("计划已执行完成（{} 步），但没有产生文本结果。", total_steps);
    }

    completed
        .iter()
        .map(|step| format!("- {}：{}", step.label, step_summary(&step.result)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 汇报已完成步骤，并附上失败步骤与原因。
fn deep_partial_text(failure: &PlanFailure) -> String {
    let reason = format!(
        "执行在第「{}」步失败：{}",
        failure.failed_step, failure.message
    );
    match failure.combined_result() {
        Some(text) => format!("{}\n\n> {}", text, reason),
        None => reason,
    }
}

/// 把单步结果压成一行摘要，最多 200 字符。
fn step_summary(result: &serde_json::Value) -> String {
    match result {
        serde_json::Value::String(text) => text.chars().take(200).collect(),
        other => other.to_string().chars().take(200).collect(),
    }
}

// ── 零散共享辅助 ──────────────────────────────────────────────────────────

/// 把路由结果投影成执行配方。
///
/// `uncertain` 被刻意忽略：它只影响告诉用户什么，不影响这个回合被允许做
/// 什么。
fn build_execution_policy(
    route: &RouteDecision,
    allowed_tools: Option<Vec<String>>,
    model_id: String,
) -> ExecutionPolicy {
    ExecutionPolicy {
        path: route.chosen.path,
        allowed_tools,
        model_id,
        system_prompt_appendix: route.chosen.config.system_prompt_appendix.clone(),
        max_iterations: route.chosen.config.max_iterations,
    }
}

/// 意图附录：`[Intent: id (name)]` 前缀 + 配置的补充提示词。
fn intent_appendix(intent: &IntentResult) -> Option<String> {
    intent
        .config
        .system_prompt_appendix
        .as_ref()
        .filter(|appendix| !appendix.trim().is_empty())
        .map(|appendix| format!("[Intent: {} ({})]\n{}", intent.id, intent.name, appendix))
}

/// 从人设解析结果中取出人设本体；未命中时返回 `None`。
fn persona_info(resolution: &PersonaResolution) -> Option<&PersonaInfo> {
    match resolution {
        PersonaResolution::Manual(persona)
        | PersonaResolution::Auto(persona)
        | PersonaResolution::Default(persona) => Some(persona),
        PersonaResolution::None => None,
    }
}

/// 人设提示词块：`Your current role: <emoji> <title>` 加人设正文。
fn persona_block(resolution: &PersonaResolution) -> Option<String> {
    persona_info(resolution).map(|persona| {
        format!(
            "Your current role: {} {}\n{}",
            persona.emoji, persona.title, persona.system_prompt
        )
    })
}

/// 人设自带的模型名；空串视为未指定。
fn persona_model_name(resolution: &PersonaResolution) -> Option<String> {
    persona_info(resolution)
        .map(|persona| persona.model_name.trim().to_string())
        .filter(|model| !model.is_empty())
}

/// 通过 `execution_log` 事件把一条执行日志推给前端。
fn emit_execution_log(
    app_handle: &tauri::AppHandle,
    level: &str,
    step: &str,
    message: String,
) {
    let entry = ExecutionLogEntry::new(level, step, message);
    let _ = app_handle.emit("execution_log", entry);
}

#[derive(Serialize, Clone)]
struct IntentResolvedEvent {
    session_id: String,
    domain_id: String,
    domain_name: String,
    intent_id: String,
    intent_name: String,
    confidence: f32,
    uncertain: bool,
    source: DecisionSource,
    candidates: Vec<crate::llm::intent::IntentCandidate>,
    elapsed_ms: u64,
    llm_calls: usize,
    probe_used: bool,
}

/// 把本回合的路由决策 emit 给前端（用于意图徽标）。
fn emit_intent_resolved(state: &AppState, session_id: &str, route: &RouteDecision) {
    let event = IntentResolvedEvent {
        session_id: session_id.to_string(),
        domain_id: route.chosen.domain_id.clone(),
        domain_name: route.chosen.domain_name.clone(),
        intent_id: route.chosen.id.clone(),
        intent_name: route.chosen.name.clone(),
        confidence: route.confidence,
        uncertain: route.uncertain,
        source: route.source,
        candidates: route.candidates.clone(),
        elapsed_ms: route.evidence.elapsed_ms,
        llm_calls: route.evidence.llm_calls,
        probe_used: route.evidence.probe_used,
    };
    let _ = state.app_handle.emit("intent_resolved", event);
}

/// 异步触发回合后的生命周期钩子：自动标题与自动摘要。
///
/// 两者都是尽力而为：失败只丢弃结果，不影响已经返回给用户的回复。
fn spawn_lifecycle_hooks(state: &AppState, session_id: &str, model_id: &str) {
    let lifecycle = state.lifecycle.clone();
    let sid = session_id.to_string();
    let mid = model_id.to_string();
    tokio::spawn(async move {
        let _ =
            crate::features::session::lifecycle::titler::maybe_generate_title(&lifecycle, &sid, &mid)
                .await;
    });

    let lifecycle2 = state.lifecycle.clone();
    let sid2 = session_id.to_string();
    let mid2 = model_id.to_string();
    tokio::spawn(async move {
        let _ = crate::features::session::lifecycle::summarizer::maybe_generate_summary(
            &lifecycle2,
            &sid2,
            &mid2,
        )
        .await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db_message(role: &str, content: &str) -> DbMessage {
        DbMessage {
            id: Uuid::new_v4().to_string(),
            session_id: "session-test".to_string(),
            role: role.to_string(),
            content: content.to_string(),
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
            tokens: None,
            created_at: "2026-11-05T00:00:00Z".to_string(),
        }
    }

    fn registry(ids: &'static [&'static str]) -> impl Fn(&str) -> bool {
        move |id| ids.contains(&id)
    }

    #[test]
    fn model_priority_prefers_intent_when_registered() {
        let resolution = resolve_model_id(
            Some("intent-model"),
            Some("persona-model"),
            "session-model",
            Some("default-model"),
            registry(&["intent-model", "persona-model", "session-model", "default-model"]),
        );

        assert_eq!(resolution.model_id, "intent-model");
        assert!(resolution.skipped.is_empty());
    }

    #[test]
    fn model_priority_falls_back_to_persona_when_intent_is_missing() {
        let resolution = resolve_model_id(
            Some("deleted-model"),
            Some("persona-model"),
            "session-model",
            None,
            registry(&["persona-model", "session-model"]),
        );

        assert_eq!(resolution.model_id, "persona-model");
        assert_eq!(resolution.skipped, vec!["deleted-model".to_string()]);
    }

    #[test]
    fn model_priority_falls_back_to_session_when_intent_and_persona_are_missing() {
        let resolution = resolve_model_id(
            Some("deleted-intent"),
            Some("deleted-persona"),
            "session-model",
            None,
            registry(&["session-model"]),
        );

        assert_eq!(resolution.model_id, "session-model");
        assert_eq!(
            resolution.skipped,
            vec!["deleted-intent".to_string(), "deleted-persona".to_string()]
        );
    }

    #[test]
    fn model_resolution_uses_registry_default_when_every_candidate_is_missing() {
        let resolution = resolve_model_id(
            Some("deleted-intent"),
            None,
            "deleted-session",
            Some("default-model"),
            registry(&["default-model"]),
        );

        assert_eq!(resolution.model_id, "default-model");
        assert_eq!(
            resolution.skipped,
            vec!["deleted-intent".to_string(), "deleted-session".to_string()]
        );
    }

    #[test]
    fn prior_write_back_persists_for_auto_with_a_real_domain() {
        assert_eq!(
            prior_write_back("auto", "domain-a", "程序开发"),
            PriorWriteBack::Persist {
                domain_id: "domain-a".to_string()
            }
        );
    }

    #[test]
    fn prior_write_back_skips_pinned_sessions() {
        assert_eq!(
            prior_write_back("pinned", "domain-a", "程序开发"),
            PriorWriteBack::Skip
        );
    }

    #[test]
    fn tool_status_is_failed_when_tool_reports_error() {
        assert_eq!(tool_status(true), "failed");
    }

    #[test]
    fn tool_status_is_completed_when_tool_succeeds() {
        assert_eq!(tool_status(false), "completed");
    }

    #[test]
    fn prior_write_back_skips_the_synthetic_result() {
        // 合成意图复用了内置领域 id，但不上报领域名：没有任何领域行被选中，
        // 因此不能写回先验。
        assert_eq!(
            prior_write_back("auto", "builtin.software_dev", ""),
            PriorWriteBack::Skip
        );
    }

    #[test]
    fn deep_context_keeps_only_the_last_six_messages() {
        let messages: Vec<DbMessage> = (1..=8)
            .map(|index| db_message("user", &format!("MESSAGE_{}", index)))
            .collect();

        let context = build_deep_context(None, None, &messages, DEEP_CONTEXT_CHAR_LIMIT);

        assert!(!context.contains("MESSAGE_1"));
        assert!(!context.contains("MESSAGE_2"));
        assert!(context.contains("MESSAGE_3"));
        assert!(context.contains("MESSAGE_8"));
    }

    #[test]
    fn deep_context_truncates_the_whole_block_and_keeps_configuration() {
        let messages: Vec<DbMessage> = (0..6)
            .map(|index| db_message("assistant", &format!("{}{}", "x".repeat(1500), index)))
            .collect();

        let context = build_deep_context(
            Some("SYSTEM_BLOCK"),
            Some("MEMORY_BLOCK"),
            &messages,
            DEEP_CONTEXT_CHAR_LIMIT,
        );

        assert!(context.chars().count() <= DEEP_CONTEXT_CHAR_LIMIT);
        assert!(context.contains("SYSTEM_BLOCK"));
        assert!(context.contains("MEMORY_BLOCK"));
    }

    #[test]
    fn deep_failure_before_execution_falls_back_to_fast() {
        assert_eq!(
            deep_failure_action(false),
            DeepFailureAction::FallbackToFast
        );
    }

    #[test]
    fn deep_failure_after_execution_started_returns_partial() {
        assert_eq!(deep_failure_action(true), DeepFailureAction::ReturnPartial);
    }

    fn route_decision(uncertain: bool) -> RouteDecision {
        RouteDecision {
            chosen: IntentResult {
                id: "test.intent".to_string(),
                name: "实现功能".to_string(),
                domain_id: "test.domain".to_string(),
                domain_name: "程序开发".to_string(),
                config: crate::llm::intent::IntentConfig {
                    system_prompt_appendix: Some("be strict".to_string()),
                    enabled_tools: Some(vec!["file_system".to_string()]),
                    model_id: Some("model-a".to_string()),
                    max_iterations: Some(7),
                    auto_escalate: false,
                },
                path: PathSelection::Deep,
                confidence: 0.4,
            },
            confidence: 0.4,
            uncertain,
            source: DecisionSource::Llm,
            candidates: vec![crate::llm::intent::IntentCandidate {
                intent_id: "test.intent".to_string(),
                name: "实现功能".to_string(),
                domain_id: "test.domain".to_string(),
                score: 0.4,
                signal: DecisionSource::Llm,
            }],
            evidence: crate::llm::intent::DecisionEvidence {
                llm_calls: 1,
                elapsed_ms: 12,
                ..Default::default()
            },
        }
    }

    #[test]
    fn execution_policy_projects_the_chosen_intent() {
        let decision = route_decision(false);
        let policy = build_execution_policy(
            &decision,
            Some(vec!["file_system".to_string()]),
            "model-a".to_string(),
        );

        assert_eq!(policy.path, PathSelection::Deep);
        assert_eq!(policy.model_id, "model-a");
        assert_eq!(policy.max_iterations, Some(7));
        assert_eq!(
            policy.system_prompt_appendix.as_deref(),
            Some("be strict")
        );
        assert_eq!(
            policy.allowed_tools,
            Some(vec!["file_system".to_string()])
        );
    }

    #[test]
    fn uncertainty_does_not_change_the_tool_policy() {
        let tools = Some(vec!["file_system".to_string()]);
        let certain =
            build_execution_policy(&route_decision(false), tools.clone(), "m".to_string());
        let uncertain = build_execution_policy(&route_decision(true), tools, "m".to_string());

        assert_eq!(certain.allowed_tools, uncertain.allowed_tools);
    }

    #[test]
    fn intent_resolved_event_exposes_uncertainty_and_drops_widened() {
        let decision = route_decision(true);
        let event = IntentResolvedEvent {
            session_id: "session".to_string(),
            domain_id: decision.chosen.domain_id.clone(),
            domain_name: decision.chosen.domain_name.clone(),
            intent_id: decision.chosen.id.clone(),
            intent_name: decision.chosen.name.clone(),
            confidence: decision.confidence,
            uncertain: decision.uncertain,
            source: decision.source,
            candidates: decision.candidates.clone(),
            elapsed_ms: decision.evidence.elapsed_ms,
            llm_calls: decision.evidence.llm_calls,
            probe_used: decision.evidence.probe_used,
        };

        let value = serde_json::to_value(event).expect("event serializes");

        assert_eq!(value["uncertain"], true);
        assert!(value.get("widened").is_none());
        assert_eq!(value["candidates"][0]["intent_id"], "test.intent");
    }
    // ── task 6：transcript 落库与历史还原 ────────────────────────────────

    fn tool_call(id: &str, name: &str, args: serde_json::Value) -> rig::message::ToolCall {
        use rig::message::{ToolCall, ToolFunction, ToolName};
        ToolCall::from_wire(
            id.to_string(),
            ToolFunction::new(ToolName::new(name).expect("合法工具名"), args),
        )
    }

    fn tool_result_message(id: &str, name: &str, content: &str) -> Message {
        use rig::message::{CallId, ToolName};
        Message::tool_result(
            CallId::from_wire(id.to_string()),
            ToolName::new(name).expect("合法工具名"),
            content.to_string(),
        )
    }

    #[test]
    fn transcript_rows_keep_assistant_tool_tool_order() {
        use rig::message::{AssistantContent, AssistantMessage};

        let messages = vec![
            Message::Assistant(AssistantMessage::new(vec![
                AssistantContent::text("let me compute"),
                AssistantContent::ToolCall(tool_call(
                    "call_1",
                    "calculator",
                    serde_json::json!({ "expression": "1 + 1" }),
                )),
            ])),
            tool_result_message("call_1", "calculator", "{\"result\":2}"),
            Message::assistant("答案是 2"),
        ];

        let rows = transcript_rows("s1", &messages);
        let roles: Vec<&str> = rows.iter().map(|row| row.role.as_str()).collect();
        assert_eq!(roles, vec!["assistant", "tool", "assistant"]);

        assert!(rows[0]
            .tool_calls
            .as_deref()
            .unwrap_or_default()
            .contains("call_1"));
        assert_eq!(rows[1].tool_name.as_deref(), Some("calculator"));
        assert_eq!(rows[1].tool_call_id.as_deref(), Some("call_1"));
    }

    #[test]
    fn history_restores_tool_call_and_real_tool_name() {
        use rig::message::{AssistantContent, UserContent};

        let mut assistant = db_message("assistant", "let me compute");
        assistant.tool_calls = Some(
            serde_json::to_string(&vec![tool_call(
                "call_1",
                "calculator",
                serde_json::json!({ "expression": "1 + 1" }),
            )])
            .expect("序列化工具调用"),
        );

        let mut tool = db_message("tool", "{\"result\":2}");
        tool.tool_call_id = Some("call_1".to_string());
        tool.tool_name = Some("calculator".to_string());

        let restored = history_messages(&[assistant, tool]);

        assert_eq!(restored.len(), 2);
        match &restored[0] {
            Message::Assistant(message) => assert!(message.content.iter().any(|item| {
                matches!(item, AssistantContent::ToolCall(call) if call.id.to_string() == "call_1")
            })),
            other => panic!("第一条应为 assistant: {:?}", other),
        }
        match &restored[1] {
            Message::User { content } => {
                let result = content
                    .iter()
                    .find_map(|item| match item {
                        UserContent::ToolResult(result) => Some(result),
                        _ => None,
                    })
                    .expect("应含工具结果");
                assert_eq!(result.name.to_string(), "calculator");
            }
            other => panic!("第二条应为 user(tool_result): {:?}", other),
        }
    }

    #[test]
    fn orphan_tool_row_is_skipped() {
        let mut orphan = db_message("tool", "stale result");
        orphan.tool_call_id = Some("missing-call".to_string());
        orphan.tool_name = Some("calculator".to_string());

        assert!(history_messages(&[orphan]).is_empty());
    }
}

