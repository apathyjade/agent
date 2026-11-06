//! 工具循环：把消息交给 LLM，执行其请求的工具调用，直到得到最终回答。
//!
//! 同时提供非流式与流式两个入口；流式入口会把文本增量、工具调用与工具结果
//! 封装成 [`StreamEvent`] 推给上层。

use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use serde::{Serialize, Deserialize};

use futures::StreamExt;
use rig::agent::MultiTurnStreamItem;
use rig::completion::Message;

use crate::llm::provider::{ProviderRegistry, ToolRuntime, TurnOutcome};
use crate::core::error::Result;
use crate::agent::tools::registry::ToolRegistry;

// ================================================================
// IPC 类型 —— 结构保持不变以兼容 Tauri 前端。
// ================================================================

/// 工具循环向上层推送的流式事件。
#[derive(Clone, Serialize, Deserialize)]
pub enum StreamEvent {
    /// 文本增量。
    Content(String),
    /// 模型发起了一次工具调用。
    ToolCall(ToolCallInfo),
    /// 工具调用返回了结果。
    ToolResult(ToolResultInfo),
    /// 流式错误：上层应把它展示为失败，而不是当普通文本追加。
    Error(String),
    /// 流结束。
    Done,
}

/// 一次工具调用的标识信息。
#[derive(Clone, Serialize, Deserialize)]
pub struct ToolCallInfo {
    /// 工具调用 id。
    pub id: String,
    /// 工具名称。
    pub name: String,
    /// 模型给出的参数（用于前端展示与落库）。
    pub arguments: serde_json::Value,
}

/// 一次工具调用的返回结果。
#[derive(Clone, Serialize, Deserialize)]
pub struct ToolResultInfo {
    /// 对应的工具调用 id。
    pub call_id: String,
    /// 工具名称。
    pub name: String,
    /// 工具返回的文本内容。
    pub result: String,
    /// 工具是否执行失败。
    ///
    /// 直接透传 Rig 的 `ToolResult::is_error`。此前该字段被丢弃，
    /// 上层只能靠匹配没有生产者的字符串前缀来判断失败，导致工具失败
    /// 永远被当作成功上报给前端。
    pub is_error: bool,
}

// ================================================================
// ToolLoop —— 内部使用 ProviderBox 的简化 agent 循环。
//
// 直接调用 ProviderBox::prompt() / chat() / prompt_with_tools()。
// 旧的 AgentLoop 及其依赖的 ChatRequest/ChatResponse 已一并移除。
// ================================================================

/// 简化版 agent 循环：在单次或多次迭代中驱动模型与工具。
pub struct ToolLoop {
    providers: Arc<Mutex<ProviderRegistry>>,
    tools: Arc<Mutex<ToolRegistry>>,
    max_iterations: usize,
}

impl ToolLoop {
    /// 用共享的 provider 与工具注册表构造循环，默认最多 10 轮。
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

    /// 设置最大迭代轮数（至少 1 轮）。
    pub fn with_max_iterations(mut self, max_iterations: usize) -> Self {
        self.max_iterations = max_iterations;
        self
    }

    /// 非流式运行。
    ///
    /// 接收调用方已构造好的 `Vec<Message>`，拆出系统提示词与最后一条用户消息，
    /// 再交给 ProviderBox。启用工具时走 Rig 的 agent 工具循环
    /// （`prompt_with_tools` / `chat_with_tools`）。
    pub async fn run(
        &self,
        model_id: &str,
        messages: Vec<Message>,
        tools_enabled: bool,
        allowed_tools: Option<Vec<String>>,
    ) -> Result<TurnOutcome> {
        let provider = {
            let registry = self.providers.lock().await;
            registry.get(model_id)?
        };

        let (preamble, prompt, history) = extract_chat_params(messages);
        let tools = {
            let registry = self.tools.lock().await;
            if tools_enabled {
                registry.tool_runtime(allowed_tools.as_deref())
            } else {
                ToolRuntime::empty()
            }
        };
        let max_turns = self.max_iterations.max(1);

        if history.is_empty() {
            provider
                .prompt_with_tools(&preamble, &prompt, tools, max_turns)
                .await
        } else {
            provider
                .chat_with_tools(&preamble, &prompt, &history, tools, max_turns)
                .await
        }
    }

