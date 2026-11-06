//! Rig AI 框架 —— 统一的 LLM 供应商接入。
//!
//! 提供 [`RigProvider`] 包装器，把任意 Rig completion model
//! （[`DynModel<Completion>`]）适配到本项目的 [`ProviderBox`] trait；同时提供
//! [`create_rig_provider`] 工厂函数。
//!
//! # 供应商映射
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
//! \* 带 \* 的供应商使用 Rig 的 OpenAI client 配自定义 `base_url`；代码里把它们
//!   标记为 "OpenAI-compatible"。
//!
//! # Rig 0.45 的形态
//!
//! 供应商 client 不再直接产出 agent。client 先构建一个 *model*
//! （`client.chat(id)`、`client.completion(id)`……），它被擦除成
//! [`DynModel<Completion>`] 后保存；agent 则在每次请求时用
//! [`rig::agent::AgentBuilder`] 构建。

use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::StreamExt;
use serde::{Deserialize, Serialize};

use rig::agent::{AgentBuilder, MultiTurnStreamItem, StreamingResult};
use rig::completion::Message;
use rig::driver::DynModel;
use rig::operation::Completion;
use rig::streaming::{Item, StreamEvent};

use crate::agent::tools::policy::{shared_state, InvalidCallHook, SharedPolicyState, ToolPolicyHook};
use crate::core::error::{AppError, Result};
use crate::llm::model::{ModelConfig, ModelProvider};
use crate::llm::provider::{ApprovalGate, ProviderBox, ToolRuntime, TurnOutcome};

// ---------------------------------------------------------------------------
// RigProvider —— 对 Rig completion model 的包装
// ---------------------------------------------------------------------------

/// 由 Rig [`DynModel<Completion>`] 驱动的 LLM 供应商。
///
/// 模型在配置加载时只做一次类型擦除，因此不同后端（OpenAI、Anthropic……）的
/// 供应商共享同一种表示，可以一起放进
/// [`ProviderRegistry`](crate::llm::provider::ProviderRegistry)。
pub struct RigProvider {
    model: DynModel<Completion>,
    model_id: String,
    /// 人工确认通道；每个 agent 构造时挂进 `ToolPolicyHook`。
    approval: Option<Arc<dyn ApprovalGate>>,
}

impl RigProvider {
    /// 用已擦除的模型与模型 id 构造包装器。
    pub fn new(model: impl Into<DynModel<Completion>>, model_id: String) -> Self {
        Self::new_with_approval(model, model_id, None)
    }

    /// 构造包装器并注入人工确认通道。
    pub fn new_with_approval(
        model: impl Into<DynModel<Completion>>,
        model_id: String,
        approval: Option<Arc<dyn ApprovalGate>>,
    ) -> Self {
        Self {
            model: model.into(),
            model_id,
            approval,
        }
    }

    fn agent(&self, preamble: &str) -> AgentBuilder {
        AgentBuilder::new(self.model.clone()).preamble(preamble)
    }

