use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use serde::{Serialize, Deserialize};

use futures::StreamExt;
use rig::agent::MultiTurnStreamItem;
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
    /// Accepts a `Vec<Message>` from callers that already construct message
    /// lists, extracts the system prompt and last user message, then delegates
    /// to ProviderBox. When tools are enabled the run goes through Rig's agent
    /// tool loop (`prompt_with_tools` / `chat_with_tools`).
    pub async fn run(
        &self,
        model_id: &str,
        messages: Vec<Message>,
        tools_enabled: bool,
        allowed_tools: Option<Vec<String>>,
    ) -> Result<String> {
        let provider = {
            let registry = self.providers.lock().await;
            registry.get(model_id)?
        };

        let (preamble, prompt, mut history) = extract_chat_params(messages);
        let tools = {
            let registry = self.tools.lock().await;
            if tools_enabled {
                registry.to_rig_tools(allowed_tools.as_deref())
            } else {
                Vec::new()
            }
        };
        let max_turns = self.max_iterations.max(1);

        if history.is_empty() {
            provider
                .prompt_with_tools(&preamble, &prompt, tools, max_turns)
                .await
        } else {
            provider
                .chat_with_tools(&preamble, &prompt, &mut history, tools, max_turns)
                .await
        }
    }

    /// Streaming agent run with tool support.
    ///
    /// Consumes Rig's agent stream and forwards text deltas, tool calls,
    /// tool results, and the final response to the caller via [`StreamEvent`]s
    /// on the returned channel.
    pub async fn run_stream(
        &self,
        model_id: &str,
        messages: Vec<Message>,
        tools_enabled: bool,
        allowed_tools: Option<Vec<String>>,
    ) -> Result<mpsc::Receiver<StreamEvent>> {
        let (tx, rx) = mpsc::channel(32);

        let provider = {
            let registry = self.providers.lock().await;
            registry.get(model_id)?
        };
        let (preamble, prompt, mut history) = extract_chat_params(messages);
        let tools = {
            let registry = self.tools.lock().await;
            if tools_enabled {
                registry.to_rig_tools(allowed_tools.as_deref())
            } else {
                Vec::new()
            }
        };
        let max_turns = self.max_iterations.max(1);

        tokio::spawn(async move {
            let stream = match provider
                .stream_with_tools(&preamble, &prompt, &mut history, tools, max_turns)
                .await
            {
                Ok(stream) => stream,
                Err(e) => {
                    let _ = tx.send(StreamEvent::Content(format!("\n[Error: {}]", e))).await;
                    let _ = tx.send(StreamEvent::Done).await;
                    return;
                }
            };

            let mut stream = stream;
            while let Some(item) = stream.next().await {
                let item = match item {
                    Ok(item) => item,
                    Err(e) => {
                        let _ = tx.send(StreamEvent::Content(format!("\n[Error: {}]", e))).await;
                        break;
                    }
                };

                match item {
                    MultiTurnStreamItem::FinalResponse(response) => {
                        let _ = tx.send(StreamEvent::Content(response.output())).await;
                        let _ = tx.send(StreamEvent::Done).await;
                        break;
                    }
                    MultiTurnStreamItem::StreamAssistantItem(rig::streaming::Item::Event(rig::streaming::StreamEvent::Text { text, .. })) => {
                        let _ = tx.send(StreamEvent::Content(text)).await;
                    }
                    MultiTurnStreamItem::StreamAssistantItem(rig::streaming::Item::Unknown(_)) => {}
                    MultiTurnStreamItem::ToolCall { tool_call } => {
                        let _ = tx
                            .send(StreamEvent::ToolCall(ToolCallInfo {
                                id: tool_call.id.to_string(),
                                name: tool_call.function.name.to_string(),
                            }))
                            .await;
                    }
                    MultiTurnStreamItem::ToolResult { tool_result } => {
                        let result = tool_result
                            .content
                            .iter()
                            .filter_map(|c| c.as_text())
                            .collect::<Vec<&str>>()
                            .join("\n");
                        let _ = tx
                            .send(StreamEvent::ToolResult(ToolResultInfo {
                                call_id: tool_result.call.to_string(),
                                name: tool_result.name.to_string(),
                                result,
                            }))
                            .await;
                    }
                    _ => {}
                }
            }
        });

        Ok(rx)
    }

    /// Build a Rig Agent with a tool set and run it.
    ///
    /// Tools are wired through [`ProviderBox::prompt_with_tools()`] with
    /// `ToolRegistry::to_rig_tools()`; Rig's agent executes the requested tool
    /// calls and feeds results back until it stops.
    pub async fn run_with_agent(
        &self,
        model_id: &str,
        system_prompt: &str,
        user_message: &str,
        allowed_tools: Option<Vec<String>>,
    ) -> Result<String> {
        let provider = {
            let registry = self.providers.lock().await;
            registry.get(model_id)?
        };
        let tools = {
            let registry = self.tools.lock().await;
            registry.to_rig_tools(allowed_tools.as_deref())
        };
        let max_turns = self.max_iterations.max(1);

        provider
            .prompt_with_tools(system_prompt, user_message, tools, max_turns)
            .await
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
            Message::User { content } => match content.first() {
                Some(rig::message::UserContent::Text(t)) => Some(t.text.clone()),
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
