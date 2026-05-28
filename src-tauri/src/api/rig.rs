//! Rig AI framework — unified LLM provider.
//!
//! Provides the [`RigProvider`] wrapper that adapts any Rig [`CompletionClient`]
//! to the project's [`ProviderBox`] trait, plus the [`create_rig_provider`]
//! factory function.
//!
//! # Provider mapping
//!
//! | Config provider    | Rig client                     | Notes                    |
//! |--------------------|--------------------------------|--------------------------|
//! | OpenAI             | `rig::providers::openai`       |                          |
//! | Anthropic          | `rig::providers::anthropic`    |                          |
//! | Google             | `rig::providers::gemini`       |                          |
//! | Groq               | `rig::providers::groq`         |                          |
//! | DeepSeek           | `rig::providers::deepseek`     |                          |
//! | Ollama             | `rig::providers::ollama`       | no API key required      |
//! | Moonshot           | `rig::providers::moonshot`     |                          |
//! | Zhipu\*            | OpenAI-compatible              | custom base URL          |
//! | SiliconFlow\*      | OpenAI-compatible              | custom base URL          |
//! | LMStudio\*         | OpenAI-compatible              | custom base URL, no key  |
//! | Custom\*           | OpenAI-compatible              | custom base URL          |
//!
//! \* Providers marked with \* use Rig's OpenAI client with a custom
//!   `base_url`.  These are listed as "OpenAI-compatible" in the code.

use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::StreamExt;
use serde::{Deserialize, Serialize};

use rig::agent::MultiTurnStreamItem;
use rig::client::CompletionClient;
use rig::completion::{Chat, Message, Prompt};
use rig::streaming::{StreamedAssistantContent, StreamingChat};
use rig::tool::ToolSet;

use crate::config::{ModelConfig, ModelProvider};
use crate::error::{AppError, Result};
use crate::api::provider::ProviderBox;

// ---------------------------------------------------------------------------
// RigProvider — generic wrapper around a Rig CompletionClient
// ---------------------------------------------------------------------------

/// A generic LLM provider backed by a Rig [`CompletionClient`].
///
/// `C` is typically one of:
/// - `rig::providers::openai::Client`
/// - `rig::providers::anthropic::Client`
/// - `rig::providers::gemini::Client`
/// - `rig::providers::groq::Client`
/// - `rig::providers::deepseek::Client`
/// - `rig::providers::ollama::Client`
/// - `rig::providers::moonshot::Client`
pub struct RigProvider<C: CompletionClient> {
    client: C,
    model: String,
}

impl<C: CompletionClient + Send + Sync + 'static> RigProvider<C> {
    pub fn new(client: C, model: String) -> Self {
        Self { client, model }
    }
}

