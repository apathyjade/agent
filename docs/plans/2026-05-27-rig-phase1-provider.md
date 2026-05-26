# Phase 1: Rig Provider 层适配 — 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

**Goal:** 将 Rig AI 框架的 provider 适配到现有的 `LLMProvider` trait，实现并行注册和运行时切换。

**Architecture:** 新增 `api/rig.rs` 包含 `RigOpenAIProvider` 和 `RigAnthropicProvider`，实现 `LLMProvider`。`ModelConfig` 增加 `backend` 字段 (`Native`/`Rig`)，`ProviderRegistry` 增加 `rig` HashMap 存储 Rig provider 实例。现有 Native provider 完全不受影响。

**Tech Stack:** Rust, Rig v0.37, async-trait, futures, tokio

---

### Task 1: 添加 `rig` 依赖和模块声明

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/api/mod.rs`

- [ ] **Step 1: 在 Cargo.toml 的 `[dependencies]` 中添加 rig**

```toml
# Rig AI framework — provider adapter (Phase 1)
rig = { version = "0.37", default-features = false, features = ["openai", "anthropic"] }
```

放在 `sha2` 之后、`[dev-dependencies]` 之前。

- [ ] **Step 2: 在 `api/mod.rs` 注册 rig 子模块**

```rust
pub mod anthropic;
pub mod openai;
pub mod provider;
pub mod rig;       // ← 新增
pub mod types;
```

- [ ] **Step 3: 验证编译（此时 `api/rig.rs` 还不存在，会报错，确认错误信息）**

Run: `cd src-tauri && cargo check 2>&1`
Expected: error[E0583] — file not found for module `rig`

---

### Task 2: 添加 `BackendKind` 枚举和 `ModelConfig.backend` 字段

**Files:**
- Modify: `src-tauri/src/config.rs`

- [ ] **Step 1: 在 `config.rs` 中 `ModelProvider` 枚举之后添加 `BackendKind`**

```rust
/// Runtime backend selection for model providers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum BackendKind {
    Native,
    Rig,
}

impl Default for BackendKind {
    fn default() -> Self {
        Self::Native
    }
}
```

- [ ] **Step 2: 在 `ModelConfig` 中添加 `backend` 字段**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub provider: ModelProvider,
    pub api_key: String,
    pub base_url: Option<String>,
    pub is_default: bool,
    pub enabled: bool,
    pub context_window: Option<u32>,
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub backend: BackendKind,        // ← 新增，默认 Native
}
```

- [ ] **Step 3: 在 `ModelConfig::default()` 的两个默认值中添加 `backend: BackendKind::Native`**

在 `config.rs` 的 `impl Default for AppConfig` 中，两个 `ModelConfig` 字面量分别添加：

```rust
// 第一个 (openai):
ModelConfig {
    // ... 现有字段 ...
    max_tokens: Some(128_000),
    backend: BackendKind::Native,  // ← 新增
},

// 第二个 (ollama):
ModelConfig {
    // ... 现有字段 ...
    max_tokens: Some(32768),
    backend: BackendKind::Native,  // ← 新增
},
```

- [ ] **Step 4: 验证编译**

Run: `cd src-tauri && cargo check 2>&1`
Expected: 编译通过，或者仅有与 `api/rig.rs` 相关的错误（Task 3 修复）

---

### Task 3: 创建 `api/rig.rs` — Rig provider 适配器

**Files:**
- Create: `src-tauri/src/api/rig.rs`

这是最核心的任务。适配器将 Rig 的 provider 包装为当前的 `LLMProvider` trait。

- [ ] **Step 1: 创建 `api/rig.rs`，编写基础结构和类型转换助手**

