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
use crate::app::commands::{StreamChunk, ToolCallEvent};
use crate::app::state::AppState;
use crate::core::db::models::{Message as DbMessage, Session as DbSession};
use crate::core::error::{AppError, Result};
use crate::features::persona::{PersonaInfo, PersonaResolution};
use crate::llm::intent::{IntentResult, PathSelection, RouteOutcome, RouteRequest, RouteSource};
use rig::completion::Message;

/// Rebuild a persisted tool result as a Rig user message.
///
/// Rig 0.45 identifies a tool result by its call id and the tool name; the
/// database persists only the call id, so the placeholder name `tool` is used
/// for stored rows.
fn tool_result_message(m: &DbMessage) -> Message {
    use rig::message::{CallId, ToolName};

    let call = CallId::from_wire(m.tool_call_id.clone().unwrap_or_default());
    match ToolName::new("tool") {
        Ok(name) => Message::tool_result(call, name, m.content.clone()),
        Err(_) => Message::user(m.content.clone()),
    }
}

/// Default system prompt used when user hasn't configured one.
/// Guides the model toward clear, well-structured, high-quality responses.
const DEFAULT_SYSTEM_PROMPT: &str = "You are a helpful, respectful AI assistant. \
Provide clear, accurate, and well-structured responses. \
Use the same language as the user's input. \
When responding in Chinese, ensure proper grammar and natural phrasing. \
If you don't know something, say so rather than making up information.";

/// Upper bound for the block handed to the planner as conversation context.
const DEEP_CONTEXT_CHAR_LIMIT: usize = 4000;
/// How many persisted messages the deep path includes as recent context.
const DEEP_CONTEXT_MESSAGE_LIMIT: usize = 6;
/// Memory rows included in the prompt (same number as the fast path).
const MEMORY_PROMPT_LIMIT: i64 = 5;

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
        intent_domain_mode: "auto".to_string(),
    };

    let db = state.db.lock().await;
    db.create_session(&sess)?;
    // `create_session` resolves the inherited default domain in SQL, so the
    // in-memory value would report `None`. Re-read the row so the caller gets
    // the actual domain state (used by the session header selector).
    let stored = db.get_session(&sess.id)?.unwrap_or(sess);

    Ok(stored)
}

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

#[tauri::command]
pub async fn archive_session(state: State<'_, AppState>, id: String) -> Result<()> {
    let db = state.db.lock().await;
    db.set_session_archived(&id, true)
}

#[tauri::command]
pub async fn unarchive_session(state: State<'_, AppState>, id: String) -> Result<()> {
    let db = state.db.lock().await;
    db.set_session_archived(&id, false)
}

#[tauri::command]
pub async fn list_archived_sessions(state: State<'_, AppState>) -> Result<Vec<DbSession>> {
    let db = state.db.lock().await;
    let all = db.list_sessions()?;
    Ok(all.into_iter().filter(|s| s.archived).collect())
}