    fn agent_with_tools(&self, preamble: &str, runtime: ToolRuntime, state: SharedPolicyState) -> AgentBuilder {
        // hook 必须挂在 builder 上：审批与终态记录都发生在 Rig 的 dispatch
        // 边界，挂到 runner 上会漏掉流式路径之外的重试。
        let builder = self
            .agent(preamble)
            .add_hook(ToolPolicyHook::new(
                state,
                runtime.approval.clone().or_else(|| self.approval.clone()),
                runtime.meta.clone(),
            ))
            .add_hook(InvalidCallHook);
        if runtime.tools.is_empty() {
            builder
        } else {
            builder.dynamic_tools(runtime.tools)
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
        tools: ToolRuntime,
        max_turns: usize,
    ) -> Result<TurnOutcome> {
        let state = shared_state();
        let agent = self
            .agent_with_tools(preamble, tools, state.clone())
            .build();

        // Rig 的 agent 内部驱动工具循环：每个被请求的工具调用都会经工具集分发
        // 并回填，直到模型停止。
        let response = agent
            .prompt(Message::user(prompt))
            .max_turns(max_turns)
            .tool_concurrency(TOOL_CONCURRENCY)
            .max_invalid_tool_call_retries(INVALID_CALL_RETRIES)
            .max_consecutive_malformed_tool_calls(MALFORMED_CALL_LIMIT)
            .run()
            .await
            .map_err(|e| AppError::Provider(format!("Rig agent error: {}", e)))?;

        Ok(turn_outcome(response, &state))
    }

    async fn chat_with_tools(
        &self,
        preamble: &str,
        prompt: &str,
        history: &[Message],
        tools: ToolRuntime,
        max_turns: usize,
    ) -> Result<TurnOutcome> {
        let state = shared_state();
        let agent = self
            .agent_with_tools(preamble, tools, state.clone())
            .build();

        let response = agent
            .prompt(Message::user(prompt))
            .max_turns(max_turns)
            .tool_concurrency(TOOL_CONCURRENCY)
            .max_invalid_tool_call_retries(INVALID_CALL_RETRIES)
            .max_consecutive_malformed_tool_calls(MALFORMED_CALL_LIMIT)
            .history(history.to_vec())
            .run()
            .await
            .map_err(|e| AppError::Provider(format!("Rig agent error: {}", e)))?;

        Ok(turn_outcome(response, &state))
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
        history: &[Message],
        tools: ToolRuntime,
        max_turns: usize,
    ) -> Result<StreamingResult> {
        let state = shared_state();
        let agent = self.agent_with_tools(preamble, tools, state).build();
        let runner = agent
            .prompt(Message::user(prompt))
            .max_turns(max_turns)
            .tool_concurrency(TOOL_CONCURRENCY)
            .max_invalid_tool_call_retries(INVALID_CALL_RETRIES)
            .max_consecutive_malformed_tool_calls(MALFORMED_CALL_LIMIT)
            .history(history.to_vec());
        Ok(runner.stream())
    }
}

/// 同时执行的工具调用上限。
const TOOL_CONCURRENCY: usize = 4;
/// 参数畸形时允许模型重试的次数。
const INVALID_CALL_RETRIES: usize = 2;
/// 连续多少轮参数畸形后放弃。
const MALFORMED_CALL_LIMIT: Option<usize> = Some(3);

/// 把 Rig 的响应与本轮策略状态合成 [`TurnOutcome`]。
pub(crate) fn turn_outcome(
    response: rig::run::PromptResponse,
    state: &SharedPolicyState,
) -> TurnOutcome {
    let decisions = state
        .lock()
        .map(|guard| guard.decisions())
        .unwrap_or_default();

    TurnOutcome {
        output: response.output(),
        messages: response.messages,
        decisions,
    }
}

/// 把 Rig 的 agent 流映射成项目里的纯文本流。
///
/// 只透出回答文本增量与最终响应；工具调用、推理与生命周期元素都渲染为空块，
/// 从而保持调用方原有的「只要文本」约定。
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
// 辅助：从 base URL 剥掉已知 API 路径后缀，避免 Rig 自己再拼一次
// （例如 "/chat/completions"）造成重复。
// ---------------------------------------------------------------------------

/// 配置里的 URL 可能以这些已知 API 路径结尾，而 Rig 会自己拼上它们。
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
// 工厂：按 ModelConfig 创建对应的 RigProvider
// ---------------------------------------------------------------------------

/// 为给定模型配置创建 [`RigProvider`] trait object。
///
/// 这是 [`ProviderRegistry`] 使用的唯一入口。
///
/// [`ProviderRegistry`]: crate::llm::provider::ProviderRegistry
/// 为给定模型配置创建 [`RigProvider`] trait object（不启用人工确认）。
pub fn create_rig_provider(model: &ModelConfig) -> Result<Box<dyn ProviderBox>> {
    create_rig_provider_with_approval(model, None)
}

/// 为给定模型配置创建 [`RigProvider`] trait object，并注入人工确认通道。
pub fn create_rig_provider_with_approval(
    model: &ModelConfig,
    approval: Option<Arc<dyn ApprovalGate>>,
) -> Result<Box<dyn ProviderBox>> {
    let model_id = model.name.clone();

    match model.provider {
        ModelProvider::OpenAI => {
            let client = rig::providers::openai::OpenAI::new(model.api_key.clone());
            Ok(Box::new(RigProvider::new_with_approval(
                client.chat(model_id.clone()),
                model.name.clone(),
                approval.clone(),
            )))
        }

        ModelProvider::Anthropic => {
            let client = rig::providers::anthropic::Anthropic::new(model.api_key.clone());
            Ok(Box::new(RigProvider::new_with_approval(
                client.completion(model_id.clone()),
                model.name.clone(),
                approval.clone(),
            )))
        }

        ModelProvider::Google => {
            let client = rig::providers::gemini::Gemini::new(model.api_key.clone());
            Ok(Box::new(RigProvider::new_with_approval(
                client.completion(model_id.clone()),
                model.name.clone(),
                approval.clone(),
            )))
        }

        ModelProvider::Groq => {
            let client = rig::providers::groq::new(model.api_key.clone());
            Ok(Box::new(RigProvider::new_with_approval(
                client.chat(model_id.clone()),
                model.name.clone(),
                approval.clone(),
            )))
        }

        ModelProvider::DeepSeek => {
            let client = rig::providers::deepseek::new(model.api_key.clone());
            Ok(Box::new(RigProvider::new_with_approval(
                client.chat(model_id.clone()),
                model.name.clone(),
                approval.clone(),
            )))
        }

        ModelProvider::Ollama => {
            // Rig 的 Ollama client 指向 daemon 根地址并自行拼 API 路径，因此先
            // 把配置里的 OpenAI 风格 URL 削到 origin。
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
            Ok(Box::new(RigProvider::new_with_approval(
                client.completion(model_id.clone()),
                model.name.clone(),
                approval.clone(),
            )))
        }

        ModelProvider::Moonshot => {
            let client = rig::providers::moonshot::new(model.api_key.clone());
            Ok(Box::new(RigProvider::new_with_approval(
                client.chat(model_id.clone()),
                model.name.clone(),
                approval.clone(),
            )))
        }

        // OpenAI 兼容供应商：Zhipu、SiliconFlow、LMStudio、Custom
        //
        // 这些用 Rig 的 OpenAI **Chat Completions** client（端点
        // `/chat/completions`），而不是更新的 Responses API，因为第三方供应商
        // 实现的就是前者。
        //
        // 用户配置的自定义 `base_url` 往往已包含完整路径（例如
        // ".../v1/chat/completions"）。由于 Rig 会自己再拼一次路径，这里先剥掉
        // 已知后缀。
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
            Ok(Box::new(RigProvider::new_with_approval(
                client.chat(model_id),
                model.name.clone(),
                approval.clone(),
            )))
        }
    }
}

