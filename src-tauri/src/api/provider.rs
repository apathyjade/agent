use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;
use rig::completion::Message;
use rig::tool::ToolSet;
use crate::config::AppConfig;
use crate::error::{AppError, Result};

use super::rig::create_rig_provider;

// ---------------------------------------------------------------------------
// Object-safe trait — wraps any Rig CompletionClient behind a single
// interface so we can store heterogeneous providers in the registry.
// ---------------------------------------------------------------------------

/// Object-safe provider interface for type-erased access to Rig clients.
///
/// Every concrete [`RigProvider<C>`](super::rig::RigProvider) implements this
/// trait, allowing [`ProviderRegistry`] to store providers built from
/// different backends (OpenAI, Anthropic, …) in a single map.
#[async_trait]
pub trait ProviderBox: Send + Sync {
    /// The model identifier (e.g. `"gpt-4o"`, `"claude-3-opus"`).
    fn model_id(&self) -> &str;

    /// One-shot prompt — system preamble + user text → assistant text.
    async fn prompt(&self, preamble: &str, prompt: &str) -> Result<String>;

    /// Multi-turn chat with conversation history.
    async fn chat(
        &self,
        preamble: &str,
        prompt: &str,
        history: &mut Vec<Message>,
    ) -> Result<String>;

    /// One-shot prompt with tool support.
    /// Rig's Agent handles multi-turn tool execution internally.
    async fn prompt_with_tools(
        &self,
        preamble: &str,
        prompt: &str,
        tool_set: ToolSet,
    ) -> Result<String>;

    /// Streamed one-shot prompt (no tools).
    async fn stream_prompt(
        &self,
        preamble: &str,
        prompt: &str,
    ) -> Result<BoxStream<'static, Result<String>>>;

    /// Streamed prompt with tool support.
    async fn stream_with_tools(
        &self,
        preamble: &str,
        prompt: &str,
        tool_set: ToolSet,
    ) -> Result<BoxStream<'static, Result<String>>>;
}

// ---------------------------------------------------------------------------
// ProviderRegistry
// ---------------------------------------------------------------------------

pub struct ProviderRegistry {
    providers: HashMap<String, Arc<dyn ProviderBox>>,
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
                    log::warn!(
                        "Failed to create Rig provider for '{}' ({}): {}",
                        model.id, model.provider, e
                    );
                    continue;
                }
            }
        }

        let default_id = config.get_default_model().map(|m| m.id.clone());

        Self {
            providers,
            default_model_id: default_id,
        }
    }

    pub fn default_model_id(&self) -> &str {
        self.default_model_id.as_deref().unwrap_or("")
    }

    pub fn get(&self, model_id: &str) -> Result<Arc<dyn ProviderBox>> {
        self.providers
            .get(model_id)
            .cloned()
            .ok_or_else(|| {
                AppError::Provider(format!(
                    "Model '{}' not found or not configured",
                    model_id
                ))
            })
    }

    /// Resolve a provider by model_id, falling back to the default if `None`.
    pub fn resolve(&self, model_id: Option<&str>) -> Result<Arc<dyn ProviderBox>> {
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
