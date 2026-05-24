use chrono::Utc;
use tauri::{Emitter, Manager, State};
use uuid::Uuid;

use crate::agent::r#loop::AgentLoop;
use crate::api::types::{Message, MessageRole, ToolCall};
use crate::commands::{StreamChunk, ToolCallEvent};
use crate::db::models::{Conversation as DbConversation, Message as DbMessage};
use crate::error::{AppError, Result};
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
pub async fn create_conversation(
    state: State<'_, AppState>,
    title: String,
    model_id: String,
    system_prompt: Option<String>,
) -> Result<DbConversation> {
    let now = Utc::now().to_rfc3339();
    let conv = DbConversation {
        id: Uuid::new_v4().to_string(),
        title,
        model_id,
        system_prompt,
        created_at: now.clone(),
        updated_at: now,
    };

    let db = state.db.lock().await;
    db.create_conversation(&conv)?;

    Ok(conv)
}

#[tauri::command]
pub async fn list_conversations(state: State<'_, AppState>) -> Result<Vec<DbConversation>> {
    let db = state.db.lock().await;
    db.list_conversations()
}

#[tauri::command]
pub async fn get_conversation(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<DbConversation>> {
    let db = state.db.lock().await;
    db.get_conversation(&id)
}

#[tauri::command]
pub async fn delete_conversation(state: State<'_, AppState>, id: String) -> Result<()> {
    let db = state.db.lock().await;
    db.delete_conversation(&id)
}

#[tauri::command]
pub async fn update_conversation_title(
    state: State<'_, AppState>,
    id: String,
    title: String,
) -> Result<()> {
    let db = state.db.lock().await;
    db.update_conversation_title(&id, &title)
}

#[tauri::command]
pub async fn update_conversation_model(
    state: State<'_, AppState>,
    id: String,
    model_id: String,
) -> Result<()> {
    let db = state.db.lock().await;
    db.update_conversation_model(&id, &model_id)
}

#[tauri::command]
pub async fn update_conversation_system_prompt(
    state: State<'_, AppState>,
    id: String,
    system_prompt: String,
) -> Result<()> {
    let db = state.db.lock().await;
    db.update_conversation_system_prompt(&id, &system_prompt)
}

#[tauri::command]
pub async fn clear_conversation(
    state: State<'_, AppState>,
    conversation_id: String,
) -> Result<()> {
    let db = state.db.lock().await;
    db.clear_messages(&conversation_id)
}

#[tauri::command]
pub async fn send_message(
    state: State<'_, AppState>,
    conversation_id: String,
    content: String,
    tools_enabled: Option<bool>,
    active_persona_id: Option<String>,
) -> Result<DbMessage> {
    let db = state.db.lock().await;

    let conv = db
        .get_conversation(&conversation_id)?
        .ok_or_else(|| AppError::NotFound("Conversation not found".to_string()))?;

    let user_msg = DbMessage {
        id: Uuid::new_v4().to_string(),
        conversation_id: conversation_id.clone(),
        role: "user".to_string(),
        content: content.clone(),
        tool_calls: None,
        tool_call_id: None,
        tokens: None,
        created_at: Utc::now().to_rfc3339(),
    };

    db.insert_message(&user_msg)?;

    let messages = db.get_messages(&conversation_id)?;
    drop(db);

    let mut api_messages: Vec<Message> = Vec::new();

    // Use custom system prompt, or fall back to a sensible default
    let system_content = conv.system_prompt.as_ref()
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

    // Resolve and inject persona context
    let project_dir = state.app_handle.path().resource_dir().ok()
        .and_then(|p| p.parent().map(|pp| pp.to_string_lossy().to_string()));
    let active_persona = state.persona.resolve(
        &content,
        project_dir.as_deref(),
        active_persona_id.as_deref(),
    ).await;
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
        .get_model(&conv.model_id)
        .and_then(|m| m.context_window)
        .map(|v| v as usize);

    let mut agent = AgentLoop::new(state.providers.clone(), state.tools.clone());
    if let Some(ctx) = context_window {
        agent = agent.with_context_limit(ctx);
    }
    let response = agent.run(&conv.model_id, api_messages, tools_enabled.unwrap_or(true)).await?;

    let db = state.db.lock().await;
    if let Some(choice) = response.choices.first() {
        let assistant_msg = DbMessage {
            id: Uuid::new_v4().to_string(),
            conversation_id: conversation_id.clone(),
            role: "assistant".to_string(),
            content: choice.message.content.clone(),
            tool_calls: None,
            tool_call_id: None,
            tokens: response.usage.as_ref().map(|u| u.completion_tokens as i32),
            created_at: Utc::now().to_rfc3339(),
        };

        db.insert_message(&assistant_msg)?;
        Ok(assistant_msg)
    } else {
        Err(AppError::Provider("No response from LLM".to_string()))
    }
}

#[tauri::command]
pub async fn send_message_stream(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    conversation_id: String,
    content: String,
    tools_enabled: Option<bool>,
    active_persona_id: Option<String>,
) -> Result<String> {
    let db = state.db.lock().await;

    let conv = db
        .get_conversation(&conversation_id)?
        .ok_or_else(|| AppError::NotFound("Conversation not found".to_string()))?;

    let user_msg = DbMessage {
        id: Uuid::new_v4().to_string(),
        conversation_id: conversation_id.clone(),
        role: "user".to_string(),
        content: content.clone(),
        tool_calls: None,
        tool_call_id: None,
        tokens: None,
        created_at: Utc::now().to_rfc3339(),
    };

    db.insert_message(&user_msg)?;

    let messages = db.get_messages(&conversation_id)?;
    drop(db);

    let mut api_messages: Vec<Message> = Vec::new();

    // Use custom system prompt, or fall back to a sensible default
    let system_content = conv.system_prompt.as_ref()
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

    // Resolve and inject persona context
    let project_dir = state.app_handle.path().resource_dir().ok()
        .and_then(|p| p.parent().map(|pp| pp.to_string_lossy().to_string()));
    let active_persona = state.persona.resolve(
        &content,
        project_dir.as_deref(),
        active_persona_id.as_deref(),
    ).await;
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

    // Emit debug event + persist to DB for session messages
    let messages_json = serde_json::to_string(&api_messages).unwrap_or_default();
    let _ = app_handle.emit("debug_messages", serde_json::to_value(&api_messages).unwrap_or_default());
    {
        let db = state.db.lock().await;
        let _ = db.set_setting(&format!("request_ctx:{}", conversation_id), &messages_json);
    }

    let context_window = state.config.lock().await
        .get_model(&conv.model_id)
        .and_then(|m| m.context_window)
        .map(|v| v as usize);

    let mut agent = AgentLoop::new(state.providers.clone(), state.tools.clone());
    if let Some(ctx) = context_window {
        agent = agent.with_context_limit(ctx);
    }
    let mut stream = agent.run_stream(&conv.model_id, api_messages, tools_enabled.unwrap_or(true)).await?;

    let mut full_content = String::new();
    let conv_id_for_messages = conversation_id.clone();

    while let Some(chunk) = stream.recv().await {
        match chunk {
            crate::agent::r#loop::StreamEvent::Content(text) => {
                full_content.push_str(&text);
                let _ = app_handle.emit("stream_chunk", StreamChunk {
                    content: text,
                    done: false,
                    tool_calls: None,
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
                });
            }
            crate::agent::r#loop::StreamEvent::ToolResult(tool_result) => {
                // Persist tool result message to DB
                {
                    let db = state.db.lock().await;
                    let tool_msg = DbMessage {
                        id: Uuid::new_v4().to_string(),
                        conversation_id: conv_id_for_messages.clone(),
                        role: "tool".to_string(),
                        content: tool_result.result.clone(),
                        tool_calls: None,
                        tool_call_id: Some(tool_result.call_id.clone()),
                        tokens: None,
                        created_at: Utc::now().to_rfc3339(),
                    };
                    let _ = db.insert_message(&tool_msg);
                } // db lock released here

                let _ = app_handle.emit("stream_chunk", StreamChunk {
                    content: String::new(),
                    done: false,
                    tool_calls: Some(vec![ToolCallEvent {
                        id: tool_result.call_id,
                        name: tool_result.name,
                        status: "completed".to_string(),
                        result: Some(tool_result.result),
                    }]),
                });
            }
            crate::agent::r#loop::StreamEvent::Done => {
                let _ = app_handle.emit("stream_chunk", StreamChunk {
                    content: String::new(),
                    done: true,
                    tool_calls: None,
                });
                break;
            }
        }
    }

    let db = state.db.lock().await;
    let assistant_msg = DbMessage {
        id: Uuid::new_v4().to_string(),
        conversation_id,
        role: "assistant".to_string(),
        content: full_content.clone(),
        tool_calls: None,
        tool_call_id: None,
        tokens: None,
        created_at: Utc::now().to_rfc3339(),
    };

    db.insert_message(&assistant_msg)?;
    Ok(full_content)
}

#[tauri::command]
pub async fn get_messages(state: State<'_, AppState>, conversation_id: String) -> Result<Vec<DbMessage>> {
    let db = state.db.lock().await;
    db.get_messages(&conversation_id)
}

#[tauri::command]
pub async fn save_request_context(
    state: State<'_, AppState>,
    conversation_id: String,
    messages_json: String,
) -> Result<()> {
    let db = state.db.lock().await;
    db.set_setting(&format!("request_ctx:{}", conversation_id), &messages_json)
}

#[tauri::command]
pub async fn get_request_context(
    state: State<'_, AppState>,
    conversation_id: String,
) -> Result<Option<String>> {
    let db = state.db.lock().await;
    db.get_setting(&format!("request_ctx:{}", conversation_id))
}
