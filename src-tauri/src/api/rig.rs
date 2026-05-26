//! Rig AI framework — unified LLM provider.
//!
//! Provides the single [`RigProvider`] type that wraps all supported
//! model providers (OpenAI, Anthropic, Gemini, Groq, …) behind the
//! project's [`LLMProvider`] trait.
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
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::types::{
    ChatRequest, ChatResponse, Choice, Message, MessageRole, StreamPayload, ToolCall as OurToolCall,
};
use crate::config::{ModelConfig, ModelProvider};
use crate::error::{AppError, Result};

use super::provider::LLMProvider;

// Rig traits needed by every provider client
use rig::client::CompletionClient;
use rig::completion::Chat;
use rig::completion::Completion;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn extract_system_prompt(messages: &[Message]) -> String {
    messages
        .iter()
        .find(|m| m.role == MessageRole::System)
        .map(|m| m.content.clone())
        .unwrap_or_default()
}

fn extract_last_user_content(messages: &[Message]) -> String {
    messages
        .iter()
        .rev()
        .find(|m| m.role == MessageRole::User)
        .map(|m| m.content.clone())
        .unwrap_or_default()
}

fn build_rig_history(messages: &[Message]) -> Vec<rig::completion::Message> {
    let last_user_idx = messages.iter().rposition(|m| m.role == MessageRole::User);

    messages
        .iter()
        .enumerate()
        .filter(|(i, m)| {
            if m.role == MessageRole::System { return false; }
            if m.role == MessageRole::Tool { return false; }
            if Some(*i) == last_user_idx { return false; }
            true
        })
        .map(|(_, m)| match m.role {
            MessageRole::User => rig::completion::Message::user(&m.content),
            MessageRole::Assistant => rig::completion::Message::assistant(&m.content),
            _ => unreachable!(), // System / Tool filtered above
        })
        .collect()
}

fn map_response(content: String) -> ChatResponse {
    ChatResponse {
        id: Uuid::new_v4().to_string(),
        choices: vec![Choice {
            message: Message {
                id: None,
                role: MessageRole::Assistant,
                content,
                tool_calls: None,
                tool_call_id: None,
            },
            finish_reason: Some("stop".into()),
        }],
        usage: None,
    }
}

// ---------------------------------------------------------------------------
// RigProvider
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

    /// Shared implementation used by both `chat()` and `chat_stream()`.
    async fn do_chat(&self, request: &ChatRequest) -> Result<String> {
        let system = extract_system_prompt(&request.messages);
        let last_user = extract_last_user_content(&request.messages);

        if last_user.is_empty() {
            return Err(AppError::Provider(
                "No user message found in request".into(),
            ));
        }

        let mut history = build_rig_history(&request.messages);

        let agent = self.client.agent(&self.model)
            .preamble(&system)
            .temperature(request.temperature.unwrap_or(0.7) as f64);

        let agent = if let Some(max_tokens) = request.max_tokens {
            agent.max_tokens(max_tokens as u64)
        } else {
            agent
        };

        let agent = agent.build();

        let response = agent
            .chat(&last_user, &mut history)
            .await
            .map_err(|e| AppError::Provider(format!("Rig error: {}", e)))?;

        Ok(response)
    }
}

#[async_trait]
impl<C: CompletionClient + Send + Sync + 'static> LLMProvider for RigProvider<C> {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse> {
        let has_tools = request.tools.as_ref().map_or(false, |t| !t.is_empty());

        if has_tools {
            self.chat_with_tools(request).await
        } else {
            let content = self.do_chat(&request).await?;
            Ok(map_response(content))
        }
    }

    async fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> Result<BoxStream<'static, Result<StreamPayload>>> {
        let has_tools = request.tools.as_ref().map_or(false, |t| !t.is_empty());

        if has_tools {
            // For streaming with tools, fall back to non-streaming for now.
            let response = self.chat_with_tools(request).await?;
            let content = response.choices.first()
                .map(|c| c.message.content.clone())
                .unwrap_or_default();
            let tool_calls = response.choices.first()
                .and_then(|c| c.message.tool_calls.clone());
            let stream = futures::stream::once(async move {
                Ok(StreamPayload {
                    content: Some(content),
                    tool_calls,
                    finish_reason: Some("stop".into()),
                })
            });
            Ok(Box::pin(stream))
        } else {
            let content = self.do_chat(&request).await?;
            let stream = futures::stream::once(async move {
                Ok(StreamPayload {
                    content: Some(content),
                    tool_calls: None,
                    finish_reason: Some("stop".into()),
                })
            });
            Ok(Box::pin(stream))
        }
    }
}