#[tauri::command]
pub async fn get_session(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<DbSession>> {
    let db = state.db.lock().await;
    db.get_session(&id)
}

#[tauri::command]
pub async fn delete_session(state: State<'_, AppState>, id: String) -> Result<()> {
    let db = state.db.lock().await;
    db.delete_session(&id)
}

#[tauri::command]
pub async fn update_session_title(
    state: State<'_, AppState>,
    id: String,
    title: String,
) -> Result<()> {
    let db = state.db.lock().await;
    db.update_session_title(&id, &title)
}

#[tauri::command]
pub async fn update_session_model(
    state: State<'_, AppState>,
    id: String,
    model_id: String,
) -> Result<()> {
    let db = state.db.lock().await;
    db.update_session_model(&id, &model_id)
}

#[tauri::command]
pub async fn update_session_system_prompt(
    state: State<'_, AppState>,
    id: String,
    system_prompt: String,
) -> Result<()> {
    let db = state.db.lock().await;
    db.update_session_system_prompt(&id, &system_prompt)
}

#[tauri::command]
pub async fn update_session_config(
    state: State<'_, AppState>,
    id: String,
    config: String,
) -> Result<()> {
    let db = state.db.lock().await;
    db.update_session_config(&id, &config)
}

#[tauri::command]
pub async fn clear_session(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<()> {
    let db = state.db.lock().await;
    db.clear_messages(&session_id)
}

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

    let assistant_content = match turn.path {
        PathSelection::Deep => {
            execute_deep_non_streaming(state.inner(), &turn).await?
        }
        PathSelection::Fast => execute_fast_non_streaming(state.inner(), &turn).await?,
    };

    let db = state.db.lock().await;
    let assistant_msg = DbMessage {
        id: Uuid::new_v4().to_string(),
        session_id: session_id.clone(),
        role: "assistant".to_string(),
        content: assistant_content,
        tool_calls: None,
        tool_call_id: None,
        tokens: None,
        created_at: Utc::now().to_rfc3339(),
    };

    db.insert_message(&assistant_msg)?;
    drop(db);

    spawn_lifecycle_hooks(state.inner(), &session_id, &turn.session.model_id);

    Ok(assistant_msg)
}

#[tauri::command]
pub async fn send_message_stream(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    session_id: String,
    content: String,
    tools_enabled: Option<bool>,
    active_persona_id: Option<String>,
) -> Result<String> {
    // ── Phase: classifying intent ──
    let _ = app_handle.emit("stream_chunk", StreamChunk {
        content: String::new(), done: false, tool_calls: None,
        phase: Some("classifying".to_string()),
    });

    // ── Phase: building context ──
    let _ = app_handle.emit("stream_chunk", StreamChunk {
        content: String::new(), done: false, tool_calls: None,
        phase: Some("building_context".to_string()),
    });

    // One resolver for both entry points: routing, recipe, model and context
    // cannot drift between streaming and non-streaming.
    let turn = prepare_turn(
        state.inner(),
        &session_id,
        &content,
        tools_enabled,
        active_persona_id,
    )
    .await?;

    // Debug view of the assembled request (fast path shape; the deep path
    // consumes the same session/system configuration through the planner).
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
                format!("路由到深度执行: intent={} ({})", turn.route.intent.id, turn.route.intent.name),
            );
            let _ = app_handle.emit("stream_chunk", StreamChunk {
                content: String::new(), done: false, tool_calls: None,
                phase: Some("deep_think".to_string()),
            });
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
        tokens: None,
        created_at: Utc::now().to_rfc3339(),
    };

    db.insert_message(&assistant_msg)?;
    drop(db);

    spawn_lifecycle_hooks(state.inner(), &session_id, &turn.session.model_id);

    Ok(full_content)
}

#[tauri::command]
pub async fn get_messages(state: State<'_, AppState>, session_id: String) -> Result<Vec<DbMessage>> {
    let db = state.db.lock().await;
    db.get_messages(&session_id)
}

#[tauri::command]
pub async fn save_request_context(
    state: State<'_, AppState>,
    session_id: String,
    messages_json: String,
) -> Result<()> {
    let db = state.db.lock().await;
    db.set_setting(&format!("request_ctx:{}", session_id), &messages_json)
}