// ---------------------------------------------------------------------------
// 结构化抽取
// ---------------------------------------------------------------------------

/// 用 Rig 的 [`Extractor`] 从文本中抽取结构化数据。
///
/// 泛型参数 `T` 是目标结构体，必须派生 [`Serialize`]、[`Deserialize`] 与
/// [`schemars::JsonSchema`]。
///
/// 意图分类器与执行规划器用它替代易碎的手写 JSON 解析。
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

    // extractor 没有独立的 preamble 方法，因此把系统提示与用户输入拼在一起。
    let combined = format!("{}\n\n{}", system_prompt, user_input);

    let extractor = rig::extractor::ExtractorBuilder::<T>::new(client.chat(model)).build();

    let response = extractor
        .extract(combined)
        .await
        .map_err(|e| AppError::Provider(format!("Extraction failed: {}", e)))?;

    Ok(response.output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::tools::policy::shared_state;
    use crate::llm::provider::ToolCallStatus;
    use rig::completion::Message;
    use rig::run::PromptResponse;

    #[test]
    fn turn_outcome_keeps_transcript_and_decisions() {
        let state = shared_state();
        {
            let mut guard = state.lock().expect("策略状态不应中毒");
            guard.set("call_1", ToolCallStatus::Completed);
            guard.set("call_2", ToolCallStatus::Failed);
        }

        let response = PromptResponse::new("done", Default::default()).with_messages(vec![
            Message::assistant("calling calculator"),
            Message::user("tool output"),
            Message::assistant("done"),
        ]);

        let outcome = turn_outcome(response, &state);

        assert_eq!(outcome.output, "done");
        assert_eq!(outcome.messages.len(), 3, "transcript 应原样保留");
        assert_eq!(
            outcome.decisions.get("call_1"),
            Some(&ToolCallStatus::Completed)
        );
        assert_eq!(outcome.decisions.get("call_2"), Some(&ToolCallStatus::Failed));
    }

    #[test]
    fn turn_outcome_without_tools_has_no_decisions() {
        let state = shared_state();
        let response = PromptResponse::new("plain answer", Default::default());

        let outcome = turn_outcome(response, &state);

        assert_eq!(outcome.output, "plain answer");
        assert!(outcome.messages.is_empty());
        assert!(outcome.decisions.is_empty());
    }
}


