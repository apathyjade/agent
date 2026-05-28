use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use serde::{Serialize, Deserialize};

use rig::completion::Message;

use crate::api::provider::ProviderRegistry;
use crate::error::Result;
use crate::tools::registry::ToolRegistry;

// ================================================================
// IPC types — kept unchanged for Tauri frontend compatibility.
// ================================================================

#[derive(Clone, Serialize, Deserialize)]
pub enum StreamEvent {
    Content(String),
    ToolCall(ToolCallInfo),
    ToolResult(ToolResultInfo),
    Done,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ToolCallInfo {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ToolResultInfo {
    pub call_id: String,
    pub name: String,
    pub result: String,
}

// ================================================================
// ToolLoop — simplified agent loop using ProviderBox internally.
//
// Phase 3.1: Uses ProviderBox::prompt() / chat() / prompt_with_tools()
// directly. The old AgentLoop was removed along with the
// ChatRequest/ChatResponse types it relied on.
// ================================================================

pub struct ToolLoop {
    providers: Arc<Mutex<ProviderRegistry>>,
    tools: Arc<Mutex<ToolRegistry>>,
    max_iterations: usize,
}

impl ToolLoop {
    pub fn new(
        providers: Arc<Mutex<ProviderRegistry>>,
        tools: Arc<Mutex<ToolRegistry>>,
    ) -> Self {
        Self {
            providers,
            tools,
            max_iterations: 10,
        }
    }

    pub fn with_max_iterations(mut self, max_iterations: usize) -> Self {
        self.max_iterations = max_iterations;
        self
    }

    /// Non-streaming agent run.
    ///
    /// Accepts a `Vec<Message>` (Rig v0.37 enum) for backward compatibility
    /// with callers that already construct message lists, extracts the system
    /// prompt and last user message, then delegates to ProviderBox.
    pub async fn run(
        &self,
        model_id: &str,
        messages: Vec<Message>,
        _tools_enabled: bool,
        _allowed_tools: Option<Vec<String>>,
    ) -> Result<String> {
        let provider = {
            let registry = self.providers.lock().await;
            registry.get(model_id)?
        };

        let (preamble, prompt, mut history) = extract_chat_params(messages);

        if history.is_empty() {
            provider.prompt(&preamble, &prompt).await
        } else {
            provider.chat(&preamble, &prompt, &mut history).await
        }
    }

    /// Streaming agent run — non-streaming fallback.
    ///
    /// Sends the full response as a single [`StreamEvent::Content`] event
    /// followed by [`StreamEvent::Done`].
    pub async fn run_stream(
        &self,
        model_id: &str,
        messages: Vec<Message>,
        tools_enabled: bool,
        allowed_tools: Option<Vec<String>>,
    ) -> Result<mpsc::Receiver<StreamEvent>> {
        let (tx, rx) = mpsc::channel(32);

        let providers = self.providers.clone();
        let tools = self.tools.clone();
        let mid = model_id.to_string();
        let max_iter = self.max_iterations;

        tokio::spawn(async move {
            let loop_ = ToolLoop {
                providers,
                tools,
                max_iterations: max_iter,
            };

            match loop_
                .run(&mid, messages, tools_enabled, allowed_tools)
                .await
            {
                Ok(content) => {
                    let _ = tx.send(StreamEvent::Content(content)).await;
                }
                Err(e) => {
                    let _ = tx
                        .send(StreamEvent::Content(format!("\n[Error: {}]", e)))
                        .await;
                }
            }
            let _ = tx.send(StreamEvent::Done).await;
        });

        Ok(rx)
    }

    /// Build a Rig Agent with ToolSet and run it.
    ///
    /// Currently delegates to [`ProviderBox::prompt()`]; proper ToolSet
    /// integration (via `prompt_with_tools`) will be enabled in Phase 3.2.
    pub async fn run_with_agent(
        &self,
        model_id: &str,
        system_prompt: &str,
        user_message: &str,
        _allowed_tools: Option<Vec<String>>,
    ) -> Result<String> {
        let provider = {
            let registry = self.providers.lock().await;
            registry.get(model_id)?
        };

        provider.prompt(system_prompt, user_message).await
    }
}

// ================================================================
// Helper: split Vec<Message> into (preamble, prompt, history)
// ================================================================

fn extract_chat_params(messages: Vec<Message>) -> (String, String, Vec<Message>) {
    let preamble = messages
        .iter()
        .find_map(|m| match m {
            Message::System { content } => Some(content.clone()),
            _ => None,
        })
        .unwrap_or_default();

    let prompt = messages
        .iter()
        .rev()
        .find_map(|m| match m {
            Message::User { content } => match content.first_ref() {
                rig::message::UserContent::Text(t) => Some(t.text.clone()),
                _ => None,
            },
            _ => None,
        })
        .unwrap_or_default();

    let mut history: Vec<Message> = messages
        .into_iter()
        .filter(|m| !matches!(m, Message::System { .. }))
        .collect();

    // Remove the last user message from history — it is the prompt
    if let Some(pos) = history
        .iter()
        .rposition(|m| matches!(m, Message::User { .. }))
    {
        history.remove(pos);
    }

    (preamble, prompt, history)
}

// ================================================================
// Utility: token estimation
//
// Kept as a standalone public function for the summarizer and
// compactor lifecycle modules which still rely on it.
// ================================================================

pub fn estimate_tokens(content: &str) -> usize {
    let mut cjk_chars: usize = 0;
    let mut ascii_chars: usize = 0;
    let mut other_chars: usize = 0;

    for ch in content.chars() {
        if (ch >= '\u{4E00}' && ch <= '\u{9FFF}')
            || (ch >= '\u{3400}' && ch <= '\u{4DBF}')
            || (ch >= '\u{F900}' && ch <= '\u{FAFF}')
            || (ch >= '\u{2F800}' && ch <= '\u{2FA1F}')
        {
            cjk_chars += 1;
        } else if ch.is_ascii() {
            ascii_chars += 1;
        } else {
            other_chars += 1;
        }
    }

    let cjk_tokens = cjk_chars * 2;
    let ascii_tokens = ascii_chars.div_ceil(4);
    let other_tokens = other_chars;

    let total = cjk_tokens + ascii_tokens + other_tokens;
    if total == 0 && !content.is_empty() {
        1
    } else {
        total
    }
}

// ================================================================
// Tests
// ================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens("hello"), 2);
        assert_eq!(estimate_tokens("a"), 1);
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
    }

    #[test]
    fn test_extract_chat_params_system_and_user() {
        let msgs = vec![
            Message::system("You are a helpful assistant."),
            Message::user("Hello!"),
        ];
        let (preamble, prompt, history) = extract_chat_params(msgs);
        assert_eq!(preamble, "You are a helpful assistant.");
        assert_eq!(prompt, "Hello!");
        assert!(history.is_empty());
    }

    #[test]
    fn test_extract_chat_params_multi_turn() {
        let msgs = vec![
            Message::system("Be concise."),
            Message::user("What is Rust?"),
            Message::assistant("A systems language."),
            Message::user("Show me an example."),
        ];
        let (preamble, prompt, history) = extract_chat_params(msgs);
        assert_eq!(preamble, "Be concise.");
        assert_eq!(prompt, "Show me an example.");
        assert_eq!(history.len(), 2);
    }

    #[test]
    fn test_extract_chat_params_no_system() {
        let msgs = vec![Message::user("Hi")];
        let (preamble, prompt, history) = extract_chat_params(msgs);
        assert_eq!(preamble, "");
        assert_eq!(prompt, "Hi");
        assert!(history.is_empty());
    }
}
