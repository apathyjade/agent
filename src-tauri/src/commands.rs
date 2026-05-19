use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::{Emitter, State};
use uuid::Uuid;

use crate::agent::r#loop::AgentLoop;
use crate::api::types::{Message, MessageRole};
use crate::config::{ModelConfig, ModelProvider};
use crate::db::models::{Conversation as DbConversation, Message as DbMessage, SystemPrompt};
use crate::error::Result;
use crate::state::AppState;

#[derive(Clone, Serialize, Deserialize)]
pub struct StreamChunk {
    pub content: String,
    pub done: bool,
    pub tool_calls: Option<Vec<ToolCallEvent>>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ToolCallEvent {
    pub id: String,
    pub name: String,
    pub status: String,
    pub result: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub provider: String,
    pub api_key: String,
    pub base_url: Option<String>,
    pub is_default: bool,
    pub enabled: bool,
    pub context_window: Option<u32>,
    pub max_tokens: Option<u32>,
}

impl From<&ModelConfig> for ModelInfo {
    fn from(m: &ModelConfig) -> Self {
        Self {
            id: m.id.clone(),
            name: m.name.clone(),
            display_name: m.display_name.clone(),
            provider: m.provider.to_string(),
            api_key: m.api_key.clone(),
            base_url: m.base_url.clone(),
            is_default: m.is_default,
            enabled: m.enabled,
            context_window: m.context_window,
            max_tokens: m.max_tokens,
        }
    }
}

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
) -> Result<DbMessage> {
    let db = state.db.lock().await;

    let conv = db
        .get_conversation(&conversation_id)?
        .ok_or_else(|| crate::error::AppError::NotFound("Conversation not found".to_string()))?;

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

    if let Some(system_prompt) = &conv.system_prompt {
        if !system_prompt.is_empty() {
            api_messages.push(Message {
                id: None,
                role: MessageRole::System,
                content: system_prompt.clone(),
                tool_calls: None,
                tool_call_id: None,
            });
        }
    }

    api_messages.extend(messages.iter().map(|m| Message {
        id: Some(m.id.clone()),
        role: match m.role.as_str() {
            "user" => MessageRole::User,
            "assistant" => MessageRole::Assistant,
            "system" => MessageRole::System,
            "tool" => MessageRole::Tool,
            _ => MessageRole::User,
        },
        content: m.content.clone(),
        tool_calls: None,
        tool_call_id: m.tool_call_id.clone(),
    }));

    let agent = AgentLoop::new(state.providers.clone(), state.tools.clone());
    let response = agent.run(&conv.model_id, api_messages, true).await?;

    let db = state.db.lock().await;
    if let Some(choice) = response.choices.first() {
        let assistant_msg = DbMessage {
            id: Uuid::new_v4().to_string(),
            conversation_id,
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
        Err(crate::error::AppError::Provider("No response from LLM".to_string()))
    }
}

#[tauri::command]
pub async fn send_message_stream(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    conversation_id: String,
    content: String,
) -> Result<String> {
    let db = state.db.lock().await;

    let conv = db
        .get_conversation(&conversation_id)?
        .ok_or_else(|| crate::error::AppError::NotFound("Conversation not found".to_string()))?;

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

    if let Some(system_prompt) = &conv.system_prompt {
        if !system_prompt.is_empty() {
            api_messages.push(Message {
                id: None,
                role: MessageRole::System,
                content: system_prompt.clone(),
                tool_calls: None,
                tool_call_id: None,
            });
        }
    }

    api_messages.extend(messages.iter().map(|m| Message {
        id: Some(m.id.clone()),
        role: match m.role.as_str() {
            "user" => MessageRole::User,
            "assistant" => MessageRole::Assistant,
            "system" => MessageRole::System,
            "tool" => MessageRole::Tool,
            _ => MessageRole::User,
        },
        content: m.content.clone(),
        tool_calls: None,
        tool_call_id: m.tool_call_id.clone(),
    }));

    let agent = AgentLoop::new(state.providers.clone(), state.tools.clone());
    let mut stream = agent.run_stream(&conv.model_id, api_messages, true).await?;

    let mut full_content = String::new();

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
pub async fn get_models(state: State<'_, AppState>) -> Result<Vec<ModelInfo>> {
    let config = state.config.lock().await;
    Ok(config.models.iter().map(ModelInfo::from).collect())
}

#[tauri::command]
pub async fn add_model(
    state: State<'_, AppState>,
    name: String,
    display_name: String,
    provider: String,
    api_key: String,
    base_url: Option<String>,
    context_window: Option<u32>,
    max_tokens: Option<u32>,
) -> Result<ModelInfo> {
    let provider_enum: ModelProvider = provider.parse().map_err(|e| crate::error::AppError::InvalidInput(e))?;

    let mut config = state.config.lock().await;
    let id = format!("{}-{}", provider, name.replace(['.', '/', ':'], "-"));

    let model = ModelConfig {
        id: id.clone(),
        name,
        display_name,
        provider: provider_enum,
        api_key,
        base_url,
        is_default: config.models.is_empty(),
        enabled: true,
        context_window,
        max_tokens,
    };

    config.add_model(model.clone());
    config.save()?;

    let mut providers = state.providers.lock().await;
    providers.add_model(model.clone());

    Ok(ModelInfo::from(&model))
}

#[tauri::command]
pub async fn remove_model(
    state: State<'_, AppState>,
    id: String,
) -> Result<()> {
    let mut config = state.config.lock().await;
    config.remove_model(&id);
    config.save()?;

    let mut providers = state.providers.lock().await;
    providers.remove_model(&id);

    Ok(())
}

#[tauri::command]
pub async fn update_model(
    state: State<'_, AppState>,
    id: String,
    api_key: String,
    base_url: Option<String>,
    display_name: String,
    enabled: bool,
) -> Result<()> {
    let mut config = state.config.lock().await;

    if let Some(existing) = config.get_model(&id) {
        let updated = ModelConfig {
            id: id.clone(),
            name: existing.name.clone(),
            display_name,
            provider: existing.provider.clone(),
            api_key,
            base_url,
            is_default: existing.is_default,
            enabled,
            context_window: existing.context_window,
            max_tokens: existing.max_tokens,
        };

        config.update_model(&id, updated.clone());
        config.save()?;

        let mut providers = state.providers.lock().await;
        providers.remove_model(&id);
        if enabled {
            providers.add_model(updated);
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn set_default_model(
    state: State<'_, AppState>,
    id: String,
) -> Result<()> {
    let mut config = state.config.lock().await;
    config.set_default_model(&id);
    config.save()?;
    Ok(())
}

#[tauri::command]
pub async fn get_default_model(state: State<'_, AppState>) -> Result<Option<ModelInfo>> {
    let config = state.config.lock().await;
    Ok(config.get_default_model().map(ModelInfo::from))
}

#[tauri::command]
pub async fn update_settings(
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> Result<()> {
    let db = state.db.lock().await;
    db.set_setting(&key, &value)?;
    drop(db);

    let config = state.config.lock().await;
    config.save()
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<HashMap<String, String>> {
    let db = state.db.lock().await;
    let settings = db.get_all_settings()?;
    Ok(settings.into_iter().map(|s| (s.key, s.value)).collect())
}

#[tauri::command]
pub async fn list_tools(state: State<'_, AppState>) -> Result<Vec<serde_json::Value>> {
    let tools = state.tools.lock().await;
    let tool_list = tools.list();
    Ok(tool_list.iter().map(|t| {
        serde_json::json!({
            "name": t.name,
            "description": t.description,
            "parameters": t.parameters,
            "enabled": t.enabled,
        })
    }).collect())
}

#[tauri::command]
pub async fn toggle_tool(
    state: State<'_, AppState>,
    name: String,
    enabled: bool,
) -> Result<()> {
    let mut tools = state.tools.lock().await;
    tools.toggle(&name, enabled)
}

#[tauri::command]
pub async fn create_system_prompt(
    state: State<'_, AppState>,
    name: String,
    content: String,
    is_default: bool,
) -> Result<SystemPrompt> {
    let now = Utc::now().to_rfc3339();
    let prompt = SystemPrompt {
        id: Uuid::new_v4().to_string(),
        name,
        content,
        is_default,
        created_at: now,
    };

    let db = state.db.lock().await;
    db.create_system_prompt(&prompt)?;
    Ok(prompt)
}

#[tauri::command]
pub async fn list_system_prompts(state: State<'_, AppState>) -> Result<Vec<SystemPrompt>> {
    let db = state.db.lock().await;
    db.list_system_prompts()
}

#[tauri::command]
pub async fn delete_system_prompt(
    state: State<'_, AppState>,
    id: String,
) -> Result<()> {
    let db = state.db.lock().await;
    db.delete_system_prompt(&id)
}

#[tauri::command]
pub async fn set_default_system_prompt(
    state: State<'_, AppState>,
    id: String,
) -> Result<()> {
    let db = state.db.lock().await;
    db.set_default_system_prompt(&id)
}

#[tauri::command]
pub async fn get_default_system_prompt(state: State<'_, AppState>) -> Result<Option<SystemPrompt>> {
    let db = state.db.lock().await;
    db.get_default_system_prompt()
}

pub const PROVIDER_OPTIONS: &[(&str, &str, &str, &str)] = &[
    ("openai", "OpenAI", "https://api.openai.com/v1/chat/completions", "gpt-4o"),
    ("anthropic", "Anthropic", "https://api.anthropic.com/v1/messages", "claude-sonnet-4-20250514"),
    ("google", "Google (Gemini)", "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions", "gemini-2.0-flash"),
    ("groq", "Groq", "https://api.groq.com/openai/v1/chat/completions", "llama-3.3-70b-versatile"),
    ("deepseek", "DeepSeek", "https://api.deepseek.com/v1/chat/completions", "deepseek-chat"),
    ("zhipu", "智谱清言", "https://open.bigmodel.cn/api/paas/v4/chat/completions", "glm-4-plus"),
    ("moonshot", "月之暗面", "https://api.moonshot.cn/v1/chat/completions", "moonshot-v1-8k"),
    ("siliconflow", "硅基流动", "https://api.siliconflow.cn/v1/chat/completions", "Qwen/Qwen2.5-72B-Instruct"),
    ("ollama", "Ollama (本地)", "http://localhost:11434/v1/chat/completions", "llama3.2"),
    ("lmstudio", "LM Studio (本地)", "http://localhost:1234/v1/chat/completions", "local-model"),
    ("custom", "自定义", "", "custom-model"),
];