#[tauri::command]
pub async fn get_request_context(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Option<String>> {
    let db = state.db.lock().await;
    db.get_setting(&format!("request_ctx:{}", session_id))
}

// ── Shared turn preparation ────────────────────────────────────────────────

/// Everything both message entry points need to run one turn.
///
/// Building this once is what keeps routing, the intent recipe, model
/// resolution and prompt assembly identical for streaming and non-streaming
/// callers.
pub(crate) struct PreparedTurn {
    pub session: DbSession,
    pub messages: Vec<DbMessage>,
    pub route: RouteOutcome,
    pub path: PathSelection,
    pub tools_enabled: bool,
    pub allowed_tools: Option<Vec<String>>,
    pub model_id: String,
    pub max_iterations: Option<usize>,
    /// Effective session system prompt (session value or the default).
    pub session_system_prompt: String,
    /// `Your current role: ...` block, when a persona was resolved.
    pub persona_block: Option<String>,
    /// Memory context for this message; injected once per turn.
    pub memory_prompt: Option<String>,
    /// Assembled fast-path messages (system/persona/appendix/memory/history).
    pub fast_messages: Vec<Message>,
    /// Raw user content, needed as the planner goal.
    pub user_content: String,
}

impl PreparedTurn {
    /// Intent appendix rendered as the existing system block, if configured.
    fn intent_appendix(&self) -> Option<String> {
        intent_appendix(&self.route.intent)
    }

    /// Session system prompt + persona + intent appendix.
    ///
    /// The planner API has no separate system-prompt parameter, so this block
    /// is handed over as the leading section of the planner context.
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

    /// Session system prompt + intent appendix; persona is planner-only.
    fn agent_task_system_prompt(&self) -> String {
        let mut parts = vec![self.session_system_prompt.clone()];
        if let Some(appendix) = self.intent_appendix() {
            parts.push(appendix);
        }
        parts.join("\n\n")
    }

    /// Bounded planner context: configuration, memory, and recent messages.
    fn deep_context(&self) -> String {
        build_deep_context(
            Some(&self.planner_system_block()),
            self.memory_prompt.as_deref(),
            &self.messages,
            DEEP_CONTEXT_CHAR_LIMIT,
        )
    }
}

/// Resolve one turn: persist the user message, route it, apply the recipe and
/// assemble the prompts.
pub(crate) async fn prepare_turn(
    state: &AppState,
    session_id: &str,
    content: &str,
    tools_enabled: Option<bool>,
    active_persona_id: Option<String>,
) -> Result<PreparedTurn> {
    let (sess, messages) = {
        let db = state.db.lock().await;

        let sess = db
            .get_session(session_id)?
            .ok_or_else(|| AppError::NotFound("Session not found".to_string()))?;

        let user_msg = DbMessage {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            role: "user".to_string(),
            content: content.to_string(),
            tool_calls: None,
            tool_call_id: None,
            tokens: None,
            created_at: Utc::now().to_rfc3339(),
        };

        db.insert_message(&user_msg)?;
        let messages = db.get_messages(session_id)?;
        (sess, messages)
    };

    // ── Route with the session's persisted domain prior ──
    let pinned = sess.intent_domain_mode == "pinned";
    let route = state
        .intent_router
        .route(RouteRequest {
            content,
            prior_domain: sess.intent_domain_id.as_deref(),
            pinned,
        })
        .await;

    // ── Persist the winning domain as the next message's prior ──
    if let PriorWriteBack::Persist { domain_id } = prior_write_back(
        &sess.intent_domain_mode,
        &route.intent.domain_id,
        &route.intent.domain_name,
    ) {
        let db = state.db.lock().await;
        if let Err(error) = db.set_session_intent_domain(session_id, Some(&domain_id), "auto") {
            log::warn!(
                "failed to persist intent domain prior for session '{}': {}",
                session_id,
                error
            );
        }
    }

    emit_intent_resolved(state, session_id, &route);

    // ── System prompt / persona ──
    let session_system_prompt = sess
        .system_prompt
        .as_ref()
        .filter(|s| !s.is_empty())
        .cloned()
        .unwrap_or_else(|| DEFAULT_SYSTEM_PROMPT.to_string());

    let effective_persona_id = active_persona_id.or_else(|| sess.persona_id.clone());
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

    let persona_block = persona_block(&active_persona);
    let persona_model = persona_model_name(&active_persona);

    // ── Model priority: intent > persona > session, skipping unregistered ──
    let (model_id, skipped_models) = {
        let providers = state.providers.lock().await;
        let default_model = providers.default_model_id().to_string();
        let resolution = resolve_model_id(
            route.intent.config.model_id.as_deref(),
            persona_model.as_deref(),
            &sess.model_id,
            Some(default_model.as_str()).filter(|id| !id.is_empty()),
            |id| providers.is_registered(id),
        );
        (resolution.model_id, resolution.skipped)
    };
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

    // ── Tools: session config ∩ intent whitelist ──
    let session_allowed_tools: Option<Vec<String>> = sess.config.as_ref().and_then(|c| {
        serde_json::from_str::<serde_json::Value>(c)
            .ok()
            .and_then(|v| v.get("enabled_tools").cloned())
            .and_then(|v| serde_json::from_value(v).ok())
    });
    let allowed_tools = state.intent_router.resolve_tools(
        session_allowed_tools,
        route.intent.config.enabled_tools.as_ref(),
    );

    // ── Memory (one call; deep reuses it in the goal prefix) ──
    let memory_prompt = state
        .memory
        .build_context_prompt(content, MEMORY_PROMPT_LIMIT)
        .await
        .ok()
        .flatten();

    // ── Fast-path message assembly (kept as the existing behaviour) ──
    let mut fast_messages: Vec<Message> = Vec::new();
    fast_messages.push(Message::system(session_system_prompt.clone()));
    if let Some(persona) = &persona_block {
        fast_messages.push(Message::system(persona.clone()));
    }
    if let Some(appendix) = intent_appendix(&route.intent) {
        fast_messages.push(Message::system(appendix));
    }
    if let Some(memory) = &memory_prompt {
        fast_messages.push(Message::system(memory.clone()));
    }
    fast_messages.extend(messages.iter().map(|m| match m.role.as_str() {
        "system" => Message::system(m.content.clone()),
        "user" => Message::user(m.content.clone()),
        "assistant" => Message::assistant(m.content.clone()),
        "tool" => tool_result_message(m),
        _ => Message::user(m.content.clone()),
    }));

    let context_window = state
        .config
        .lock()
        .await
        .get_model(&model_id)
        .and_then(|m| m.context_window)
        .map(|v| v as usize);

    if let Some(ctx) = context_window {
        let db = state.db.lock().await;
        fast_messages = crate::features::session::lifecycle::compactor::compress_context(
            &db,
            session_id,
            fast_messages,
            ctx,
        )?;
    }

    Ok(PreparedTurn {
        session: sess,
        messages,
        path: route.intent.path,
        tools_enabled: tools_enabled.unwrap_or(true),
        allowed_tools,
        model_id,
        max_iterations: route.intent.config.max_iterations,
        session_system_prompt,
        persona_block,
        memory_prompt,
        fast_messages,
        route,
        user_content: content.to_string(),
    })
}

// ── Model resolution ───────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelResolution {
    pub model_id: String,
    /// Non-empty candidates that were skipped because they were not registered.
    pub skipped: Vec<String>,
}

