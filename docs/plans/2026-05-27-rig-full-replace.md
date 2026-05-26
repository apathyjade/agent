# Phase 1b: 完全替换 Native Provider 为 Rig

> **Goal:** 删除 `BackendKind` 选择机制和 Native provider 实现，所有 LLM 通信统一走 Rig。

**Scope:**
- 移除 `BackendKind` 枚举和 `ModelConfig.backend` 字段
- `ProviderRegistry` 简化为单一 `HashMap<String, Arc<dyn LLMProvider>>`
- `RigProvider` 扩展为支持全部 11 种 provider 类型 + 自定义 base_url
- 添加真正的流式传输支持（`StreamingChat`）
- 删除 `openai.rs` 和 `anthropic.rs` 的 Native 实现

**Provider 映射表：**

| Config provider | Rig provider | 构造方式 |
|----------------|-------------|---------|
| OpenAI | `rig::providers::openai` | `Client::new(&api_key)` |
| Anthropic | `rig::providers::anthropic` | `Client::new(&api_key)` |
| Google | `rig::providers::gemini` | `Client::new(&api_key)` |
| Groq | `rig::providers::groq` | `Client::new(&api_key)` |
| DeepSeek | `rig::providers::deepseek` | `Client::new(&api_key)` |
| Ollama | `rig::providers::ollama` | `Client::new()` |
| Moonshot | `rig::providers::moonshot` | `Client::new(&api_key)` |
| Zhipu | `rig::providers::openai` (兼容) | `Client::new_with_url(&api_key, base_url)` |
| SiliconFlow | `rig::providers::openai` (兼容) | `Client::new_with_url(&api_key, base_url)` |
| LMStudio | `rig::providers::openai` (兼容) | `Client::new_with_url("", base_url)` |
| Custom | `rig::providers::openai` (兼容) | `Client::new_with_url(&api_key, base_url)` |

---

### Task 1: 移除 BackendKind，简化 ProviderRegistry

**Files:** `config.rs`, `commands/model.rs`, `commands_provider.rs`, `provider.rs`

- 删除 `BackendKind` 枚举
- 删除 `ModelConfig.backend` 字段和 `#[serde(default)]`
- 删除 `commands/model.rs` 和 `commands_provider.rs` 中的 `backend: BackendKind::Native`
- `ProviderRegistry` 改为单一 `providers: HashMap<String, Arc<dyn LLMProvider>>`

### Task 2: 重写 RigProvider 为工厂模式 + 流式传输

**Files:** `api/rig.rs`

- 移除 `BackendKind` 相关逻辑
- 添加 `RigProvider::from_config(model: &ModelConfig) -> Result<Self>` 工厂方法
- 所有 11 种 provider 映射到对应的 Rig Client
- 用 `StreamingChat` trait 实现真实的流式传输

### Task 3: 清理 Legacy 代码

**Files:** 删除 `api/openai.rs`, `api/anthropic.rs`（或标记为 deprecated）

### Task 4: 编译验证 + 测试

### Task 5: 提交
