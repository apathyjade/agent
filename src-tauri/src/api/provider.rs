use async_trait::async_trait;
use futures::stream::BoxStream;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::api::types::{ChatRequest, ChatResponse, Message, MessageRole, StreamPayload};
use crate::config::AppConfig;
use crate::error::{AppError, Result};

use super::rig::create_rig_provider;

#[async_trait]
pub trait LLMProvider: Send + Sync {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse>;
    async fn chat_stream(&self, request: ChatRequest) -> Result<BoxStream<'static, Result<StreamPayload>>>;
}

pub struct ProviderRegistry {
    providers: HashMap<String, Arc<dyn LLMProvider>>,
    default_model_id: Option<String>,
}

impl ProviderRegistry {
    pub fn new(config: &AppConfig) -> Self {
        let mut providers = HashMap::new();

        for model in &config.models {
            if !model.enabled {
                continue;
            }

            match create_rig_provider(model) {
                Ok(provider) => {
                    providers.insert(model.id.clone(), Arc::from(provider));
                }
                Err(e) => {
                    log::warn!("Failed to create Rig provider for '{}' ({}): {}",
                        model.id, model.provider, e);
                    continue;
                }
            }
        }

        let default_id = config.get_default_model().map(|m| m.id.clone());

        Self { providers, default_model_id: default_id }
    }

    pub fn default_model_id(&self) -> &str {
        self.default_model_id.as_deref().unwrap_or("")
    }

    pub fn get(&self, model_id: &str) -> Result<Arc<dyn LLMProvider>> {
        self.providers.get(model_id)
            .cloned()
            .ok_or_else(|| AppError::Provider(format!("Model '{}' not found or not configured", model_id)))
    }

    /// Resolve a provider by model_id, falling back to the default if `None`.
    pub fn resolve(&self, model_id: Option<&str>) -> Result<Arc<dyn LLMProvider>> {
        let mid = model_id.unwrap_or_else(|| self.default_model_id());
        if mid.is_empty() {
            return Err(AppError::Provider("No model configured".into()));
        }
        self.get(mid)
    }

    pub fn list_models(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }

    pub fn add_model(&mut self, model: crate::config::ModelConfig) {
        if !model.enabled {
            return;
        }

        match create_rig_provider(&model) {
            Ok(provider) => {
                self.providers.insert(model.id.clone(), Arc::from(provider));
            }
            Err(e) => {
                log::warn!("Failed to create Rig provider for '{}': {}", model.id, e);
            }
        }
    }

    pub fn remove_model(&mut self, model_id: &str) {
        self.providers.remove(model_id);
    }

    pub fn is_registered(&self, model_id: &str) -> bool {
        self.providers.contains_key(model_id)
    }

    pub fn get_registered_model_ids(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }
}

// ---------------------------------------------------------------------------
// Convenience: simple system + user chat that returns text content.
// ---------------------------------------------------------------------------

/// Perform a simple system-prompt + user-message chat and return the text content.
///
/// This is a convenience wrapper around the common pattern of:
/// 1. Resolving the model from the registry (locked briefly, then released)
/// 2. Building a `ChatRequest` with system + user messages
/// 3. Extracting the response text
///
/// When `model_id` is `None`, the registry's default model is used.
pub async fn chat_text(
    providers: &Arc<Mutex<ProviderRegistry>>,
    model_id: Option<&str>,
    system_prompt: &str,
    user_message: &str,
    max_tokens: Option<usize>,
    temperature: Option<f32>,
) -> Result<String> {
    let provider = {
        let registry = providers.lock().await;
        registry.resolve(model_id)?
    };

    let request = ChatRequest {
        messages: vec![
            Message {
                id: None,
                role: MessageRole::System,
                content: system_prompt.to_string(),
                tool_calls: None,
                tool_call_id: None,
            },
            Message {
                id: None,
                role: MessageRole::User,
                content: user_message.to_string(),
                tool_calls: None,
                tool_call_id: None,
            },
        ],
        model: String::new(),
        tools: None,
        stream: Some(false),
        max_tokens,
        temperature,
    };

    let response = provider.chat(request).await?;
    Ok(response
        .choices
        .first()
        .map(|c| c.message.content.clone())
        .unwrap_or_default())
}