```rust
//! Rig AI framework provider adapters.
//!
//! Wraps Rig's OpenAI and Anthropic providers behind the existing `LLMProvider` trait,
//! allowing runtime backend switching via `ModelConfig.backend`.
//!
//! # Message Mapping
//!
//! | ChatRequest (current)    | Rig                        |
//! |--------------------------|----------------------------|
//! | MessageRole::System      | Agent preamble             |
//! | MessageRole::User        | Message { role: User }     |
//! | MessageRole::Assistant   | Message { role: Assistant }|
//! | MessageRole::Tool        | Message { role: Tool }     |

use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt};
use std::sync::Arc;

use rig::completion::{Completion, CompletionRequest, CompletionResponse, Message, MessageRole, ToolCall};
use rig::providers;

use crate::api::provider::LLMProvider;
use crate::api::types::{ChatRequest, ChatResponse, Choice, StreamPayload, Usage};
use crate::config::ModelConfig;
use crate::error::{AppError, Result};

/// Map `MessageRole` from our types to Rig's `MessageRole`.
fn to_rig_role(role: &crate::api::types::MessageRole) -> MessageRole {
    match role {
        crate::api::types::MessageRole::System => MessageRole::System,
        crate::api::types::MessageRole::User => MessageRole::User,
        crate::api::types::MessageRole::Assistant => MessageRole::Assistant,
        crate::api::types::MessageRole::Tool => MessageRole::Tool,
    }
}

/// Map Rig's `Message` back to our types (for tool_calls within response).
fn from_rig_tool_calls(tcs: &[ToolCall]) -> Vec<crate::api::types::ToolCall> {
    tcs.iter().map(|tc| crate::api::types::ToolCall {
        id: tc.id.clone(),
        name: tc.function.name.clone(),
        arguments: serde_json::from_str(&tc.function.arguments).unwrap_or_default(),
    }).collect()
}

/// Convert `ChatRequest` messages to Rig `Message` list.
/// System messages are excluded (handled separately as preamble).
fn build_rig_messages(request: &ChatRequest) -> Vec<Message> {
    request.messages.iter()
        .filter(|m| m.role != crate::api::types::MessageRole::System)
        .map(|m| Message {
            role: to_rig_role(&m.role),
            content: m.content.clone(),
            tool_calls: m.tool_calls.as_ref().map(|tcs| tcs.iter().map(|tc| ToolCall {
                id: tc.id.clone(),
                function: rig::completion::FunctionCall {
                    name: tc.name.clone(),
                    arguments: tc.arguments.to_string(),
                },
            }).collect()),
            tool_call_id: m.tool_call_id.clone(),
        })
        .collect()
}

/// Extract system prompt from messages (first System role message).
fn extract_system_prompt(request: &ChatRequest) -> String {
    request.messages.iter()
        .find(|m| m.role == crate::api::types::MessageRole::System)
        .map(|m| m.content.clone())
        .unwrap_or_default()
}

/// Build a minimal `ChatResponse` from Rig's `CompletionResponse`.
fn build_chat_response(rig_response: CompletionResponse) -> ChatResponse {
    let content = rig_response.choices.first()
        .and_then(|c| c.message.content.as_deref())
        .unwrap_or("")
        .to_string();

    let tool_calls = rig_response.choices.first()
        .and_then(|c| c.message.tool_calls.as_ref())
        .map(|tcs| from_rig_tool_calls(tcs));

    ChatResponse {
        id: rig_response.id,
        choices: vec![Choice {
            message: crate::api::types::Message {
                id: None,
                role: crate::api::types::MessageRole::Assistant,
                content,
                tool_calls,
                tool_call_id: None,
            },
            finish_reason: rig_response.choices.first()
                .and_then(|c| c.finish_reason.clone()),
        }],
        usage: rig_response.usage.map(|u| Usage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
        }),
    }
}
```

- [ ] **Step 2: 实现 `RigOpenAIProvider`**

在同一个文件 (`api/rig.rs`) 中追加：

