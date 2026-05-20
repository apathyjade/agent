use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use serde::{Serialize, Deserialize};
use futures::StreamExt;

use crate::api::provider::{LLMProvider, ProviderRegistry};
use crate::api::types::{
    ChatRequest, ChatResponse, Message, MessageRole, ToolCall, ToolDefinition,
};
use crate::error::{AppError, Result};
use crate::tools::registry::ToolRegistry;

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

pub struct AgentLoop {
    providers: Arc<Mutex<ProviderRegistry>>,
    tools: Arc<Mutex<ToolRegistry>>,
    max_iterations: usize,
    max_context_tokens: usize,
}

impl AgentLoop {
    pub fn new(providers: Arc<Mutex<ProviderRegistry>>, tools: Arc<Mutex<ToolRegistry>>) -> Self {
        Self {
            providers,
            tools,
            max_iterations: 10,
            max_context_tokens: 32000,
        }
    }

    pub fn with_context_limit(mut self, max_tokens: usize) -> Self {
        self.max_context_tokens = max_tokens;
        self
    }

    pub fn with_max_iterations(mut self, max_iterations: usize) -> Self {
        self.max_iterations = max_iterations;
        self
    }

    pub async fn run(&self, model_id: &str, messages: Vec<Message>, tools_enabled: bool) -> Result<ChatResponse> {
        let provider = self.providers.lock().await.get(model_id)?;
        let tool_registry = self.tools.lock().await;

        let tool_definitions: Vec<ToolDefinition> = if tools_enabled {
            tool_registry.get_enabled()
                .iter()
                .map(|tool| ToolDefinition {
                    tool_type: "function".to_string(),
                    function: crate::api::types::FunctionDefinition {
                        name: tool.name().to_string(),
                        description: tool.description().to_string(),
                        parameters: tool.parameters(),
                    },
                })
                .collect()
        } else {
            vec![]
        };

        drop(tool_registry);

        let optimized_messages = Self::optimize_context(&messages, self.max_context_tokens);
        let mut current_messages = optimized_messages;
        let mut iteration = 0;

        loop {
            if iteration >= self.max_iterations {
                return Err(AppError::Tool("Max iterations reached".to_string()));
            }

            let chat_request = ChatRequest {
                messages: current_messages.clone(),
                model: model_id.to_string(),
                tools: if tool_definitions.is_empty() { None } else { Some(tool_definitions.clone()) },
                stream: Some(false),
            };

            let response = Self::retry_with_backoff(&chat_request, 3, provider.clone()).await?;

            if let Some(choice) = response.choices.first() {
                let assistant_message = &choice.message;

                if let Some(tool_calls) = &assistant_message.tool_calls {
                    current_messages.push(assistant_message.clone());

                    for tool_call in tool_calls {
                        let tool_result = self.execute_tool(tool_call).await?;
                        current_messages.push(Message {
                            id: None,
                            role: MessageRole::Tool,
                            content: serde_json::to_string(&tool_result).unwrap_or_default(),
                            tool_calls: None,
                            tool_call_id: Some(tool_call.id.clone()),
                        });
                    }

                    iteration += 1;
                    continue;
                }
            }

            return Ok(response);
        }
    }

    pub async fn run_stream(&self, model_id: &str, messages: Vec<Message>, tools_enabled: bool) -> Result<mpsc::Receiver<StreamEvent>> {
        let (tx, rx) = mpsc::channel(32);

        let providers = self.providers.clone();
        let tools = self.tools.clone();
        let max_iterations = self.max_iterations;
        let max_context_tokens = self.max_context_tokens;
        let mid = model_id.to_string();

        tokio::spawn(async move {
            let optimized = Self::optimize_context(&messages, max_context_tokens);

            if let Err(e) = Self::run_stream_inner(&providers, &tools, &mid, optimized, tools_enabled, &tx, max_iterations).await {
                let _ = tx.send(StreamEvent::Content(format!("\n[Error: {}]", e))).await;
            }
            let _ = tx.send(StreamEvent::Done).await;
        });

        Ok(rx)
    }

