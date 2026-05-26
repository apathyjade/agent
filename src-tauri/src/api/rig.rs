//! Rig provider adapters.
//!
//! Wraps `rig::providers::openai` and `rig::providers::anthropic` behind
//! the project's [`LLMProvider`] trait so they can be registered side-by-side
//! with the existing direct-HTTP providers.
//!
//! # Phase 1 limitations
//!
//! - Tool calling is **not** forwarded to the project's tool system.
//!   Rig agent handles tool calls internally when tools are configured,
//!   but our `LLMProvider` trait does not round-trip tool results through
//!   Rig's agent loop.
//! - Streaming is a single-payload wrapper around the non-streaming
//!   response (no SSE parsing).
//! - Only `chat()` is implemented via `Agent::chat()` for multi-turn
//!   conversation. The `prompt()` helper is available for one-shot use.

use async_trait::async_trait;
use futures::stream::BoxStream;
use uuid::Uuid;

use crate::api::types::{
    ChatRequest, ChatResponse, Choice, Message, MessageRole, StreamPayload,
};
use crate::config::ModelConfig;
use crate::error::{AppError, Result};

use super::provider::LLMProvider;

// Import Rig's CompletionClient trait so `.agent()` is available on provider clients.
use rig::client::CompletionClient;
use rig::completion::Chat;

// ---------------------------------------------------------------------------
// Helper: map Rig's PromptError into AppError
// ---------------------------------------------------------------------------

fn map_rig_error(e: rig::completion::PromptError) -> AppError {
    AppError::Provider(format!("Rig error: {}", e))
}

// ---------------------------------------------------------------------------
// Helper: extract the system prompt (first System-role message content)
// ---------------------------------------------------------------------------

fn extract_system_prompt(messages: &[Message]) -> String {
    messages
        .iter()
        .find(|m| m.role == MessageRole::System)
        .map(|m| m.content.clone())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Helper: extract the last User-role message content
// ---------------------------------------------------------------------------

fn extract_last_user_content(messages: &[Message]) -> String {
    messages
        .iter()
        .rev()
        .find(|m| m.role == MessageRole::User)
        .map(|m| m.content.clone())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Helper: build Rig conversation history from our internal messages.
//
// Excludes:
// - System messages (passed as preamble instead)
// - Tool messages (not forwarded in Phase 1)
// - The last User message (passed as the `chat()` prompt instead)
// ---------------------------------------------------------------------------

fn build_rig_history(messages: &[Message]) -> Vec<rig::completion::Message> {
    // Find index of last user message so we can skip it
    let last_user_idx = messages.iter().rposition(|m| m.role == MessageRole::User);

    messages
        .iter()
        .enumerate()
        .filter(|(i, m)| {
            // Skip system messages
            if m.role == MessageRole::System {
                return false;
            }
            // Skip tool messages (not forwarded in Phase 1)
            if m.role == MessageRole::Tool {
                return false;
            }
            // Skip the last user message (it will be the prompt)
            if Some(*i) == last_user_idx {
                return false;
            }
            true
        })
        .map(|(_, m)| match m.role {
            MessageRole::User => rig::completion::Message::user(&m.content),
            MessageRole::Assistant => rig::completion::Message::assistant(&m.content),
            // System and Tool roles are filtered out above
            _ => unreachable!(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Helper: map Rig's response string into our ChatResponse type
// ---------------------------------------------------------------------------

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

// ===========================================================================
// Generic RigProvider<C>
// ===========================================================================

/// Generic Rig-backed provider parameterized by a [`CompletionClient`].
///
/// Use the type aliases [`RigOpenAIProvider`] and [`RigAnthropicProvider`]
/// for concrete provider types.
pub struct RigProvider<C: CompletionClient> {
    client: C,
    model: String,
}

impl<C: CompletionClient + Send + Sync + 'static> RigProvider<C> {
    /// Create a new generic Rig provider.
    pub fn new(client: C, model: String) -> Self {
        Self { client, model }
    }

    /// Shared chat logic used by both `chat()` and `chat_stream()`.
    async fn chat_inner(&self, request: &ChatRequest) -> Result<String> {
        let system = extract_system_prompt(&request.messages);
        let last_user = extract_last_user_content(&request.messages);

        if last_user.is_empty() {
            return Err(AppError::Provider(
                "No user message found in request".into(),
            ));
        }

        let mut history = build_rig_history(&request.messages);

        // Build the Rig agent with the same configuration as the request
        let agent = self
            .client
            .agent(&self.model)
            .preamble(&system)
            .temperature(request.temperature.unwrap_or(0.7) as f64);

        let agent = if let Some(max_tokens) = request.max_tokens {
            agent.max_tokens(max_tokens as u64)
        } else {
            agent
        };

        let agent = agent.build();

        // Run conversation
        let response = agent
            .chat(&last_user, &mut history)
            .await
            .map_err(map_rig_error)?;

        Ok(response)
    }
}

#[async_trait]
impl<C: CompletionClient + Send + Sync + 'static> LLMProvider for RigProvider<C> {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse> {
        let content = self.chat_inner(&request).await?;
        Ok(map_response(content))
    }

    async fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> Result<BoxStream<'static, Result<StreamPayload>>> {
        let result = self.chat_inner(&request).await?;

        let stream = futures::stream::once(async move {
            Ok(StreamPayload {
                content: Some(result),
                tool_calls: None,
                finish_reason: Some("stop".into()),
            })
        });

        Ok(Box::pin(stream))
    }
}

// ---------------------------------------------------------------------------
// Concrete constructors for RigProvider type aliases
// ---------------------------------------------------------------------------

impl RigProvider<rig::providers::openai::Client> {
    /// Create a Rig-backed OpenAI provider from a [`ModelConfig`].
    ///
    /// `model.api_key` should already be resolved via keychain.
    pub fn new_openai(model: &ModelConfig) -> Result<Self> {
        let client = rig::providers::openai::Client::new(&model.api_key)
            .map_err(|e| AppError::Provider(format!("Failed to create OpenAI client: {}", e)))?;
        Ok(Self {
            client,
            model: model.name.clone(),
        })
    }
}

impl RigProvider<rig::providers::anthropic::Client> {
    /// Create a Rig-backed Anthropic provider from a [`ModelConfig`].
    ///
    /// `model.api_key` should already be resolved via keychain.
    pub fn new_anthropic(model: &ModelConfig) -> Result<Self> {
        let client = rig::providers::anthropic::Client::new(&model.api_key)
            .map_err(|e| AppError::Provider(
                format!("Failed to create Anthropic client: {}", e),
            ))?;
        Ok(Self {
            client,
            model: model.name.clone(),
        })
    }
}

// ---------------------------------------------------------------------------
// Type aliases for external use (preserve public API names)
// ---------------------------------------------------------------------------

/// OpenAI provider backed by the Rig framework.
///
/// Uses `rig::providers::openai::Client` to create agents that handle
/// multi-turn chat internally.
pub type RigOpenAIProvider = RigProvider<rig::providers::openai::Client>;

/// Anthropic provider backed by the Rig framework.
///
/// Uses `rig::providers::anthropic::Client` to create agents that handle
/// multi-turn chat internally.
pub type RigAnthropicProvider = RigProvider<rig::providers::anthropic::Client>;
