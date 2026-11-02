//! Rig AI framework — unified LLM provider.
//!
//! Provides the [`RigProvider`] wrapper that adapts any Rig completion model
//! (a [`DynModel<Completion>`]) to the project's [`ProviderBox`] trait, plus
//! the [`create_rig_provider`] factory function.
//!
//! # Provider mapping
//!
//! | Config provider    | Rig client                       | Notes                    |
//! |--------------------|----------------------------------|--------------------------|
//! | OpenAI             | `rig::providers::openai`         | Chat Completions wire    |
//! | Anthropic          | `rig::providers::anthropic`      |                          |
//! | Google             | `rig::providers::gemini`         |                          |
//! | Groq               | `rig::providers::groq`           | OpenAI-shaped dialect    |
//! | DeepSeek           | `rig::providers::deepseek`       | OpenAI-shaped dialect    |
//! | Ollama             | `rig::providers::ollama`         | no API key required      |
//! | Moonshot           | `rig::providers::moonshot`       | OpenAI-shaped dialect    |
//! | Zhipu\*            | OpenAI-compatible                | custom base URL          |
//! | SiliconFlow\*      | OpenAI-compatible                | custom base URL          |
//! | LMStudio\*         | OpenAI-compatible                | custom base URL, no key  |
//! | Custom\*           | OpenAI-compatible                | custom base URL          |
//!
//! \* Providers marked with \* use Rig's OpenAI client with a custom
//!   `base_url`.  These are listed as "OpenAI-compatible" in the code.
//!
//! # Rig 0.45 shape
//!
//! Provider clients no longer hand out agents directly. A client builds a
//! *model* (`client.chat(id)`, `client.completion(id)`, …), which is erased to
//! a [`DynModel<Completion>`] and stored; agents are then built per request
//! with [`rig::agent::AgentBuilder`].

use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::StreamExt;
use serde::{Deserialize, Serialize};

use rig::agent::{AgentBuilder, MultiTurnStreamItem};
use rig::completion::Message;
use rig::driver::DynModel;
use rig::operation::Completion;
use rig::streaming::{Item, StreamEvent};
use rig::tool::DynamicTool;

use crate::api::provider::ProviderBox;
use crate::config::{ModelConfig, ModelProvider};
use crate::error::{AppError, Result};

// ---------------------------------------------------------------------------
// RigProvider — wrapper around a Rig completion model
// ---------------------------------------------------------------------------

/// An LLM provider backed by a Rig [`DynModel<Completion>`].
///
/// The model is type-erased once, when the config is loaded, so providers of
/// different backends (OpenAI, Anthropic, …) share one repr and can be stored
/// in the [`ProviderRegistry`](crate::api::provider::ProviderRegistry).
pub struct RigProvider {
    model: DynModel<Completion>,
    model_id: String,
}

impl RigProvider {
    pub fn new(model: impl Into<DynModel<Completion>>, model_id: String) -> Self {
        Self {
            model: model.into(),
            model_id,
        }
    }

    fn agent(&self, preamble: &str) -> AgentBuilder {
        AgentBuilder::new(self.model.clone()).preamble(preamble)
    }

    fn agent_with_tools(&self, preamble: &str, tools: Vec<DynamicTool>) -> AgentBuilder {
        let builder = self.agent(preamble);
        if tools.is_empty() {
            builder
        } else {
            builder.dynamic_tools(tools)
        }
    }
}

#[async_trait]
impl ProviderBox for RigProvider {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    async fn prompt(&self, preamble: &str, prompt: &str) -> Result<String> {
        let agent = self.agent(preamble).build();

        agent
            .prompt(Message::user(prompt))
            .await
            .map(|response| response.output())
            .map_err(|e| AppError::Provider(format!("Rig prompt error: {}", e)))
    }