/// Resolve the model for one turn.
///
/// Priority is `intent.model_id` > persona `model_name` > `session.model_id`.
/// A candidate the provider registry does not know is skipped (and reported
/// back so the caller can log it) instead of failing the message. The registry
/// default is the last-resort fallback so a stale session model cannot make a
/// turn unsendable.
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

    // Nothing registered: keep the last non-empty candidate so the provider
    // layer can produce a precise error instead of an empty model id.
    let fallback = candidates
        .iter()
        .flatten()
        .map(|c| c.trim())
        .filter(|c| !c.is_empty())
        .last()
        .unwrap_or("")
        .to_string();

    ModelResolution {
        model_id: fallback,
        skipped,
    }
}

// ── Domain prior write-back ────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PriorWriteBack {
    Persist { domain_id: String },
    Skip,
}

/// Decide whether this routing result becomes the session's next prior.
///
/// Only `auto` sessions learn a domain. `pinned` sessions belong to the user,
/// and the synthetic result (empty `domain_name`) means no domain was chosen.
pub(crate) fn prior_write_back(mode: &str, domain_id: &str, domain_name: &str) -> PriorWriteBack {
    let is_auto = mode != "pinned";
    let has_real_domain = !domain_id.trim().is_empty() && !domain_name.trim().is_empty();

    if is_auto && has_real_domain {
        PriorWriteBack::Persist {
            domain_id: domain_id.to_string(),
        }
    } else {
        PriorWriteBack::Skip
    }
}

// ── Deep context ───────────────────────────────────────────────────────────

/// Assemble the planner context: turn configuration, memory, and the last
/// [`DEEP_CONTEXT_MESSAGE_LIMIT`] persisted messages, bounded to `limit` chars.
///
/// The configuration block is preserved by construction: the history budget is
/// whatever remains after the fixed sections, so truncation drops old
/// conversation text rather than the session instructions.
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

/// Keep the last `limit` chars (the newest context) when truncating.
fn truncate_tail(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut chars: Vec<char> = text.chars().rev().take(limit).collect();
    chars.reverse();
    chars.into_iter().collect()
}