```rust
// ----- OpenAI -----

pub struct RigOpenAIProvider {
    client: providers::openai::Client,
    model: String,
}

impl RigOpenAIProvider {
    pub fn new(api_key: String, model: String) -> Self {
        let client = providers::openai::Client::new(&api_key);
        Self { client, model }
    }
}

#[async_trait]
impl LLMProvider for RigOpenAIProvider {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse> {
        let preamble = extract_system_prompt(&request);
        let messages = build_rig_messages(&request);

        let agent = self.client
            .agent(&self.model)
            .preamble(&preamble)
            .temperature(request.temperature.unwrap_or(0.7))
            .max_tokens(request.max_tokens.unwrap_or(4096) as u64)
            .build();

        let completion_request = CompletionRequest {
            messages,
            ..Default::default()
        };

        let response = agent
            .completion(completion_request)
            .await
            .map_err(|e| AppError::Provider(format!("Rig OpenAI error: {}", e)))?;

        Ok(build_chat_response(response))
    }

    async fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> Result<BoxStream<'static, Result<StreamPayload>>> {
        // Phase 1: fall back to non-streaming, wrap in a single-item stream.
        // Full streaming support will be added in a follow-up iteration.
        let result = self.chat(request).await?;
        let stream = futures::stream::once(async move {
            Ok(StreamPayload {
                content: result.choices.first().map(|c| c.message.content.clone()),
                tool_calls: result.choices.first()
                    .and_then(|c| c.message.tool_calls.clone()),
                finish_reason: result.choices.first()
                    .and_then(|c| c.finish_reason.clone()),
            })
        });
        Ok(Box::pin(stream))
    }
}
```

- [ ] **Step 3: 实现 `RigAnthropicProvider`**

在同一个文件 (`api/rig.rs`) 中追加：

```rust
// ----- Anthropic -----

pub struct RigAnthropicProvider {
    client: providers::anthropic::Client,
    model: String,
}

impl RigAnthropicProvider {
    pub fn new(api_key: String, model: String) -> Self {
        let client = providers::anthropic::Client::new(&api_key);
        Self { client, model }
    }
}

#[async_trait]
impl LLMProvider for RigAnthropicProvider {
    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse> {
        let preamble = extract_system_prompt(&request);
        let messages = build_rig_messages(&request);

        let agent = self.client
            .agent(&self.model)
            .preamble(&preamble)
            .temperature(request.temperature.unwrap_or(0.7))
            .max_tokens(request.max_tokens.unwrap_or(4096) as u64)
            .build();

        let completion_request = CompletionRequest {
            messages,
            ..Default::default()
        };

        let response = agent
            .completion(completion_request)
            .await
            .map_err(|e| AppError::Provider(format!("Rig Anthropic error: {}", e)))?;

        Ok(build_chat_response(response))
    }

    async fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> Result<BoxStream<'static, Result<StreamPayload>>> {
        // Same fallback as OpenAI for Phase 1
        let result = self.chat(request).await?;
        let stream = futures::stream::once(async move {
            Ok(StreamPayload {
                content: result.choices.first().map(|c| c.message.content.clone()),
                tool_calls: result.choices.first()
                    .and_then(|c| c.message.tool_calls.clone()),
                finish_reason: result.choices.first()
                    .and_then(|c| c.finish_reason.clone()),
            })
        });
        Ok(Box::pin(stream))
    }
}
```

- [ ] **Step 4: 验证编译**

Run: `cd src-tauri && cargo check 2>&1`
Expected: 编译通过（如果 Rig API 不完全匹配，根据编译器错误调整上方的 Rig 调用代码）

---

### Task 4: 修改 `ProviderRegistry` 支持 Rig backend

**Files:**
- Modify: `src-tauri/src/api/provider.rs`

- [ ] **Step 1: 在 `provider.rs` 开头添加 import**

```rust
use super::rig::{RigOpenAIProvider, RigAnthropicProvider};
use crate::config::BackendKind;
```