    async fn chat(
        &self,
        preamble: &str,
        prompt: &str,
        history: &mut Vec<Message>,
    ) -> Result<String> {
        let agent = self.agent(preamble).build();

        agent
            .chat(Message::user(prompt), history)
            .await
            .map(|response| response.output())
            .map_err(|e| AppError::Provider(format!("Rig chat error: {}", e)))
    }

    async fn prompt_with_tools(
        &self,
        preamble: &str,
        prompt: &str,
        tools: Vec<DynamicTool>,
    ) -> Result<String> {
        let agent = self.agent_with_tools(preamble, tools).build();

        // Rig's agent drives the tool loop internally: every requested call is
        // dispatched through the tool set and answered until the model stops.
        agent
            .prompt(Message::user(prompt))
            .await
            .map(|response| response.output())
            .map_err(|e| AppError::Provider(format!("Rig agent error: {}", e)))
    }

    async fn stream_prompt(
        &self,
        preamble: &str,
        prompt: &str,
    ) -> Result<BoxStream<'static, Result<String>>> {
        let agent = self.agent(preamble).build();
        Ok(box_stream(agent.prompt(Message::user(prompt)).stream()))
    }

    async fn stream_with_tools(
        &self,
        preamble: &str,
        prompt: &str,
        tools: Vec<DynamicTool>,
    ) -> Result<BoxStream<'static, Result<String>>> {
        let agent = self.agent_with_tools(preamble, tools).build();
        Ok(box_stream(agent.prompt(Message::user(prompt)).stream()))
    }
}

/// Map a Rig agent stream to the project's plain-text stream.
///
/// Only answer-text deltas and the final response are surfaced; tool-call,
/// reasoning and lifecycle items render as empty chunks so callers keep the
/// original "text only" contract.
fn box_stream(stream: rig::agent::StreamingResult) -> BoxStream<'static, Result<String>> {
    Box::pin(stream.map(|item| match item {
        Ok(MultiTurnStreamItem::StreamAssistantItem(Item::Event(StreamEvent::Text {
            text, ..
        }))) => Ok(text),
        Ok(MultiTurnStreamItem::FinalResponse(response)) => Ok(response.output()),
        Ok(_) => Ok(String::new()),
        Err(e) => Ok(format!("[Stream Error: {}]", e)),
    }))
}

// ---------------------------------------------------------------------------
// Helper: strip known API path suffixes from a base URL so Rig can append
// its own path (e.g. "/chat/completions") without duplication.
// ---------------------------------------------------------------------------

/// Known API paths that config URLs may end with, which Rig appends itself.
const KNOWN_API_PATHS: &[&str] = &["/chat/completions"];

fn strip_api_suffix(url: &str) -> &str {
    for suffix in KNOWN_API_PATHS {
        if let Some(trimmed) = url.strip_suffix(suffix) {
            return trimmed.trim_end_matches('/');
        }
    }
    url.trim_end_matches('/')
}

// ---------------------------------------------------------------------------
// Factory: create the appropriate RigProvider from a ModelConfig
// ---------------------------------------------------------------------------