#[async_trait]
impl<C: CompletionClient + Send + Sync + 'static> ProviderBox for RigProvider<C> {
    fn model_id(&self) -> &str {
        &self.model
    }

    async fn prompt(&self, preamble: &str, prompt: &str) -> Result<String> {
        let agent = self
            .client
            .agent(&self.model)
            .preamble(preamble)
            .build();

        agent
            .prompt(prompt)
            .await
            .map_err(|e| AppError::Provider(format!("Rig prompt error: {}", e)))
    }

    async fn chat(
        &self,
        preamble: &str,
        prompt: &str,
        history: &mut Vec<Message>,
    ) -> Result<String> {
        let agent = self
            .client
            .agent(&self.model)
            .preamble(preamble)
            .build();

        agent
            .chat(prompt, history)
            .await
            .map_err(|e| AppError::Provider(format!("Rig chat error: {}", e)))
    }

    async fn prompt_with_tools(
        &self,
        preamble: &str,
        prompt: &str,
        tool_set: ToolSet,
    ) -> Result<String> {
        let agent = self
            .client
            .agent(&self.model)
            .preamble(preamble)
            .build();

        // Add tools after building the agent via ToolServerHandle
        agent
            .tool_server_handle
            .append_toolset(tool_set)
            .await
            .map_err(|e| AppError::Provider(format!("Rig tool init error: {}", e)))?;

        agent
            .prompt(prompt)
            .await
            .map_err(|e| AppError::Provider(format!("Rig agent error: {}", e)))
    }

    async fn stream_prompt(
        &self,
        preamble: &str,
        prompt: &str,
    ) -> Result<BoxStream<'static, Result<String>>> {
        let agent = self
            .client
            .agent(&self.model)
            .preamble(preamble)
            .build();

        let chat_history: Vec<Message> = Vec::new();
        let stream = agent.stream_chat(prompt, chat_history).await;

        Ok(Box::pin(stream.map(|item| match item {
            Ok(MultiTurnStreamItem::StreamAssistantItem(content)) => match content {
                StreamedAssistantContent::Text(text) => Ok(text.text),
                _ => Ok(String::new()),
            },
            Ok(MultiTurnStreamItem::FinalResponse(response)) => {
                Ok(response.response().to_string())
            }
            Err(e) => Ok(format!("[Stream Error: {}]", e)),
            _ => Ok(String::new()),
        })))
    }

    async fn stream_with_tools(
        &self,
        preamble: &str,
        prompt: &str,
        tool_set: ToolSet,
    ) -> Result<BoxStream<'static, Result<String>>> {
        let agent = self
            .client
            .agent(&self.model)
            .preamble(preamble)
            .build();

        agent
            .tool_server_handle
            .append_toolset(tool_set)
            .await
            .map_err(|e| AppError::Provider(format!("Rig tool init error: {}", e)))?;

        let chat_history: Vec<Message> = Vec::new();
        let stream = agent.stream_chat(prompt, chat_history).await;

        Ok(Box::pin(stream.map(|item| match item {
            Ok(MultiTurnStreamItem::StreamAssistantItem(content)) => match content {
                StreamedAssistantContent::Text(text) => Ok(text.text),
                _ => Ok(String::new()),
            },
            Ok(MultiTurnStreamItem::FinalResponse(response)) => {
                Ok(response.response().to_string())
            }
            Err(e) => Ok(format!("[Stream Error: {}]", e)),
            _ => Ok(String::new()),
        })))
    }
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
pub fn create_rig_provider(model: &ModelConfig) -> Result<Box<dyn ProviderBox>> {
    match model.provider {
        ModelProvider::OpenAI => {
            let client = rig::providers::openai::Client::new(&model.api_key)
                .map_err(|e| AppError::Provider(format!("OpenAI init: {}", e)))?;
            Ok(Box::new(RigProvider::new(client, model.name.clone())))
        }

        ModelProvider::Anthropic => {
            let client = rig::providers::anthropic::Client::new(&model.api_key)
                .map_err(|e| AppError::Provider(format!("Anthropic init: {}", e)))?;
            Ok(Box::new(RigProvider::new(client, model.name.clone())))
        }

        ModelProvider::Google => {
            let client = rig::providers::gemini::Client::new(&model.api_key)
                .map_err(|e| AppError::Provider(format!("Gemini init: {}", e)))?;
            Ok(Box::new(RigProvider::new(client, model.name.clone())))
        }

        ModelProvider::Groq => {
            let client = rig::providers::groq::Client::new(&model.api_key)
                .map_err(|e| AppError::Provider(format!("Groq init: {}", e)))?;
            Ok(Box::new(RigProvider::new(client, model.name.clone())))
        }

        ModelProvider::DeepSeek => {
            let client = rig::providers::deepseek::Client::new(&model.api_key)
                .map_err(|e| AppError::Provider(format!("DeepSeek init: {}", e)))?;
            Ok(Box::new(RigProvider::new(client, model.name.clone())))
        }

        ModelProvider::Ollama => {
            let client = rig::providers::ollama::Client::new(model.api_key.clone())
                .map_err(|e| AppError::Provider(format!("Ollama init: {}", e)))?;
            Ok(Box::new(RigProvider::new(client, model.name.clone())))
        }

        ModelProvider::Moonshot => {
            let client = rig::providers::moonshot::Client::new(&model.api_key)
                .map_err(|e| AppError::Provider(format!("Moonshot init: {}", e)))?;
            Ok(Box::new(RigProvider::new(client, model.name.clone())))
        }

        // OpenAI-compatible providers: Zhipu, SiliconFlow, LMStudio, Custom
        //
        // These use Rig's OpenAI **Completions** API client (Chat Completions,
        // endpoint `/chat/completions`) instead of the newer Responses API,
        // because that is what these third-party providers implement.
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

            let mut builder = rig::providers::openai::CompletionsClient::builder()
                .api_key(&model.api_key);

            let api_base = strip_api_suffix(&effective_base_url);
            if !api_base.is_empty() {
                builder = builder.base_url(api_base);
            }

            if has_custom_base {
                log::info!(
                    "Using OpenAI-compatible Rig CompletionsClient for '{}' with base_url: {}",
                    model.id, api_base
                );
            }

            let client = builder.build()
                .map_err(|e| AppError::Provider(format!("OpenAI-compat init: {}", e)))?;
            Ok(Box::new(RigProvider::new(client, model.name.clone())))
        }
    }
}

// ---------------------------------------------------------------------------
// Structured extraction (Phase 3)
// ---------------------------------------------------------------------------

/// Extract structured data from text using Rig's [`Extractor`].
///
/// The generic parameter `T` is the target struct.  It must derive
/// [`Serialize`], [`Deserialize`], and [`JsonSchema`].
///
/// This is used by the intent classifier and execution planner to
/// replace brittle manual JSON parsing.
pub async fn extract_structured<T>(
    api_key: &str,
    model: &str,
    system_prompt: &str,
    user_input: &str,
) -> Result<T>
where
    T: Serialize + for<'de> Deserialize<'de> + schemars::JsonSchema + Send + Sync + 'static,
{
    let client = rig::providers::openai::Client::new(api_key)
        .map_err(|e| AppError::Provider(format!("Extract client init: {}", e)))?;

    // Combine system prompt and user input, since the extractor
    // doesn't have a separate preamble method.
    let combined = format!("{}\n\n{}", system_prompt, user_input);

    let extractor = client
        .extractor::<T>(model)
        .build();

    let response = extractor
        .extract_with_usage(&combined)
        .await
        .map_err(|e| AppError::Provider(format!("Extraction failed: {}", e)))?;

    Ok(response.data)
}
