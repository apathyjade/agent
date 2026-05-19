use async_trait::async_trait;
use futures::stream::BoxStream;
use std::collections::HashMap;
use std::sync::Arc;

use crate::api::types::{ChatRequest, ChatResponse, StreamPayload};
use crate::config::{AppConfig, ModelConfig, ModelProvider};
use crate::error::{AppError, Result};

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

            let has_key = !model.api_key.is_empty();
            let needs_key = Self::requires_api_key(&model.provider);

            if model.is_compatible_with_openai_api() && (!needs_key || has_key) {
                let provider = OpenAIProvider::new(model.clone());
                openai_compatible.insert(model.id.clone(), Arc::new(provider));
            } else if matches!(model.provider, ModelProvider::Anthropic) && has_key {
                let provider = AnthropicProvider::new(model.clone());
                anthropic.insert(model.id.clone(), Arc::new(provider));
            }
        }

        Self {
            openai_compatible,
            anthropic,
        }
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

        let has_key = !model.api_key.is_empty();
        let needs_key = Self::requires_api_key(&model.provider);

        if model.is_compatible_with_openai_api() && (!needs_key || has_key) {
            let provider = OpenAIProvider::new(model.clone());
            self.openai_compatible.insert(model.id.clone(), Arc::new(provider));
        } else if matches!(model.provider, ModelProvider::Anthropic) && has_key {
            let provider = AnthropicProvider::new(model.clone());
            self.anthropic.insert(model.id.clone(), Arc::new(provider));
        }
    }

    pub fn remove_model(&mut self, model_id: &str) {
        self.openai_compatible.remove(model_id);
        self.anthropic.remove(model_id);
    }
}