/// Create a [`RigProvider`] trait-object for the given model configuration.
///
/// This is the single entry-point used by [`ProviderRegistry`].
///
/// [`ProviderRegistry`]: crate::api::provider::ProviderRegistry
pub fn create_rig_provider(model: &ModelConfig) -> Result<Box<dyn ProviderBox>> {
    let model_id = model.name.clone();

    match model.provider {
        ModelProvider::OpenAI => {
            let client = rig::providers::openai::OpenAI::new(model.api_key.clone());
            Ok(Box::new(RigProvider::new(
                client.chat(model_id.clone()),
                model.name.clone(),
            )))
        }

        ModelProvider::Anthropic => {
            let client = rig::providers::anthropic::Anthropic::new(model.api_key.clone());
            Ok(Box::new(RigProvider::new(
                client.completion(model_id.clone()),
                model.name.clone(),
            )))
        }

        ModelProvider::Google => {
            let client = rig::providers::gemini::Gemini::new(model.api_key.clone());
            Ok(Box::new(RigProvider::new(
                client.completion(model_id.clone()),
                model.name.clone(),
            )))
        }

        ModelProvider::Groq => {
            let client = rig::providers::groq::new(model.api_key.clone());
            Ok(Box::new(RigProvider::new(
                client.chat(model_id.clone()),
                model.name.clone(),
            )))
        }

        ModelProvider::DeepSeek => {
            let client = rig::providers::deepseek::new(model.api_key.clone());
            Ok(Box::new(RigProvider::new(
                client.chat(model_id.clone()),
                model.name.clone(),
            )))
        }

        ModelProvider::Ollama => {
            // Rig's Ollama client points at the daemon root and appends the
            // API path itself, so a configured OpenAI-style URL is reduced to
            // its origin first.
            let client = match model.base_url.as_deref() {
                Some(url) => {
                    let root = strip_api_suffix(url);
                    let root = root.strip_suffix("/v1").unwrap_or(root);
                    rig::providers::ollama::OllamaConfig::new()
                        .with_base_url(root)
                        .client()
                }
                None => rig::providers::ollama::Ollama::new(),
            };
            Ok(Box::new(RigProvider::new(
                client.completion(model_id.clone()),
                model.name.clone(),
            )))
        }

        ModelProvider::Moonshot => {
            let client = rig::providers::moonshot::new(model.api_key.clone());
            Ok(Box::new(RigProvider::new(
                client.chat(model_id.clone()),
                model.name.clone(),
            )))
        }

        // OpenAI-compatible providers: Zhipu, SiliconFlow, LMStudio, Custom
        //
        // These use Rig's OpenAI **Chat Completions** client (endpoint
        // `/chat/completions`) instead of the newer Responses API, because
        // that is what these third-party providers implement.
        //
        // When the user has set a custom `base_url` in their config, it often
        // includes the full path (e.g. ".../v1/chat/completions").  Since Rig
        // appends the path itself, we strip the known suffix first.
        ModelProvider::Zhipu
        | ModelProvider::SiliconFlow
        | ModelProvider::LMStudio
        | ModelProvider::Custom => {
            let effective_base_url = model.effective_base_url();
            let has_custom_base = model.base_url.is_some();
            let api_base = strip_api_suffix(&effective_base_url);

            let mut config = rig::providers::openai::OpenAIConfig::new(model.api_key.clone());
            if !api_base.is_empty() {
                config = config.with_base_url(api_base);
            }

            if has_custom_base {
                log::info!(
                    "Using OpenAI-compatible Rig client for '{}' with base_url: {}",
                    model.id, api_base
                );
            }

            let client = config.client();
            Ok(Box::new(RigProvider::new(
                client.chat(model_id),
                model.name.clone(),
            )))
        }
    }
}

// ---------------------------------------------------------------------------
// Structured extraction
// ---------------------------------------------------------------------------

/// Extract structured data from text using Rig's [`Extractor`].
///
/// The generic parameter `T` is the target struct.  It must derive
/// [`Serialize`], [`Deserialize`], and [`schemars::JsonSchema`].
///
/// This is used by the intent classifier and execution planner to
/// replace brittle manual JSON parsing.
///
/// [`Extractor`]: rig::extractor::Extractor
pub async fn extract_structured<T>(
    api_key: &str,
    model: &str,
    system_prompt: &str,
    user_input: &str,
) -> Result<T>
where
    T: Serialize + for<'de> Deserialize<'de> + schemars::JsonSchema + Send + Sync + 'static,
{
    let client = rig::providers::openai::OpenAI::new(api_key);

    // Combine system prompt and user input, since the extractor
    // doesn't have a separate preamble method.
    let combined = format!("{}\n\n{}", system_prompt, user_input);

    let extractor = rig::extractor::ExtractorBuilder::<T>::new(client.chat(model)).build();

    let response = extractor
        .extract(combined)
        .await
        .map_err(|e| AppError::Provider(format!("Extraction failed: {}", e)))?;

    Ok(response.output)
}