// ── Deep failure branch selection ──────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeepFailureAction {
    /// Nothing ran yet: the fast path can answer safely.
    FallbackToFast,
    /// Steps already ran (side effects happened): return what exists.
    ReturnPartial,
}

/// A failure before execution may fall back; a failure during execution must
/// not repeat side effects.
pub(crate) fn deep_failure_action(execution_started: bool) -> DeepFailureAction {
    if execution_started {
        DeepFailureAction::ReturnPartial
    } else {
        DeepFailureAction::FallbackToFast
    }
}

// ── Execution helpers ──────────────────────────────────────────────────────

enum DeepAttempt {
    Completed { text: String },
    Partial { text: String, failed_step: String },
    PlannerFailed { error: String },
}

async fn execute_fast_non_streaming(state: &AppState, turn: &PreparedTurn) -> Result<String> {
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
            // Branch selection is shared with the streaming path so the
            // "planner failed vs step failed" rule cannot drift.
            match deep_failure_action(false) {
                DeepFailureAction::FallbackToFast => execute_fast_non_streaming(state, turn).await,
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

    // ── Phase: thinking (LLM generating first response) ──
    let _ = app_handle.emit("stream_chunk", StreamChunk {
        content: String::new(), done: false, tool_calls: None,
        phase: Some("thinking".to_string()),
    });

    while let Some(chunk) = stream.recv().await {
        match chunk {
            crate::agent::fast_loop::StreamEvent::Content(text) => {
                full_content.push_str(&text);
                let _ = app_handle.emit("stream_chunk", StreamChunk {
                    content: text,
                    done: false,
                    tool_calls: None,
                    phase: None,
                });
            }
            crate::agent::fast_loop::StreamEvent::ToolCall(tool_call) => {
                let _ = app_handle.emit("stream_chunk", StreamChunk {
                    content: String::new(),
                    done: false,
                    tool_calls: Some(vec![ToolCallEvent {
                        id: tool_call.id,
                        name: tool_call.name,
                        status: "calling".to_string(),
                        result: None,
                    }]),
                    phase: Some("executing_tool".to_string()),
                });
            }
            crate::agent::fast_loop::StreamEvent::ToolResult(tool_result) => {
                // Persist tool result message to DB
                {
                    let db = state.db.lock().await;
                    let tool_msg = DbMessage {
                        id: Uuid::new_v4().to_string(),
                        session_id: sess_id_for_messages.clone(),
                        role: "tool".to_string(),
                        content: tool_result.result.clone(),
                        tool_calls: None,
                        tool_call_id: Some(tool_result.call_id.clone()),
                        tokens: None,
                        created_at: Utc::now().to_rfc3339(),
                    };
                    let _ = db.insert_message(&tool_msg);
                } // db lock released here

                let is_error = tool_result.result.starts_with("Tool execution error:");
                let status = if is_error { "failed".to_string() } else { "completed".to_string() };
                let _ = app_handle.emit("stream_chunk", StreamChunk {
                    content: String::new(),
                    done: false,
                    tool_calls: Some(vec![ToolCallEvent {
                        id: tool_result.call_id,
                        name: tool_result.name,
                        status,
                        result: Some(tool_result.result),
                    }]),
                    phase: None,
                });
                // After tool result, LLM will think again — emit "thinking" phase
                let _ = app_handle.emit("stream_chunk", StreamChunk {
                    content: String::new(),
                    done: false,
                    tool_calls: None,
                    phase: Some("thinking".to_string()),
                });
            }
            crate::agent::fast_loop::StreamEvent::Done => {
                let _ = app_handle.emit("stream_chunk", StreamChunk {
                    content: String::new(),
                    done: true,
                    tool_calls: None,
                    phase: Some("completed".to_string()),
                });
                break;
            }
        }
    }

    Ok(full_content)
}

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
            let _ = app_handle.emit("stream_chunk", StreamChunk {
                content: text.clone(),
                done: true,
                tool_calls: None,
                phase: Some("completed".to_string()),
            });
            Ok(text)
        }
        DeepAttempt::Partial { text, failed_step } => {
            log::warn!("deep execution failed at step '{}'; returning partial result", failed_step);
            // The runtime already emitted `plan_failed`; this marks the chat
            // round itself as failed without re-running anything.
            let _ = app_handle.emit("stream_chunk", StreamChunk {
                content: text.clone(),
                done: true,
                tool_calls: None,
                phase: Some("failed".to_string()),
            });
            Ok(text)
        }
    }
}