    fn optimize_context(messages: &[Message], max_tokens: usize) -> Vec<Message> {
        if messages.is_empty() {
            return vec![];
        }

        let mut system_msg: Option<Message> = None;
        let rest: Vec<&Message> = messages.iter()
            .filter(|m| {
                if m.role == MessageRole::System {
                    system_msg = Some((*m).clone());
                    false
                } else {
                    true
                }
            })
            .collect();

        let mut total_tokens = system_msg.as_ref()
            .map(|m| Self::estimate_tokens(&m.content))
            .unwrap_or(0);
        let mut selected = Vec::new();

        for msg in rest.iter().rev() {
            let msg_tokens = Self::estimate_tokens(&msg.content);
            if total_tokens + msg_tokens > max_tokens && !selected.is_empty() {
                break;
            }
            total_tokens += msg_tokens;
            selected.push((*msg).clone());
        }

        selected.reverse();

        let mut result: Vec<Message> = Vec::new();
        if let Some(sys) = system_msg {
            result.push(sys);
        }
        result.extend(selected);

        result
    }

    fn estimate_tokens(content: &str) -> usize {
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

        // CJK chars: ~2 tokens each (safer overestimate)
        // ASCII: ~0.25 tokens each (4 chars ≈ 1 token)
        // Other (emoji, etc.): ~1 token each
        let cjk_tokens = cjk_chars * 2;
        let ascii_tokens = ascii_chars.div_ceil(4);
        let other_tokens = other_chars;

        // Ensure at least 1 token for non-empty content
        let total = cjk_tokens + ascii_tokens + other_tokens;
        if total == 0 && !content.is_empty() { 1 } else { total }
    }

    async fn retry_with_backoff(request: &ChatRequest, max_retries: u32, provider: Arc<dyn LLMProvider>) -> Result<ChatResponse> {
        let mut last_error = None;

        for attempt in 0..max_retries {
            match provider.chat(request.clone()).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    // Don't retry non-retryable errors (auth, invalid input, etc.)
                    if !e.is_retryable() {
                        return Err(e);
                    }
                    last_error = Some(e);
                    if attempt < max_retries - 1 {
                        let delay = std::time::Duration::from_millis(100 * 2u64.pow(attempt));
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| AppError::Provider("Unknown error after retries".to_string())))
    }

    async fn run_stream_inner(
        providers: &Arc<Mutex<ProviderRegistry>>,
        tools: &Arc<Mutex<ToolRegistry>>,
        model_id: &str,
        messages: Vec<Message>,
        tools_enabled: bool,
        tx: &mpsc::Sender<StreamEvent>,
        max_iterations: usize,
    ) -> Result<()> {
        let provider = providers.lock().await.get(model_id)?;
        let tool_definitions = {
            let tool_registry = tools.lock().await;
            if tools_enabled {
                tool_registry.get_enabled()
                    .iter()
                    .map(|tool| ToolDefinition {
                        tool_type: "function".to_string(),
                        function: crate::api::types::FunctionDefinition {
                            name: tool.name().to_string(),
                            description: tool.description().to_string(),
                            parameters: tool.parameters(),
                        },
                    })
                    .collect()
            } else {
                vec![]
            }
        };

        let mut current_messages = messages;
        let mut iteration = 0;

        loop {
            if iteration >= max_iterations {
                return Err(AppError::Tool("Max iterations reached".to_string()));
            }

            let chat_request = ChatRequest {
                messages: current_messages.clone(),
                model: model_id.to_string(),
                tools: if tool_definitions.is_empty() { None } else { Some(tool_definitions.clone()) },
                stream: Some(true),
            };

            let mut stream = provider.chat_stream(chat_request).await?;
            let mut full_content = String::new();
            let mut detected_tool_calls: Vec<ToolCall> = Vec::new();

            while let Some(result) = stream.next().await {
                match result {
                    Ok(payload) => {
                        if let Some(content) = payload.content {
                            full_content.push_str(&content);
                            let _ = tx.send(StreamEvent::Content(content)).await;
                        }

                        if let Some(tool_calls) = payload.tool_calls {
                            for tc in tool_calls {
                                detected_tool_calls.push(tc);
                                let _ = tx.send(StreamEvent::ToolCall(ToolCallInfo {
                                    id: detected_tool_calls.last().map(|t| t.id.clone()).unwrap_or_default(),
                                    name: detected_tool_calls.last().map(|t| t.name.clone()).unwrap_or_default(),
                                })).await;
                            }
                        }

                        if let Some(_finish_reason) = payload.finish_reason {
                            // Streaming complete for this round
                        }
                    }
                    Err(e) => return Err(e),
                }
            }

            current_messages.push(Message {
                id: None,
                role: MessageRole::Assistant,
                content: full_content.clone(),
                tool_calls: if detected_tool_calls.is_empty() { None } else { Some(detected_tool_calls.clone()) },
                tool_call_id: None,
            });

            if detected_tool_calls.is_empty() {
                return Ok(());
            }

            for tc in &detected_tool_calls {
                let result = {
                    let tool_registry = tools.lock().await;
                    tool_registry.execute(&tc.name, tc.arguments.clone()).await
                };

                match result {
                    Ok(value) => {
                        let result_str = serde_json::to_string(&value).unwrap_or_default();
                        let _ = tx.send(StreamEvent::ToolResult(ToolResultInfo {
                            call_id: tc.id.clone(),
                            name: tc.name.clone(),
                            result: result_str.clone(),
                        })).await;

                        current_messages.push(Message {
                            id: None,
                            role: MessageRole::Tool,
                            content: result_str,
                            tool_calls: None,
                            tool_call_id: Some(tc.id.clone()),
                        });
                    }
                    Err(e) => {
                        let err_str = format!("Tool execution error: {}", e);
                        current_messages.push(Message {
                            id: None,
                            role: MessageRole::Tool,
                            content: err_str,
                            tool_calls: None,
                            tool_call_id: Some(tc.id.clone()),
                        });
                    }
                }
            }

            iteration += 1;
        }
    }