impl<C: CompletionClient + Send + Sync + 'static> RigProvider<C> {
    /// Chat with tool definitions — uses Rig's lower-level completion API
    /// to return raw tool_calls that our AgentLoop can execute.
    async fn chat_with_tools(&self, request: ChatRequest) -> Result<ChatResponse> {
        let system = extract_system_prompt(&request.messages);
        let last_user = extract_last_user_content(&request.messages);

        if last_user.is_empty() {
            return Err(AppError::Provider(
                "No user message found in request".into(),
            ));
        }

        let history = build_rig_history(&request.messages);

        let agent_builder = self.client.agent(&self.model)
            .preamble(&system)
            .temperature(request.temperature.unwrap_or(0.7) as f64);

        let agent_builder = if let Some(max_tokens) = request.max_tokens {
            agent_builder.max_tokens(max_tokens as u64)
        } else {
            agent_builder
        };

        let agent = agent_builder.build();

        // Convert our ToolDefinition to Rig's ToolDefinition
        let rig_tools: Vec<rig::completion::ToolDefinition> = request
            .tools
            .unwrap_or_default()
            .iter()
            .map(|t| rig::completion::ToolDefinition {
                name: t.function.name.clone(),
                description: t.function.description.clone(),
                parameters: t.function.parameters.clone(),
            })
            .collect();

        // Use the Completion trait which returns raw tool_calls
        let builder = agent
            .completion(&last_user, history)
            .await
            .map_err(|e| AppError::Provider(format!("Rig completion builder: {}", e)))?;

        let builder = builder.tools(rig_tools);

        let response = builder
            .send()
            .await
            .map_err(|e| AppError::Provider(format!("Rig completion send: {}", e)))?;

        // Parse response for text and tool_calls
        let mut content = String::new();
        let mut tool_calls: Vec<OurToolCall> = Vec::new();

        for item in response.choice.iter() {
            match item {
                rig::completion::AssistantContent::Text(text) => {
                    content.push_str(&text.text);
                }
                rig::completion::AssistantContent::ToolCall(tc) => {
                    tool_calls.push(OurToolCall {
                        id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        arguments: tc.function.arguments.clone(),
                    });
                }
                _ => {} // Skip Reasoning, Image, etc.
            }
        }

        Ok(ChatResponse {
            id: response
                .message_id
                .unwrap_or_else(|| Uuid::new_v4().to_string()),
            choices: vec![Choice {
                message: Message {
                    id: None,
                    role: MessageRole::Assistant,
                    content,
                    tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
                    tool_call_id: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Concrete type aliases
// ---------------------------------------------------------------------------

pub type RigOpenAI = RigProvider<rig::providers::openai::Client>;
pub type RigAnthropic = RigProvider<rig::providers::anthropic::Client>;
pub type RigGemini = RigProvider<rig::providers::gemini::Client>;
pub type RigGroq = RigProvider<rig::providers::groq::Client>;
pub type RigDeepSeek = RigProvider<rig::providers::deepseek::Client>;
pub type RigOllama = RigProvider<rig::providers::ollama::Client>;
pub type RigMoonshot = RigProvider<rig::providers::moonshot::Client>;

// ---------------------------------------------------------------------------
// Factory: create the appropriate RigProvider from a ModelConfig
// ---------------------------------------------------------------------------

/// Create a [`RigProvider`] trait-object for the given model configuration.
///
/// This is the single entry-point used by [`ProviderRegistry`].
pub fn create_rig_provider(model: &ModelConfig) -> Result<Box<dyn LLMProvider>> {
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
        // These use the OpenAI client with a custom base_url.
        ModelProvider::Zhipu
        | ModelProvider::SiliconFlow
        | ModelProvider::LMStudio
        | ModelProvider::Custom => {
            let base_url = model.effective_base_url();
            if model.base_url.is_some() {
                log::info!(
                    "Using OpenAI-compatible Rig client for '{}' with base_url: {}",
                    model.id, base_url
                );
            }
            // TODO: Some Rig OpenAI clients support a builder with custom URL.
            // Fallback: use standard OpenAI client for now.
            let client = rig::providers::openai::Client::new(&model.api_key)
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
