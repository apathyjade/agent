use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt};
use reqwest::Client;
use serde_json::json;
use std::collections::HashMap;

use crate::api::types::{ChatRequest, ChatResponse, StreamPayload, ToolCall};
use crate::config::ModelConfig;
use crate::error::{AppError, Result};

use super::provider::LLMProvider;

pub struct OpenAIProvider {
    client: Client,
    model: ModelConfig,
}

impl OpenAIProvider {
    pub fn new(model: ModelConfig) -> Self {
        let mut builder = Client::builder();
        if matches!(model.provider, crate::config::ModelProvider::Ollama) || matches!(model.provider, crate::config::ModelProvider::LMStudio) {
            builder = builder.danger_accept_invalid_certs(true);
        }
        Self {
            client: builder.build().unwrap_or_else(|_| Client::new()),
            model,
        }
    }

    fn api_url(&self) -> String {
        self.model.effective_base_url()
    }

    fn build_request_body(&self, request: &ChatRequest, stream: bool) -> serde_json::Value {
        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    crate::api::types::MessageRole::System => "system",
                    crate::api::types::MessageRole::User => "user",
                    crate::api::types::MessageRole::Assistant => "assistant",
                    crate::api::types::MessageRole::Tool => "tool",
                };
                let mut msg = json!({
                    "role": role,
                    "content": m.content,
                });
                if let Some(tool_calls) = &m.tool_calls {
                    msg["tool_calls"] = serde_json::to_value(tool_calls).unwrap_or(json!([]));
                }
                if let Some(tool_call_id) = &m.tool_call_id {
                    msg["tool_call_id"] = serde_json::Value::String(tool_call_id.clone());
                }
                msg
            })
            .collect();

        let mut body = json!({
            "model": self.model.name,
            "messages": messages,
            "stream": stream,
        });

        if let Some(tools) = &request.tools {
            body["tools"] = serde_json::to_value(tools).unwrap_or(json!([]));
        }

        if let Some(max_tokens) = self.model.max_tokens {
            body["max_tokens"] = serde_json::Value::Number(serde_json::Number::from(max_tokens));
        }

        body
    }
}

#[async_trait]
impl LLMProvider for OpenAIProvider {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse> {
        let body = self.build_request_body(&request, false);

        let mut req = self.client.post(self.api_url())
            .header("Content-Type", "application/json");

        if !self.model.api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", self.model.api_key));
        }

        let response = req
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            return Err(AppError::Provider(format!(
                "API returned {}: {}",
                status.as_u16(),
                error_body
            )));
        }

        let response_data = response
            .json::<ChatResponse>()
            .await?;

        Ok(response_data)
    }

    async fn chat_stream(&self, request: ChatRequest) -> Result<BoxStream<'static, Result<StreamPayload>>> {
        let body = self.build_request_body(&request, true);

        let client = self.client.clone();
        let api_url = self.api_url();
        let api_key = self.model.api_key.clone();

        let mut req = client.post(&api_url)
            .header("Content-Type", "application/json");

        if !api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", api_key));
        }

        let response = req
            .json(&body)
            .send()
            .await?;

        // Check HTTP status — API errors (4xx/5xx) are NOT SSE streams.
        // Without this check, JSON error bodies silently produce zero SSE events,
        // making it appear as if no API call was made.
        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            return Err(AppError::Provider(format!(
                "API returned {}: {}",
                status.as_u16(),
                error_body
            )));
        }

        let bytes_stream = response.bytes_stream();

        // flat_map emits ALL SSE events from each chunk, not just the first one.
        // This is critical — some providers (DeepSeek V4, etc.) batch multiple events
        // per HTTP chunk. filter_map + results.remove(0) would silently discard everything
        // after the first event, causing garbled/missing text.
        let parsed = bytes_stream.flat_map(|chunk| {
            let items: Vec<Result<StreamPayload>> = match chunk {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    let mut items = Vec::new();
                    let mut tool_calls_map: HashMap<i64, serde_json::Value> = HashMap::new();

                    for line in text.lines() {
                        if line.starts_with("data: ") && line != "data: [DONE]" {
                            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line[6..]) {
                                let delta = &json["choices"][0]["delta"];

                                // Emit content immediately — each SSE event that carries
                                // text delta is its own payload.
                                if let Some(content) = delta["content"].as_str() {
                                    if !content.is_empty() {
                                        items.push(Ok(StreamPayload {
                                            content: Some(content.to_string()),
                                            tool_calls: None,
                                            finish_reason: None,
                                        }));
                                    }
                                }

                                // Accumulate tool-call chunks within this HTTP chunk.
                                // (Multi-chunk tool calls are handled across flat_map invocations
                                //  because each chunk independently builds its own map.)
                                if let Some(tcs) = delta["tool_calls"].as_array() {
                                    for tc in tcs {
                                        let index = tc["index"].as_i64().unwrap_or(0);
                                        let entry = tool_calls_map.entry(index).or_insert_with(|| {
                                            json!({
                                                "id": "",
                                                "function": {"name": "", "arguments": ""}
                                            })
                                        });
                                        if let Some(id) = tc["id"].as_str() {
                                            if !id.is_empty() {
                                                entry["id"] = json!(id);
                                            }
                                        }
                                        if let Some(name) = tc["function"]["name"].as_str() {
                                            if !name.is_empty() {
                                                entry["function"]["name"] = json!(name);
                                            }
                                        }
                                        if let Some(args) = tc["function"]["arguments"].as_str() {
                                            let current = entry["function"]["arguments"].as_str().unwrap_or("");
                                            let merged = format!("{}{}", current, args);
                                            entry["function"]["arguments"] = json!(merged);
                                        }
                                    }
                                }

                                // Emit finish_reason (with accumulated tool_calls if applicable)
                                if let Some(finish) = json["choices"][0]["finish_reason"].as_str() {
                                    if !finish.is_empty() && finish != "null" {
                                        let tool_calls = if finish == "tool_calls" && !tool_calls_map.is_empty() {
                                            Some(tool_calls_map.iter().map(|(_, v)| ToolCall {
                                                id: v["id"].as_str().unwrap_or("").to_string(),
                                                name: v["function"]["name"].as_str().unwrap_or("").to_string(),
                                                arguments: serde_json::from_str(
                                                    v["function"]["arguments"].as_str().unwrap_or("{}")
                                                ).unwrap_or(json!({})),
                                            }).collect())
                                        } else {
                                            None
                                        };

                                        items.push(Ok(StreamPayload {
                                            content: None,
                                            tool_calls,
                                            finish_reason: Some(finish.to_string()),
                                        }));
                                    }
                                }
                            }
                        }
                    }

                    items
                }
                Err(e) => vec![Err(AppError::Http(e))],
            };

            futures::stream::iter(items)
        });

        Ok(Box::pin(parsed))
    }
}
