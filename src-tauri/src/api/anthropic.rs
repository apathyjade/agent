use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::StreamExt;
use reqwest::Client;
use serde_json::json;

use crate::api::types::{ChatRequest, ChatResponse, Choice, Message, MessageRole, StreamPayload, ToolCall, Usage};
use crate::config::ModelConfig;
use crate::error::{AppError, Result};

use super::provider::LLMProvider;

pub struct AnthropicProvider {
    client: Client,
    model: ModelConfig,
}

impl AnthropicProvider {
    pub fn new(model: ModelConfig) -> Self {
        Self {
            client: Client::new(),
            model,
        }
    }

    fn api_url(&self) -> String {
        self.model.effective_base_url()
    }

    /// Build an Anthropic-format message from our internal Message type.
    /// Anthropic uses content blocks (array) for tool interactions,
    /// and a top-level `system` field instead of a system message in the array.
    fn to_anthropic_msg(msg: &Message) -> serde_json::Value {
        let role_str = match msg.role {
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            // Tool results are sent to Anthropic as user messages with content blocks
            MessageRole::Tool => "user",
            MessageRole::System => "user",
        };

        match msg.role {
            MessageRole::Tool => {
                // Anthropic expects tool results as content blocks
                let content: Vec<serde_json::Value> = vec![json!({
                    "type": "tool_result",
                    "tool_use_id": msg.tool_call_id.as_deref().unwrap_or(""),
                    "content": msg.content,
                })];
                json!({ "role": role_str, "content": content })
            }
            MessageRole::Assistant if msg.tool_calls.is_some() => {
                // Assistant messages with tool calls use content blocks
                // First block is text content (may be empty), rest are tool_use blocks
                let mut content: Vec<serde_json::Value> = vec![];
                if !msg.content.is_empty() {
                    content.push(json!({ "type": "text", "text": msg.content }));
                }
                if let Some(ref calls) = msg.tool_calls {
                    for tc in calls {
                        content.push(json!({
                            "type": "tool_use",
                            "id": tc.id,
                            "name": tc.name,
                            "input": tc.arguments,
                        }));
                    }
                }
                json!({ "role": role_str, "content": content })
            }
            _ => {
                // Plain text messages: use string content for simplicity
                json!({ "role": role_str, "content": msg.content })
            }
        }
    }
}

#[async_trait]
impl LLMProvider for AnthropicProvider {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse> {
        let system_message = request
            .messages
            .iter()
            .find(|m| matches!(m.role, MessageRole::System))
            .map(|m| m.content.clone());

        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .filter(|m| !matches!(m.role, MessageRole::System))
            .map(Self::to_anthropic_msg)
            .collect();

        let mut body = json!({
            "model": self.model.name,
            "max_tokens": self.model.max_tokens.unwrap_or(4096),
            "messages": messages,
        });

        if let Some(system) = system_message {
            body["system"] = serde_json::Value::String(system);
        }

        if let Some(tools) = &request.tools {
            let anthropic_tools: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    json!({
                        "name": t.function.name,
                        "description": t.function.description,
                        "input_schema": t.function.parameters,
                    })
                })
                .collect();
            body["tools"] = serde_json::Value::Array(anthropic_tools);
        }

        let response = self
            .client
            .post(self.api_url())
            .header("x-api-key", &self.model.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let json: serde_json::Value = response.json().await?;

        let content = json["content"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|c| c["text"].as_str())
            .unwrap_or("")
            .to_string();

        let usage = Usage {
            prompt_tokens: json["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32,
            completion_tokens: json["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32,
            total_tokens: (json["usage"]["input_tokens"].as_u64().unwrap_or(0)
                + json["usage"]["output_tokens"].as_u64().unwrap_or(0)) as u32,
        };

        Ok(ChatResponse {
            id: json["id"].as_str().unwrap_or("").to_string(),
            choices: vec![Choice {
                message: Message {
                    id: None,
                    role: MessageRole::Assistant,
                    content,
                    tool_calls: None,
                    tool_call_id: None,
                },
                finish_reason: Some(json["stop_reason"].as_str().unwrap_or("").to_string()),
            }],
            usage: Some(usage),
        })
    }

    async fn chat_stream(&self, request: ChatRequest) -> Result<BoxStream<'static, Result<StreamPayload>>> {
        let system_message = request
            .messages
            .iter()
            .find(|m| matches!(m.role, MessageRole::System))
            .map(|m| m.content.clone());

        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .filter(|m| !matches!(m.role, MessageRole::System))
            .map(Self::to_anthropic_msg)
            .collect();

        let mut body = json!({
            "model": self.model.name,
            "max_tokens": self.model.max_tokens.unwrap_or(4096),
            "messages": messages,
            "stream": true,
        });

        if let Some(system) = system_message {
            body["system"] = serde_json::Value::String(system);
        }

        if let Some(tools) = &request.tools {
            let anthropic_tools: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    json!({
                        "name": t.function.name,
                        "description": t.function.description,
                        "input_schema": t.function.parameters,
                    })
                })
                .collect();
            body["tools"] = serde_json::Value::Array(anthropic_tools);
        }

        let client = self.client.clone();
        let api_url = self.api_url();
        let api_key = self.model.api_key.clone();

        let stream = async move {
            let response = client
                .post(&api_url)
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01")
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await?;

            let bytes_stream = response.bytes_stream();
            Ok::<_, AppError>(bytes_stream)
        }
        .await?;

        let parsed = stream.filter_map(|chunk| {
            async {
                match chunk {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes);
                        for line in text.lines() {
                            if line.starts_with("data: ") {
                                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line[6..]) {
                                    match json["type"].as_str() {
                                        Some("content_block_delta") => {
                                            if let Some(content) = json["delta"]["text"].as_str() {
                                                if !content.is_empty() {
                                                    return Some(Ok(StreamPayload {
                                                        content: Some(content.to_string()),
                                                        tool_calls: None,
                                                        finish_reason: None,
                                                    }));
                                                }
                                            }
                                        }
                                        Some("content_block_start") => {
                                            if json["content_block"]["type"].as_str() == Some("tool_use") {
                                                return Some(Ok(StreamPayload {
                                                    content: None,
                                                    tool_calls: Some(vec![ToolCall {
                                                        id: json["content_block"]["id"].as_str().unwrap_or("").to_string(),
                                                        name: json["content_block"]["name"].as_str().unwrap_or("").to_string(),
                                                        arguments: json!({}),
                                                    }]),
                                                    finish_reason: None,
                                                }));
                                            }
                                        }
                                        Some("content_block_stop") => {
                                            // Tool call content block finished, no additional data needed
                                        }
                                        Some("message_delta") => {
                                            if let Some(stop) = json["delta"]["stop_reason"].as_str() {
                                                return Some(Ok(StreamPayload {
                                                    content: None,
                                                    tool_calls: None,
                                                    finish_reason: Some(stop.to_string()),
                                                }));
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        None
                    }
                    Err(e) => Some(Err(AppError::Http(e))),
                }
            }
        });

        Ok(Box::pin(parsed))
    }
}