    /// 流式运行，支持工具调用。
    ///
    /// 消费 Rig 的 agent 流，并通过返回通道把文本增量、工具调用、工具结果
    /// 与最终响应以 [`StreamEvent`] 的形式转发给调用方。
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
        let (preamble, prompt, history) = extract_chat_params(messages);
        let tools = {
            let registry = self.tools.lock().await;
            if tools_enabled {
                registry.tool_runtime(allowed_tools.as_deref())
            } else {
                ToolRuntime::empty()
            }
        };
        let max_turns = self.max_iterations.max(1);

        tokio::spawn(async move {
            let mut stream = match provider
                .stream_with_tools(&preamble, &prompt, &history, tools, max_turns)
                .await
            {
                Ok(stream) => stream,
                Err(e) => {
                    let _ = tx.send(StreamEvent::Error(e.to_string())).await;
                    let _ = tx.send(StreamEvent::Done).await;
                    return;
                }
            };

            while let Some(item) = stream.next().await {
                let item = match item {
                    Ok(item) => item,
                    Err(e) => {
                        let _ = tx.send(StreamEvent::Error(e.to_string())).await;
                        break;
                    }
                };

                match item {
                    MultiTurnStreamItem::FinalResponse(_) => {
                        // 文本已经通过增量发过；这里只结束流，避免最终回答重复。
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
                                arguments: tool_call.function.arguments_value(),
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
                                is_error: tool_result.is_error,
                            }))
                            .await;
                    }
                    _ => {}
                }
            }

            // 所有正常/异常退出路径都恰好发一次 Done。
            let _ = tx.send(StreamEvent::Done).await;
        });

        Ok(rx)
    }

    /// 构造一个带工具集的 Rig Agent 并运行。
    ///
    /// 工具通过 [`ProviderBox::prompt_with_tools()`] 与
    /// `ToolRegistry::to_rig_tools()` 注入；Rig 的 agent 会执行模型请求的
    /// 工具调用并回灌结果，直到模型停止。
    pub async fn run_with_agent(
        &self,
        model_id: &str,
        system_prompt: &str,
        user_message: &str,
        allowed_tools: Option<Vec<String>>,
    ) -> Result<TurnOutcome> {
        let provider = {
            let registry = self.providers.lock().await;
            registry.get(model_id)?
        };
        let tools = {
            let registry = self.tools.lock().await;
            registry.tool_runtime(allowed_tools.as_deref())
        };
        let max_turns = self.max_iterations.max(1);

        provider
            .prompt_with_tools(system_prompt, user_message, tools, max_turns)
            .await
    }
}

// ================================================================
// 辅助函数：把 Vec<Message> 拆成 (preamble, prompt, history)
// ================================================================

/// 从消息列表拆出系统提示词、最后一条用户消息与其余历史。
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

    // 从历史中移除最后一条用户消息 —— 它就是本次的 prompt。
    if let Some(pos) = history
        .iter()
        .rposition(|m| matches!(m, Message::User { .. }))
    {
        history.remove(pos);
    }

    (preamble, prompt, history)
}

// ================================================================
// 工具函数：token 估算
//
// 保留为独立公共函数，summarizer 与 compactor 生命周期模块仍在使用。
// ================================================================

/// 估算一段文本的 token 数。
///
/// 规则：CJK 字符按 2 token、ASCII 按 4 字符 1 token、其他字符按 1 token 计。
/// 这是给上下文裁剪用的近似值，不追求与具体分词器一致。
pub fn estimate_tokens(content: &str) -> usize {
    let mut cjk_chars: usize = 0;
    let mut ascii_chars: usize = 0;
    let mut other_chars: usize = 0;

    for ch in content.chars() {
        if ('\u{4E00}'..='\u{9FFF}').contains(&ch)
            || ('\u{3400}'..='\u{4DBF}').contains(&ch)
            || ('\u{F900}'..='\u{FAFF}').contains(&ch)
            || ('\u{2F800}'..='\u{2FA1F}').contains(&ch)
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
// 测试
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


