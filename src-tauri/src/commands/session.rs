use chrono::Utc;
use tauri::{Emitter, Manager, State};
use uuid::Uuid;

use crate::agent::r#loop::AgentLoop;
use crate::api::types::{Message, MessageRole, ToolCall};
use crate::commands::{StreamChunk, ToolCallEvent};
use crate::db::models::{Session as DbSession, Message as DbMessage};
use crate::error::{AppError, Result};
use crate::execution::planner::LlmPlanner;
use crate::execution::runtime::ExecutionRuntime;
use crate::execution::types::{ExecutionHandle, ExecutionLogEntry, PlanProgressEvent};
use crate::persona::PersonaResolution;
use crate::state::AppState;

/// Default system prompt used when user hasn't configured one.
/// Guides the model toward clear, well-structured, high-quality responses.
const DEFAULT_SYSTEM_PROMPT: &str = "You are a helpful, respectful AI assistant. \
Provide clear, accurate, and well-structured responses. \
Use the same language as the user's input. \
When responding in Chinese, ensure proper grammar and natural phrasing. \
If you don't know something, say so rather than making up information.";

#[tauri::command]
pub async fn create_session(
    state: State<'_, AppState>,
    title: String,
    model_id: String,
    system_prompt: Option<String>,
    persona_id: Option<String>,
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
    };

    let db = state.db.lock().await;
    db.create_session(&sess)?;

    Ok(sess)
}