- [ ] **Step 2: 在 `ProviderRegistry` 结构体中添加 `rig` HashMap**

```rust
pub struct ProviderRegistry {
    openai_compatible: HashMap<String, Arc<OpenAIProvider>>,
    anthropic: HashMap<String, Arc<AnthropicProvider>>,
    rig: HashMap<String, Arc<dyn LLMProvider>>,     // ← 新增
    default_model_id: Option<String>,
}
```

- [ ] **Step 3: 修改 `ProviderRegistry::new()` 增加 Rig 分支**

在 `for model in &config.models` 循环中，在 resolve API key 之后，增加 backend 判断：

```rust
pub fn new(config: &AppConfig) -> Self {
    let mut openai_compatible = HashMap::new();
    let mut anthropic = HashMap::new();
    let mut rig = HashMap::new();  // ← 新增

    for model in &config.models {
        if !model.enabled {
            continue;
        }

        let resolved_key = keychain::resolve_api_key(&model.id, &model.api_key);
        let has_key = !resolved_key.is_empty();
        let needs_key = Self::requires_api_key(&model.provider);

        // -- Rig backend branch --
        if model.backend == BackendKind::Rig {
            if !has_key && needs_key {
                continue;  // skip, no API key
            }
            let provider: Arc<dyn LLMProvider> = match model.provider {
                ModelProvider::OpenAI => {
                    Arc::new(RigOpenAIProvider::new(resolved_key, model.name.clone()))
                }
                ModelProvider::Anthropic => {
                    Arc::new(RigAnthropicProvider::new(resolved_key, model.name.clone()))
                }
                // For other providers, fall through to native
                _ if model.is_compatible_with_openai_api() && needs_key == has_key => {
                    // Use Rig OpenAI provider as generic OpenAI-compatible
                    Arc::new(RigOpenAIProvider::new(resolved_key, model.name.clone()))
                }
                _ => continue,
            };
            rig.insert(model.id.clone(), provider);
            continue;
        }

        // -- Native backend (existing logic below) --
        if model.is_compatible_with_openai_api() && (!needs_key || has_key) {
            // ... existing code unchanged ...
        } else if matches!(model.provider, ModelProvider::Anthropic) && has_key {
            // ... existing code unchanged ...
        }
    }

    let default_id = config.get_default_model().map(|m| m.id.clone());

    Self {
        openai_compatible,
        anthropic,
        rig,  // ← 新增
        default_model_id: default_id,
    }
}
```

- [ ] **Step 4: 更新 `get()` 方法**

```rust
pub fn get(&self, model_id: &str) -> Result<Arc<dyn LLMProvider>> {
    if let Some(provider) = self.openai_compatible.get(model_id) {
        return Ok(provider.clone());
    }
    if let Some(provider) = self.anthropic.get(model_id) {
        return Ok(provider.clone());
    }
    if let Some(provider) = self.rig.get(model_id) {           // ← 新增
        return Ok(provider.clone());
    }
    Err(AppError::Provider(format!(
        "Model '{}' not found or not configured",
        model_id
    )))
}
```

- [ ] **Step 5: 更新 `list_models()`, `get_registered_model_ids()`, `remove_model()`, `is_registered()` 方法**

```rust
pub fn list_models(&self) -> Vec<String> {
    let mut models = Vec::new();
    models.extend(self.openai_compatible.keys().cloned());
    models.extend(self.anthropic.keys().cloned());
    models.extend(self.rig.keys().cloned());                    // ← 新增
    models
}

pub fn remove_model(&mut self, model_id: &str) {
    self.openai_compatible.remove(model_id);
    self.anthropic.remove(model_id);
    self.rig.remove(model_id);                                  // ← 新增
}

pub fn is_registered(&self, model_id: &str) -> bool {
    self.openai_compatible.contains_key(model_id)
        || self.anthropic.contains_key(model_id)
        || self.rig.contains_key(model_id)                      // ← 新增
}

pub fn get_registered_model_ids(&self) -> Vec<String> {
    let mut ids: Vec<String> = self.openai_compatible.keys().cloned().collect();
    ids.extend(self.anthropic.keys().cloned());
    ids.extend(self.rig.keys().cloned());                       // ← 新增
    ids
}
```