/// Plan and execute one deep turn with `Interactive` scope.
///
/// Registers an `ExecutionHandle` for the duration of the run so the existing
/// cancel command can stop it, and removes the handle on every path.
async fn run_deep_turn(
    state: &AppState,
    turn: &PreparedTurn,
    app_handle: &tauri::AppHandle,
    forward_progress: bool,
) -> DeepAttempt {
    let planner = LlmPlanner::new(state.providers.clone(), state.tools.clone());
    let context = turn.deep_context();

    let plan = match planner
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
        Ok(plan) => plan,
        Err(error) => {
            match deep_failure_action(false) {
                DeepFailureAction::FallbackToFast => {
                    return DeepAttempt::PlannerFailed {
                        error: error.to_string(),
                    };
                }
                DeepFailureAction::ReturnPartial => unreachable!(),
            }
        }
    };

    let total_steps = plan.steps.len();
    emit_execution_log(
        app_handle,
        "info",
        "planner",
        format!("计划生成成功: {} 步", total_steps),
    );

    // No `plan_generated` event: a chat turn must not open the confirmation
    // dialog. Progress is the only UI signal.
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
    let forwarder = if forward_progress {
        let handle = app_handle.clone();
        let mut rx = event_rx;
        Some(tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                let _ = handle.emit("plan_progress", event);
            }
        }))
    } else {
        drop(event_rx);
        None
    };

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

/// Pick the assistant text for a completed plan.
///
/// Prefer the last step that produced text (the final answer usually lives
/// there); otherwise fall back to a compact step summary.
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

/// Report the completed steps plus the failing step and reason.
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

fn step_summary(result: &serde_json::Value) -> String {
    match result {
        serde_json::Value::String(text) => text.chars().take(200).collect(),
        other => other.to_string().chars().take(200).collect(),
    }
}

// ── Small shared helpers ───────────────────────────────────────────────────

fn intent_appendix(intent: &IntentResult) -> Option<String> {
    intent
        .config
        .system_prompt_appendix
        .as_ref()
        .filter(|appendix| !appendix.trim().is_empty())
        .map(|appendix| format!("[Intent: {} ({})]\n{}", intent.id, intent.name, appendix))
}

fn persona_info(resolution: &PersonaResolution) -> Option<&PersonaInfo> {
    match resolution {
        PersonaResolution::Manual(persona)
        | PersonaResolution::Auto(persona)
        | PersonaResolution::Default(persona) => Some(persona),
        PersonaResolution::None => None,
    }
}

fn persona_block(resolution: &PersonaResolution) -> Option<String> {
    persona_info(resolution).map(|persona| {
        format!(
            "Your current role: {} {}\n{}",
            persona.emoji, persona.title, persona.system_prompt
        )
    })
}

fn persona_model_name(resolution: &PersonaResolution) -> Option<String> {
    persona_info(resolution)
        .map(|persona| persona.model_name.trim().to_string())
        .filter(|model| !model.is_empty())
}

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
struct IntentResolvedEvent<'a> {
    session_id: &'a str,
    domain_id: &'a str,
    domain_name: &'a str,
    intent_id: &'a str,
    intent_name: &'a str,
    confidence: f32,
    source: RouteSource,
    widened: bool,
}

/// Emit the per-turn routing decision for the frontend badge.
fn emit_intent_resolved(state: &AppState, session_id: &str, route: &RouteOutcome) {
    let event = IntentResolvedEvent {
        session_id,
        domain_id: &route.intent.domain_id,
        domain_name: &route.intent.domain_name,
        intent_id: &route.intent.id,
        intent_name: &route.intent.name,
        confidence: route.intent.confidence,
        source: route.source,
        widened: route.widened,
    };
    let _ = state.app_handle.emit("intent_resolved", event);
}

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
    fn prior_write_back_skips_the_synthetic_result() {
        // The synthetic intent reuses the builtin domain id but reports no
        // domain name: no domain row was chosen, so nothing may be written.
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
}