    async fn execute_tool(&self, tool_call: &ToolCall) -> Result<serde_json::Value> {
        let tools = self.tools.lock().await;
        let input = tool_call.arguments.clone();
        tools.execute(&tool_call.name, input).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_optimize_context_preserves_system_message() {
        let msgs = vec![
            Message { id: None, role: MessageRole::System, content: "You are a helpful assistant".to_string(), tool_calls: None, tool_call_id: None },
            Message { id: None, role: MessageRole::User, content: "Hello".to_string(), tool_calls: None, tool_call_id: None },
            Message { id: None, role: MessageRole::Assistant, content: "Hi there!".to_string(), tool_calls: None, tool_call_id: None },
        ];
        let result = AgentLoop::optimize_context(&msgs, 4000);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].role, MessageRole::System);
        assert_eq!(result[0].content, "You are a helpful assistant");
    }

    #[test]
    fn test_optimize_context_empty_input() {
        let result = AgentLoop::optimize_context(&[], 4000);
        assert!(result.is_empty());
    }

    #[test]
    fn test_optimize_context_no_system_message() {
        let msgs = vec![
            Message { id: None, role: MessageRole::User, content: "Hello".to_string(), tool_calls: None, tool_call_id: None },
        ];
        let result = AgentLoop::optimize_context(&msgs, 4000);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content, "Hello");
    }

    #[test]
    fn test_optimize_context_respects_token_limit() {
        let long_content = "A".repeat(200);
        let msgs = vec![
            Message { id: None, role: MessageRole::User, content: "short".to_string(), tool_calls: None, tool_call_id: None },
            Message { id: None, role: MessageRole::Assistant, content: long_content.clone(), tool_calls: None, tool_call_id: None },
            Message { id: None, role: MessageRole::User, content: "latest".to_string(), tool_calls: None, tool_call_id: None },
        ];
        let result = AgentLoop::optimize_context(&msgs, 10);
        assert!(result.len() < 3);
        assert!(!result.is_empty());
        assert_eq!(result.last().unwrap().content, "latest");
    }

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(AgentLoop::estimate_tokens("hello"), 2);
        assert_eq!(AgentLoop::estimate_tokens("a"), 1);
        assert_eq!(AgentLoop::estimate_tokens(""), 0);
        assert_eq!(AgentLoop::estimate_tokens("abcd"), 1);
        assert_eq!(AgentLoop::estimate_tokens("abcde"), 2);
    }
}