- [ ] **Step 6: 更新 `add_model()` 方法**

```rust
pub fn add_model(&mut self, model: ModelConfig) {
    if !model.enabled {
        return;
    }

    let resolved_key = keychain::resolve_api_key(&model.id, &model.api_key);
    let has_key = !resolved_key.is_empty();
    let needs_key = Self::requires_api_key(&model.provider);

    // Rig backend
    if model.backend == BackendKind::Rig {
        if !has_key && needs_key {
            return;
        }
        let provider: Arc<dyn LLMProvider> = match model.provider {
            ModelProvider::OpenAI => {
                Arc::new(RigOpenAIProvider::new(resolved_key, model.name.clone()))
            }
            ModelProvider::Anthropic => {
                Arc::new(RigAnthropicProvider::new(resolved_key, model.name.clone()))
            }
            _ if model.is_compatible_with_openai_api() && needs_key == has_key => {
                Arc::new(RigOpenAIProvider::new(resolved_key, model.name.clone()))
            }
            _ => return,
        };
        self.rig.insert(model.id.clone(), provider);
        return;
    }

    // Native backend (existing logic)
    if model.is_compatible_with_openai_api() && (!needs_key || has_key) {
        // ... existing code unchanged ...
    } else if matches!(model.provider, ModelProvider::Anthropic) && has_key {
        // ... existing code unchanged ...
    }
}
```

- [ ] **Step 7: 验证编译**

Run: `cd src-tauri && cargo check 2>&1`
Expected: 编译通过，0 errors

---

### Task 5: 编译检查和诊断

**Files:**
- Verify: `src-tauri/src/`

- [ ] **Step 1: 完整编译检查**

Run: `cd src-tauri && cargo check 2>&1`
Expected: `Checking agent_lib v0.1.0` → `Finished` 无错误

- [ ] **Step 2: 运行现有测试，确保无损**

Run: `cd src-tauri && cargo test 2>&1`
Expected: 现有测试全部通过（calculator、runtime 等不受影响）

- [ ] **Step 3: LSP diagnostics 检查**

Run: `lsp_diagnostics` on `src-tauri/src/api/rig.rs`
Expected: 无错误、无 warning

---

### Task 6: 手动验证（可选，需 API key）

此任务需要有效的 OpenAI/Anthropic API key，仅在条件满足时执行。

- [ ] **Step 1: 验证 Rig backend 切换**

1. 在 `config.json` 中设置一个 model 的 `"backend": "rig"`
2. 启动应用
3. 使用该 model 发送一条消息
4. 确认收到正常回复

- [ ] **Step 2: 验证 Native backend 不受影响**

1. 保持另一个 model 的 `"backend": "native"`
2. 切换回 native model
3. 发送相同消息
4. 确认表现和之前一致

---

### Task 7: 提交

- [ ] **Step 1: 提交 Phase 1 变更**

```bash
git add src-tauri/Cargo.toml src-tauri/src/api/mod.rs src-tauri/src/config.rs src-tauri/src/api/provider.rs src-tauri/src/api/rig.rs
git commit -m "feat: add Rig AI framework provider adapter (Phase 1)

- Add rig v0.37 dependency with openai/anthropic features
- Create RigOpenAIProvider and RigAnthropicProvider implementing LLMProvider
- Add BackendKind enum and ModelConfig.backend field (default: Native)
- Extend ProviderRegistry with rig HashMap for Rig provider storage
- Config-to-Rig message mapping with full message history support

Backward compatible: existing configs without backend field default to Native.
Phase 2 (memory/RAG) will follow after this is verified.
" 
```