#[tauri::command]
pub async fn list_sessions(
    state: State<'_, AppState>,
    include_archived: Option<bool>,
) -> Result<Vec<DbSession>> {
    let db = state.db.lock().await;
    let all = db.list_sessions()?;
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
    let db = state.db.lock().await;

    let sess = db
        .get_session(&session_id)?
        .ok_or_else(|| AppError::NotFound("Session not found".to_string()))?;

    let user_msg = DbMessage {
        id: Uuid::new_v4().to_string(),
        session_id: session_id.clone(),
        role: "user".to_string(),
        content: content.clone(),
        tool_calls: None,
        tool_call_id: None,
        tokens: None,
        created_at: Utc::now().to_rfc3339(),
    };

    db.insert_message(&user_msg)?;

    let messages = db.get_messages(&session_id)?;
    drop(db);

    let mut api_messages: Vec<Message> = Vec::new();

    // Use custom system prompt, or fall back to a sensible default
    let system_content = sess.system_prompt.as_ref()
        .filter(|s| !s.is_empty())
        .map(|s| s.as_str())
        .unwrap_or(DEFAULT_SYSTEM_PROMPT);
    api_messages.push(Message {
        id: None,
        role: MessageRole::System,
        content: system_content.to_string(),
        tool_calls: None,
        tool_call_id: None,
    });

    // Resolve persona: explicit active_persona_id > session's persona_id > auto-detect
    let effective_persona_id = active_persona_id.clone()
        .or_else(|| sess.persona_id.clone());
    let project_dir = state.app_handle.path().resource_dir().ok()
        .and_then(|p| p.parent().map(|pp| pp.to_string_lossy().to_string()));
    let active_persona = state.persona.resolve(
        &content,
        project_dir.as_deref(),
        effective_persona_id.as_deref(),
    ).await;

    // If persona specifies a model, override the session model
    let effective_model_id = match &active_persona {
        PersonaResolution::Manual(p) | PersonaResolution::Auto(p) | PersonaResolution::Default(p) => {
            if !p.model_name.is_empty() {
                p.model_name.clone()
            } else {
                sess.model_id.clone()
            }
        }
        _ => sess.model_id.clone(),
    };

    if !matches!(active_persona, PersonaResolution::None) {
        let persona = match &active_persona {
            PersonaResolution::Manual(p) | PersonaResolution::Auto(p) | PersonaResolution::Default(p) => p,
            _ => unreachable!(),
        };
        api_messages.push(Message {
            id: None,
            role: MessageRole::System,
            content: format!(
                "Your current role: {} {}\n{}",
                persona.emoji, persona.title, persona.system_prompt
            ),
            tool_calls: None,
            tool_call_id: None,
        });
    }

    // Inject relevant memories from the memory system
    if let Ok(Some(memory_prompt)) = state.memory.build_context_prompt(&content, 5).await {
        api_messages.push(Message {
            id: None,
            role: MessageRole::System,
            content: memory_prompt,
            tool_calls: None,
            tool_call_id: None,
        });
    }

    api_messages.extend(messages.iter().map(|m| {
        let tool_calls: Option<Vec<ToolCall>> = m.tool_calls.as_ref()
            .and_then(|s| serde_json::from_str(s).ok());
        Message {
            id: Some(m.id.clone()),
            role: match m.role.as_str() {
                "user" => MessageRole::User,
                "assistant" => MessageRole::Assistant,
                "system" => MessageRole::System,
                "tool" => MessageRole::Tool,
                _ => MessageRole::User,
            },
            content: m.content.clone(),
            tool_calls,
            tool_call_id: m.tool_call_id.clone(),
        }
    }));

    let context_window = state.config.lock().await
        .get_model(&effective_model_id)
        .and_then(|m| m.context_window)
        .map(|v| v as usize);

    // Compress context if a context window limit is configured
    if let Some(ctx) = context_window {
        let db = state.db.lock().await;
        api_messages = crate::lifecycle::compactor::compress_context(
            &db, &session_id, api_messages, ctx,
        )?;
    }

    // Parse session config for tool filtering
    let allowed_tools: Option<Vec<String>> = sess.config.as_ref().and_then(|c| {
        serde_json::from_str::<serde_json::Value>(c).ok()
            .and_then(|v| v.get("enabled_tools").cloned())
            .and_then(|v| serde_json::from_value(v).ok())
    });

    let mut agent = AgentLoop::new(state.providers.clone(), state.tools.clone());
    if let Some(ctx) = context_window {
        agent = agent.with_context_limit(ctx);
    }
    let response = agent.run(&effective_model_id, api_messages, tools_enabled.unwrap_or(true), allowed_tools.clone()).await?;

    let db = state.db.lock().await;
    if let Some(choice) = response.choices.first() {
        let assistant_msg = DbMessage {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.clone(),
            role: "assistant".to_string(),
            content: choice.message.content.clone(),
            tool_calls: None,
            tool_call_id: None,
            tokens: response.usage.as_ref().map(|u| u.completion_tokens as i32),
            created_at: Utc::now().to_rfc3339(),
        };

        db.insert_message(&assistant_msg)?;

        // Fire-and-forget lifecycle hooks
        let lifecycle = state.lifecycle.clone();
        let sid = session_id.clone();
        let mid = sess.model_id.clone();
        tokio::spawn(async move {
            let _ = crate::lifecycle::titler::maybe_generate_title(
                &lifecycle, &sid, &mid,
            ).await;
        });

        let lifecycle2 = state.lifecycle.clone();
        let sid2 = session_id.clone();
        let mid2 = sess.model_id.clone();
        tokio::spawn(async move {
            let _ = crate::lifecycle::summarizer::maybe_generate_summary(
                &lifecycle2, &sid2, &mid2,
            ).await;
        });

        Ok(assistant_msg)
    } else {
        Err(AppError::Provider("No response from LLM".to_string()))
    }
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
    let db = state.db.lock().await;

    let sess = db
        .get_session(&session_id)?
        .ok_or_else(|| AppError::NotFound("Session not found".to_string()))?;

    let user_msg = DbMessage {
        id: Uuid::new_v4().to_string(),
        session_id: session_id.clone(),
        role: "user".to_string(),
        content: content.clone(),
        tool_calls: None,
        tool_call_id: None,
        tokens: None,
        created_at: Utc::now().to_rfc3339(),
    };

    db.insert_message(&user_msg)?;

    let messages = db.get_messages(&session_id)?;
    drop(db);

    // ── Phase: classifying intent ──
    let _ = app_handle.emit("stream_chunk", StreamChunk {
        content: String::new(), done: false, tool_calls: None,
        phase: Some("classifying".to_string()),
    });

    // ── LLM Intent Classification ──
    let _intent_result: crate::intent::IntentResult = state.intent_router.classify(&content).await;
    let should_escalate = state.intent_router.should_auto_escalate(&_intent_result.name);
    log::info!(
        "LLM classified: intent={}, should_escalate={}",
        _intent_result.name,
        should_escalate
    );

    // Emit execution log for debugging
    let emit_log = |app_handle: &tauri::AppHandle, level: &str, step: &str, message: String| {
        let entry = ExecutionLogEntry::new(level, step, message);
        let _ = app_handle.emit("execution_log", entry);
    };
    emit_log(&app_handle, "info", "intent", format!("LLM 分类结果: intent={}, should_escalate={}", _intent_result.name, should_escalate));

    // ── Autonomous mode: check if this intent should auto-escalate ──
    if should_escalate {
        emit_log(&app_handle, "info", "planner", "开始生成执行计划...".to_string());
        let _ = app_handle.emit("stream_chunk", StreamChunk {
            content: String::new(), done: false, tool_calls: None,
            phase: Some("planning".to_string()),
        });

        // Generate execution plan
        let planner = LlmPlanner::new(state.providers.clone(), state.tools.clone());
        match planner.generate_plan(&content, &session_id, _intent_result.config.model_id.as_deref()).await {
            Ok(plan) => {
                let step_count = plan.steps.len();
                emit_log(&app_handle, "info", "planner", format!("计划生成成功: {} 步", step_count));
                let plan_id = plan.id.clone();

                // Emit plan to frontend
                let _ = app_handle.emit("plan_generated", &plan);

                // Update session to autonomous mode
                {
                    let db = state.db.lock().await;
                    let status_json = serde_json::to_string(
                        &crate::execution::types::ExecStatus::Running { step_index: 0, started_at: Utc::now().to_rfc3339() }
                    ).unwrap_or_default();
                    let _ = db.update_session_execution(&session_id, "autonomous", &status_json, Some(&plan_id));
                }

                // Spawn background execution
                let handle = ExecutionHandle::new(session_id.clone(), plan_id.clone());
                let cf = handle.cancel_flag.clone();
                let pf = handle.pause_flag.clone();
                let sid = session_id.clone();
                let app_handle2 = app_handle.clone();

                let runtime = ExecutionRuntime::new(state.providers.clone(), state.tools.clone(), state.db.clone()).with_app_handle(app_handle.clone());
                let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<PlanProgressEvent>(32);

                // Forward events
                let app_handle3 = app_handle.clone();
                tokio::spawn(async move {
                    while let Some(event) = event_rx.recv().await {
                        let _ = app_handle3.emit("plan_progress", event);
                    }
                });

                // Execute plan
                let plan_to_execute = plan;
                tokio::spawn(async move {
                    let result = runtime.execute(plan_to_execute, event_tx, cf, pf).await;
                    let _ = app_handle2.emit("stream_chunk", StreamChunk {
                        content: String::new(),
                        done: true,
                        tool_calls: None,
                        phase: Some(match result {
                            Ok(()) => "completed",
                            Err(_) => "failed",
                        }.to_string()),
                    });
                });

                // Store execution handle
                state.active_executions.lock().await.insert(session_id.clone(), handle);

                // Save a placeholder assistant message
                {
                    let db = state.db.lock().await;
                    let summary_msg = DbMessage {
                        id: Uuid::new_v4().to_string(),
                        session_id: sid.clone(),
                        role: "assistant".to_string(),
                        content: format!("开始自主执行计划——共 {} 步。进度将在时间线中显示。", step_count),
                        tool_calls: None,
                        tool_call_id: None,
                        tokens: None,
                        created_at: Utc::now().to_rfc3339(),
                    };
                    let _ = db.insert_message(&summary_msg);
                }

                // Emit stream_chunk so frontend shows the message
                let summary = format!("开始自主执行计划——共 {} 步。进度将在时间线中显示。", step_count);
                let _ = app_handle.emit("stream_chunk", StreamChunk {
                    content: summary.clone(),
                    done: true,
                    tool_calls: None,
                    phase: Some("completed".to_string()),
                });

                emit_log(&app_handle, "info", "execution", "自主执行已在后台启动。可在时间线中查看进度。".to_string());
                return Ok(summary);
            }
            Err(e) => {
                let err_msg = format!("计划生成失败，降级到普通聊天模式: {}", e);
                log::error!("{}", err_msg);
                emit_log(&app_handle, "error", "planner", err_msg);
                // Fall through to normal chat flow
            }
        }
    } else {
        emit_log(&app_handle, "info", "intent", format!("不触发自主模式，走普通 Chat 流程"));
    }

    // ── Phase: building context ──
    let _ = app_handle.emit("stream_chunk", StreamChunk {
        content: String::new(), done: false, tool_calls: None,
        phase: Some("building_context".to_string()),
    });

    let mut api_messages: Vec<Message> = Vec::new();

    // Use custom system prompt, or fall back to a sensible default
    let system_content = sess.system_prompt.as_ref()
        .filter(|s| !s.is_empty())
        .map(|s| s.as_str())
        .unwrap_or(DEFAULT_SYSTEM_PROMPT);
    api_messages.push(Message {
        id: None,
        role: MessageRole::System,
        content: system_content.to_string(),
        tool_calls: None,
        tool_call_id: None,
    });

    // Resolve persona: explicit active_persona_id > session's persona_id > auto-detect
    let effective_persona_id = active_persona_id.clone()
        .or_else(|| sess.persona_id.clone());
    let project_dir = state.app_handle.path().resource_dir().ok()
        .and_then(|p| p.parent().map(|pp| pp.to_string_lossy().to_string()));
    let active_persona = state.persona.resolve(
        &content,
        project_dir.as_deref(),
        effective_persona_id.as_deref(),
    ).await;

    // If persona specifies a model, override the session model
    let effective_model_id = match &active_persona {
        PersonaResolution::Manual(p) | PersonaResolution::Auto(p) | PersonaResolution::Default(p) => {
            if !p.model_name.is_empty() {
                p.model_name.clone()
            } else {
                sess.model_id.clone()
            }
        }
        _ => sess.model_id.clone(),
    };

    if !matches!(active_persona, PersonaResolution::None) {
        let persona = match &active_persona {
            PersonaResolution::Manual(p) | PersonaResolution::Auto(p) | PersonaResolution::Default(p) => p,
            _ => unreachable!(),
        };
        api_messages.push(Message {
            id: None,
            role: MessageRole::System,
            content: format!(
                "Your current role: {} {}\n{}",
                persona.emoji, persona.title, persona.system_prompt
            ),
            tool_calls: None,
            tool_call_id: None,
        });
    }

    // ── Intent Routing: append intent system prompt appendix ──
    if let Some(appendix) = &_intent_result.config.system_prompt_appendix {
        api_messages.push(Message {
            id: None,
            role: MessageRole::System,
            content: format!("[Intent: {}]\n{}", _intent_result.name, appendix),
            tool_calls: None,
            tool_call_id: None,
        });
    }

    // Inject relevant memories from the memory system
    if let Ok(Some(memory_prompt)) = state.memory.build_context_prompt(&content, 5).await {
        api_messages.push(Message {
            id: None,
            role: MessageRole::System,
            content: memory_prompt,
            tool_calls: None,
            tool_call_id: None,
        });
    }

    api_messages.extend(messages.iter().map(|m| {
        let tool_calls: Option<Vec<ToolCall>> = m.tool_calls.as_ref()
            .and_then(|s| serde_json::from_str(s).ok());
        Message {
            id: Some(m.id.clone()),
            role: match m.role.as_str() {
                "user" => MessageRole::User,
                "assistant" => MessageRole::Assistant,
                "system" => MessageRole::System,
                "tool" => MessageRole::Tool,
                _ => MessageRole::User,
            },
            content: m.content.clone(),
            tool_calls,
            tool_call_id: m.tool_call_id.clone(),
        }
    }));

    // Emit debug event + persist to DB for session messages
    let messages_json = serde_json::to_string(&api_messages).unwrap_or_default();
    let _ = app_handle.emit("debug_messages", serde_json::to_value(&api_messages).unwrap_or_default());
    {
        let db = state.db.lock().await;
        let _ = db.set_setting(&format!("request_ctx:{}", session_id), &messages_json);
    }

    let context_window = state.config.lock().await
        .get_model(&effective_model_id)
        .and_then(|m| m.context_window)
        .map(|v| v as usize);

    // Compress context if a context window limit is configured
    if let Some(ctx) = context_window {
        let db = state.db.lock().await;
        api_messages = crate::lifecycle::compactor::compress_context(
            &db, &session_id, api_messages, ctx,
        )?;
    }

    // Parse session config for tool filtering
    let session_allowed_tools: Option<Vec<String>> = sess.config.as_ref().and_then(|c| {
        serde_json::from_str::<serde_json::Value>(c).ok()
            .and_then(|v| v.get("enabled_tools").cloned())
            .and_then(|v| serde_json::from_value(v).ok())
    });

    // ── Intent Routing: resolve final tool list ──
    let allowed_tools = state.intent_router.resolve_tools(
        session_allowed_tools,
        _intent_result.config.enabled_tools.as_ref(),
    );

    let mut agent = AgentLoop::new(state.providers.clone(), state.tools.clone());
    if let Some(ctx) = context_window {
        agent = agent.with_context_limit(ctx);
    }
    // ── Intent Routing: apply max_iterations if configured ──
    if let Some(max_iter) = _intent_result.config.max_iterations {
        agent = agent.with_max_iterations(max_iter);
    }
    let mut stream = agent.run_stream(&effective_model_id, api_messages, tools_enabled.unwrap_or(true), allowed_tools.clone()).await?;

    let mut full_content = String::new();
    let sess_id_for_messages = session_id.clone();

    // ── Phase: thinking (LLM generating first response) ──
    let _ = app_handle.emit("stream_chunk", StreamChunk {
        content: String::new(), done: false, tool_calls: None,
        phase: Some("thinking".to_string()),
    });

    while let Some(chunk) = stream.recv().await {
        match chunk {
            crate::agent::r#loop::StreamEvent::Content(text) => {
                full_content.push_str(&text);
                let _ = app_handle.emit("stream_chunk", StreamChunk {
                    content: text,
                    done: false,
                    tool_calls: None,
                    phase: None,
                });
            }
            crate::agent::r#loop::StreamEvent::ToolCall(tool_call) => {
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
            crate::agent::r#loop::StreamEvent::ToolResult(tool_result) => {
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
            crate::agent::r#loop::StreamEvent::Done => {
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

    // Fire-and-forget lifecycle hooks
    let lifecycle = state.lifecycle.clone();
    let sid = session_id.clone();
    let mid = sess.model_id.clone();
    tokio::spawn(async move {
        let _ = crate::lifecycle::titler::maybe_generate_title(
            &lifecycle, &sid, &mid,
        ).await;
    });

    let lifecycle2 = state.lifecycle.clone();
    let sid2 = session_id.clone();
    let mid2 = sess.model_id.clone();
    tokio::spawn(async move {
        let _ = crate::lifecycle::summarizer::maybe_generate_summary(
            &lifecycle2, &sid2, &mid2,
        ).await;
    });

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
