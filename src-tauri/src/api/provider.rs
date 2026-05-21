use async_trait::async_trait;
use futures::stream::BoxStream;
use std::collections::HashMap;
use std::sync::Arc;

use crate::api::types::{ChatRequest, ChatResponse, StreamPayload};
use crate::config::{AppConfig, ModelConfig, ModelProvider};
use crate::error::{AppError, Result};
use crate::keychain;

use super::openai::OpenAIProvider;
use super::anthropic::AnthropicProvider;

#[async_trait]
pub trait LLMProvider: Send + Sync {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse>;
    async fn chat_stream(&self, request: ChatRequest) -> Result<BoxStream<'static, Result<StreamPayload>>>;
}

pub struct ProviderRegistry {
    openai_compatible: HashMap<String, Arc<OpenAIProvider>>,
    anthropic: HashMap<String, Arc<AnthropicProvider>>,
    default_model_id: Option<String>,
}

impl ProviderRegistry {
    fn requires_api_key(provider: &ModelProvider) -> bool {
        !matches!(provider, ModelProvider::Ollama | ModelProvider::LMStudio)
    }

    pub fn new(config: &AppConfig) -> Self {
        let mut openai_compatible = HashMap::new();
        let mut anthropic = HashMap::new();

        for model in &config.models {
            if !model.enabled {
                continue;
            }

            // Resolve API key: keychain first, config fallback
            let resolved_key = keychain::resolve_api_key(&model.id, &model.api_key);
            let has_key = !resolved_key.is_empty();
            let needs_key = Self::requires_api_key(&model.provider);

            if model.is_compatible_with_openai_api() && (!needs_key || has_key) {
                let mut cfg = model.clone();
                cfg.api_key = resolved_key;
                let provider = OpenAIProvider::new(cfg);
                openai_compatible.insert(model.id.clone(), Arc::new(provider));
            } else if matches!(model.provider, ModelProvider::Anthropic) && has_key {
                let mut cfg = model.clone();
                cfg.api_key = resolved_key;
                let provider = AnthropicProvider::new(cfg);
                anthropic.insert(model.id.clone(), Arc::new(provider));
            }
        }

        let default_id = config.get_default_model().map(|m| m.id.clone());

        Self {
            openai_compatible,
            anthropic,
            default_model_id: default_id,
        }
    }

    pub fn resolve_api_key_in_model(model: &mut ModelConfig) {
        if model.api_key.is_empty() {
            model.api_key = keychain::resolve_api_key(&model.id, "");
        }
    }

    pub fn default_model_id(&self) -> &str {
        self.default_model_id.as_deref().unwrap_or("")
    }

    pub fn get(&self, model_id: &str) -> Result<Arc<dyn LLMProvider>> {
        if let Some(provider) = self.openai_compatible.get(model_id) {
            return Ok(provider.clone());
        }
        if let Some(provider) = self.anthropic.get(model_id) {
            return Ok(provider.clone());
        }
        Err(AppError::Provider(format!("Model '{}' not found or not configured", model_id)))
    }

    pub fn list_models(&self) -> Vec<String> {
        let mut models = Vec::new();
        models.extend(self.openai_compatible.keys().cloned());
        models.extend(self.anthropic.keys().cloned());
        models
    }

    pub fn add_model(&mut self, model: ModelConfig) {
        if !model.enabled {
            return;
        }

        let resolved_key = keychain::resolve_api_key(&model.id, &model.api_key);
        let has_key = !resolved_key.is_empty();
        let needs_key = Self::requires_api_key(&model.provider);

        if model.is_compatible_with_openai_api() && (!needs_key || has_key) {
            let mut cfg = model.clone();
            cfg.api_key = resolved_key;
            let provider = OpenAIProvider::new(cfg);
            self.openai_compatible.insert(model.id.clone(), Arc::new(provider));
        } else if matches!(model.provider, ModelProvider::Anthropic) && has_key {
            let mut cfg = model.clone();
            cfg.api_key = resolved_key;
            let provider = AnthropicProvider::new(cfg);
            self.anthropic.insert(model.id.clone(), Arc::new(provider));
        }
    }

    pub fn remove_model(&mut self, model_id: &str) {
        self.openai_compatible.remove(model_id);
        self.anthropic.remove(model_id);
    }

    pub fn is_registered(&self, model_id: &str) -> bool {
        self.openai_compatible.contains_key(model_id) || self.anthropic.contains_key(model_id)
    }

    pub fn get_registered_model_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.openai_compatible.keys().cloned().collect();
        ids.extend(self.anthropic.keys().cloned());
        ids
    }
}
