# Rig AI Framework 渐进式集成设计

> **状态**: 设计稿 v1
> **日期**: 2026-05-27
> **影响范围**: `src-tauri/src/api/`, `src-tauri/src/memory/`
> **里程碑**: Phase 1 (Provider 层) → Phase 2 (记忆/RAG) → Phase 3 (结构化输出) → Phase 4 (Agent 循环)

---

## 1. 背景与目标

### 1.1 当前状态

项目是一个 Tauri 桌面 AI 客户端，拥有自研的 AI 基础设施：

- **Provider 层**: 自定义 `LLMProvider` trait（`chat` + `chat_stream`），仅 2 个实现（OpenAI-compatible + Anthropic Messages API）
- **记忆系统**: SQLite `LIKE` 关键词搜索，无法理解语义
- **结构化输出**: 手动 `serde_json::from_str` 解析 LLM 响应，脆弱且无编译期检查
- **Agent 循环**: 自研最多 10 次迭代 + 指数退避重试

### 1.2 Rig 框架概况

[Rig](https://rig.rs/) v0.37.0 是一个 Rust LLM 应用框架，提供：

- 20+ 模型 provider（OpenAI、Anthropic、Gemini、Cohere、Ollama 等）
- 10+ 向量存储集成（MongoDB、Qdrant、LanceDB 等）
- Agent 系统（带工具调用、RAG、上下文管理）
- 结构化输出提取（`extract!` 宏）
- 流式传输、遥测、WASM 兼容

### 1.3 核心约束

| 约束 | 说明 |
|------|------|
| **渐进式** | 每阶段独立可用，不阻塞其他工作 |
| **零破坏** | 现有代码零改动，新旧并行运行 |
| **加法替代替换** | Rig 代码作为可选项加入，非强制迁移 |
| **向后兼容** | 现有配置无感升级，默认走 `Native` |

---

## 2. 总体架构

```
┌─────────────────────────────────────────────────────────┐
│                      AppState                            │
│                                                          │
│  ┌─────────────────────────────────────────────────┐     │
│  │              ProviderRegistry                    │     │
│  │                                                   │     │
│  │  ┌─────────────────┐  ┌──────────────────────┐  │     │
│  │  │  Native Providers │  │    Rig Providers     │  │     │
│  │  │                   │  │                      │  │     │
│  │  │  OpenAIProvider   │  │  RigOpenAIProvider   │  │     │
│  │  │  AnthropicProvider│  │  RigAnthropicProvider│  │     │
│  │  │                   │  │  RigCohereProvider   │  │     │
│  │  │                   │  │  RigGeminiProvider   │  │     │
│  │  │                   │  │  ...(future)         │  │     │
│  │  └─────────────────┘  └──────────────────────┘  │     │
│  └─────────────────────────────────────────────────┘     │
│                                                          │
│  ┌─────────────────────────────────────────────────┐     │
│  │              MemoryManager                       │     │
│  │  ┌──────────────┐  ┌─────────────────────────┐  │     │
│  │  │  SQLite (like)│  │  Vector Index (cosine)  │  │     │
│  │  │  (fallback)   │  │  (semantic, optional)   │  │     │
│  │  └──────────────┘  └─────────────────────────┘  │     │
│  └─────────────────────────────────────────────────┘     │
└─────────────────────────────────────────────────────────┘
```

### 2.1 数据流变迁

**Phase 1 后 - Provider 请求：**
```
send_message → AgentLoop → ProviderRegistry → NativeProvider 或 RigProvider
                                                   ↑
                                              backend: "rig"
```

**Phase 2 后 - 记忆检索：**
```
send_message → MemoryManager → VectorIndex (如有 embedder) 或 SQLite LIKE
                                     ↑
                                 语义搜索
```

---

## 3. Phase 1: Provider 层适配

### 3.1 目标

在保留现有 provider 实现的前提下，将 Rig 的 provider 适配到 `LLMProvider` trait，实现并行注册和运行时切换。

### 3.2 变更清单

| 文件 | 操作 | 说明 |
|------|------|------|
| `Cargo.toml` | 修改 | 添加 `rig` 依赖 |
| `api/mod.rs` | 修改 | 添加 `rig` 子模块声明 |
| `api/rig.rs` | **新建** | Rig provider 适配器实现 |
| `api/provider.rs` | 修改 | `ProviderRegistry::new()` 增加 Rig 分支 |
| `config.rs` | 修改 | `ModelConfig` 增加 `BackendKind::Rig` |

### 3.3 `api/rig.rs` 适配器

```rust
//! Rig provider 适配器
//!
//! 将 Rig 的 provider 包装为当前的 LLMProvider trait，
//! 允许通过 backend 配置选择 Rig 实现。
//!
//! # 支持列表
//!
//! | Rig Provider        | 环境变量              | 对应模型                      |
//! |---------------------|-----------------------|-------------------------------|
//! | OpenAI              | OPENAI_API_KEY        | gpt-4o, gpt-4o-mini, o3      |
//! | Anthropic           | ANTHROPIC_API_KEY     | claude-sonnet-4, claude-haiku |
//! | Cohere (future)     | CO_API_KEY            | command-r+                    |
//! | Gemini (future)     | GEMINI_API_KEY        | gemini-2.0-flash              |

use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;
use serde_json::Value;

use rig::{
    completion::{Completion, Prompt},
    providers::{self, ProviderFactory},
};

use crate::api::{
    provider::LLMProvider,
    types::{ChatRequest, ChatResponse, StreamPayload, ToolCall, Message},
};
use crate::error::{AppError, Result};

// ----- Rig OpenAI Provider -----

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
        let agent = self.client
            .agent(&self.model)
            .preamble(&request.system_prompt)
            .temperature(request.temperature.unwrap_or(0.7))
            .build();

        let response = agent
            .prompt(&request.user_message)
            .await
            .map_err(|e| AppError::ProviderError(e.to_string()))?;

        Ok(ChatResponse {
            content: response,
            tool_calls: vec![],
            ..Default::default()
        })
    }

    async fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> Result<BoxStream<'static, Result<StreamPayload>>> {
        // TODO: Phase 1 先实现非流式，流式在后续迭代中补充
        let result = self.chat(request).await?;
        let stream = futures::stream::once(async move {
            Ok(StreamPayload {
                r#type: "text".into(),
                content: result.content,
                ..Default::default()
            })
        });
        Ok(Box::pin(stream))
    }
}

// ----- Rig Anthropic Provider -----

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
        let agent = self.client
            .agent(&self.model)
            .preamble(&request.system_prompt)
            .temperature(request.temperature.unwrap_or(0.7))
            .build();

        let response = agent
            .prompt(&request.user_message)
            .await
            .map_err(|e| AppError::ProviderError(e.to_string()))?;

        Ok(ChatResponse {
            content: response,
            tool_calls: vec![],
            ..Default::default()
        })
    }

    async fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> Result<BoxStream<'static, Result<StreamPayload>>> {
        let result = self.chat(request).await?;
        let stream = futures::stream::once(async move {
            Ok(StreamPayload {
                r#type: "text".into(),
                content: result.content,
                ..Default::default()
            })
        });
        Ok(Box::pin(stream))
    }
}
```

### 3.4 config.rs 变更

```rust
/// 配置中的后端选择
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub provider: String,
    pub model: String,
    pub api_key: Option<String>,

    #[serde(default)]
    pub backend: BackendKind,  // ← 新增
}
```

### 3.5 ProviderRegistry 变更

```rust
impl ProviderRegistry {
    pub fn new(config: &AppConfig) -> Self {
        let mut registry = HashMap::new();

        for mc in &config.models {
            let provider: Arc<dyn LLMProvider> = match mc.backend {
                BackendKind::Native => match mc.provider.as_str() {
                    "openai" => Arc::new(NativeOpenAIProvider::new(mc)),
                    "anthropic" => Arc::new(NativeAnthropicProvider::new(mc)),
                    other => return Err(unknown_provider(other)),
                },
                BackendKind::Rig => match mc.provider.as_str() {
                    "openai" => {
                        let key = mc.api_key.clone()
                            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
                            .unwrap_or_default();
                        Arc::new(RigOpenAIProvider::new(key, mc.model.clone()))
                    }
                    "anthropic" => {
                        let key = mc.api_key.clone()
                            .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
                            .unwrap_or_default();
                        Arc::new(RigAnthropicProvider::new(key, mc.model.clone()))
                    }
                    other => {
                        // Rig 支持但原生没有的 provider 自动使用 Rig
                        let key = mc.api_key.clone().unwrap_or_default();
                        Arc::new(RigOpenAIProvider::new(key, mc.model.clone()))
                    }
                },
            };
            registry.insert(mc.name.clone(), provider);
        }

        Self { registry }
    }
}
```

### 3.6 向后兼容

- 现有 `config.toml` 不包含 `backend` 字段 → serde 反序列化为 `BackendKind::Native`（Default）
- 用户升级到新版后，**行为无变化**
- 前端设置新增 `BackendKind` 选择器（默认 Native），切换后重新加载 provider

### 3.7 验证

- `cargo check` 通过
- 写单元测试：mock Rig provider → 验证 `LLMProvider` trait 实现
- 手动测试：启动应用 → 切换 backend → 发送消息 → 观察回复正常

---

## 4. Phase 2: 记忆/RAG 升级

### 4.1 目标

将记忆检索从关键词匹配（SQLite `LIKE`）升级为语义向量搜索，同时保持 SQLite 作为持久化存储和降级路径。

### 4.2 变更清单

| 文件 | 操作 | 说明 |
|------|------|------|
| `Cargo.toml` | 修改 | 启用 `rig` 的 embedding feature |
| `memory/mod.rs` | 修改 | 增加 embedder + vector_index |
| `memory/vector_index.rs` | **新建** | 内存向量索引实现 |
| `api/rig.rs` | 修改 | 增加 embedding model 创建函数 |

### 4.3 向量索引

```rust
// memory/vector_index.rs

/// 轻量内存向量索引
///
/// 存储 (text, embedding) 对，查询时计算余弦相似度。
/// 应用启动时从 SQLite 重建，不负责持久化。
pub struct InMemoryVectorIndex {
    entries: Vec<IndexEntry>,
}

struct IndexEntry {
    key: String,        // memory_id
    text: String,
    embedding: Vec<f32>,
}

impl InMemoryVectorIndex {
    pub fn new() -> Self {
        Self { entries: vec![] }
    }

    pub fn insert(&mut self, key: String, text: String, embedding: Vec<f32>) {
        self.entries.push(IndexEntry { key, text, embedding });
    }

    /// 余弦相似度 top-k 搜索
    pub fn search(&self, query_embedding: &[f32], k: usize) -> Vec<(String, f32)> {
        let mut scores: Vec<_> = self.entries
            .iter()
            .map(|e| {
                let sim = cosine_similarity(&e.embedding, query_embedding);
                (e.key.clone(), sim)
            })
            .collect();
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores.truncate(k);
        scores
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 { 0.0 } else { dot / (norm_a * norm_b) }
}
```

### 4.4 MemoryManager 变更

```rust
pub struct MemoryManager {
    db: Arc<Mutex<Database>>,
    embedder: Option<rig::providers::openai::EmbeddingModel>,  // ← 新增
    vector_index: Arc<Mutex<InMemoryVectorIndex>>,             // ← 新增
    max_memories: usize,
}

impl MemoryManager {
    /// 应用启动时重建向量索引
    pub async fn new(...) -> Self {
        let embedder = create_embedder_if_available();
        let vector_index = Arc::new(Mutex::new(InMemoryVectorIndex::new()));

        // 从 SQLite 批量重建
        if let Some(ref emb) = embedder {
            let memories = db.load_all_memories().await;
            Self::rebuild_index(emb, &vector_index, &memories).await;
        }

        Self { db, embedder, vector_index, max_memories }
    }

    /// 检索 —— 优先语义，降级到 LIKE
    pub async fn retrieve_relevant(&self, query: &str, limit: usize) -> Result<Vec<MemoryRecord>> {
        if let Some(ref embedder) = self.embedder {
            // 语义搜索分支
            let query_embedding = embedder
                .embed_text(query)
                .await
                .map_err(|e| AppError::EmbeddingError(e.to_string()))?;

            let index = self.vector_index.lock().await;
            let results = index.search(&query_embedding, limit);

            // 从 SQLite 按 key 取完整记录
            let mut records = Vec::with_capacity(results.len());
            for (key, score) in &results {
                if let Some(record) = self.db.get_memory_by_id(key).await? {
                    records.push(record);
                }
            }
            return Ok(records);
        }

        // 降级到 LIKE 搜索
        self.retrieve_fallback(query, limit).await
    }

    /// 存入记忆时同步计算 embedding
    pub async fn store_memory(&self, content: &str) -> Result<()> {
        let record = self.db.insert_memory(content).await?;

        if let Some(ref embedder) = self.embedder {
            let embedding = embedder.embed_text(content).await?;
            let mut index = self.vector_index.lock().await;
            index.insert(record.id.clone(), content.to_string(), embedding);
        }

        Ok(())
    }
}
```

### 4.5 Embedding Model 创建

```rust
// api/rig.rs 增加
use rig::providers::openai;

pub fn create_openai_embedder(api_key: &str) -> Result<openai::EmbeddingModel> {
    let client = openai::Client::new(api_key);
    let embedder = client.embedding_model(openai::TEXT_EMBEDDING_3_SMALL);
    Ok(embedder)
}
```

### 4.6 降级策略

| 条件 | 行为 |
|------|------|
| 无 API key | `embedder = None`，全程 LIKE |
| Embedding API 超时 | 单次捕获错误，回退 LIKE |
| 向量索引为空 | 等价于无结果，不报错 |
| API key 之后配置 | 下次启动重建索引 |

### 4.7 验证

- [ ] 无 API key 时记忆行为不变
- [ ] 有 API key 时语义搜索返回准确结果
- [ ] 启动时重建索引不超时（< 5s 对于 < 1000 条记忆）
- [ ] Embedding API 失败自动降级
- [ ] `cargo test` 通过

---

## 5. Phase 3: 结构化输出（概要）

### 5.1 目标

用 Rig 的 `Extract` 派生宏替换手动 JSON 解析，提升类型安全。

### 5.2 受影响模块

| 模块 | 当前 | Rig 方案 |
|------|------|---------|
| `intent/classifier.rs` | `serde_json::from_str` 解析 LLM JSON | `rig::extract::Extract` 派生宏 |
| `execution/planner.rs` | 同上 | 同上 |
| `lifecycle/summarizer.rs` | 同上 | 同上 |

### 5.3 示例

```rust
use rig::extract::Extract;

#[derive(Extract)]
pub struct IntentClassification {
    /// The primary intent of the user message
    pub intent: String,
    /// Confidence score 0.0-1.0
    pub confidence: f32,
}

// Rig 自动生成 JSON schema，LLM 输出自动校验和反序列化
let classification: IntentClassification = model
    .extract::<IntentClassification>("Classify this message...")
    .await?;
```

### 5.4 范围

Phase 3 是独立替换，不影响 Phase 1/2 的代码。建议在 Phase 1-2 稳定后开始。

---

## 6. Phase 4: Agent 循环增强（概要）

### 6.1 目标

在保留 `AgentLoop` 接口的前提下，引入 Rig 的 `Agent` 类型处理复杂的工具调用链。

### 6.2 挑战

- 当前 `AgentLoop` 使用 `Arc<dyn LLMProvider>`（运行时多态）
- Rig `Agent` 使用泛型 `<T: CompletionModel>`（编译期单态化）
- 需要一个 adapter 层桥接两种风格

### 6.3 方案

```
AgentLoop (当前接口不变)
    │
    ├── run()       → 走 NativeProvider (现有逻辑)
    │
    └── run_rig()   → 内部构造 Rig Agent，适配结果到当前 ChatResponse
```

```rust
impl AgentLoop {
    pub async fn run_rig(
        &self,
        request: ChatRequest,
        rig_model: impl rig::completion::CompletionModel + 'static,
    ) -> Result<ChatResponse> {
        let agent = rig::AgentBuilder::new(rig_model)
            .preamble(&request.system_prompt)
            .build();

        let response = agent.prompt(&request.user_message).await?;
        Ok(ChatResponse {
            content: response,
            ..Default::default()
        })
    }
}
```

Phase 4 为**可选阶段**，取决于 Phase 1-3 的效果评估。

---

## 7. 里程碑与时间估算

| 阶段 | 新增文件 | 修改文件 | 估算 | 风险 |
|------|---------|---------|------|------|
| **Phase 1** Provider 层 | 1 | 3 | 2-3 小时 | 低 |
| **Phase 2** 记忆/RAG | 2 | 2 | 3-4 小时 | 低 |
| **Phase 3** 结构化输出 | 0 | 3-4 | 2-3 小时 | 低 |
| **Phase 4** Agent 循环 | 1 | 2 | 4-6 小时 | 中 |

每阶段产出均可独立验收和交付。

---

## 8. 不纳入范围

| 项目 | 理由 |
|------|------|
| 删除现有的 Native provider | 保持降级能力和用户选择权 |
| 用 Rig 替换工具系统 | 当前工具系统灵活且与 MCP 深度耦合 |
| 用 Rig 替换 pipeline 引擎 | 工作流 YAML DSL 是独特设计，Rig 链模式不匹配 |
| 重构 AppState | Rig 没有提供更好的并发模式 |

---

## 9. 附录

### 9.1 Rig 相关资源

- 官方文档: <https://docs.rig.rs/>
- API 参考: <https://docs.rs/rig-core/latest/rig/>
- GitHub: <https://github.com/0xPlaygrounds/rig>
- 版本: v0.37.0 (2026-05)

### 9.2 关键决策记录

| 决策 | 选项 | 选择 | 理由 |
|------|------|------|------|
| Backend 选择方式 | 全局 / 模型级 | 模型级 (ModelConfig.backend) | 灵活，可混合使用 |
| 向量存储位置 | 独立 DB / 内存 | 内存 + SQLite 重建 | 无额外依赖，零持久化风险 |
| Embedding 模型 | OpenAI / 本地 (fastembed) | 优先 OpenAI，后续可加 | 现有 API key 可直接复用 |
| Rig 入口 crate | rig-core / rig facade | rig facade | feature gate 控制依赖更干净 |

### 9.3 术语

| 术语 | 说明 |
|------|------|
| Native | 项目当前的 provider 实现 |
| Rig | [Rig framework](https://rig.rs/) provider 实现 |
| BackendKind | `ModelConfig` 中用于选择后端的枚举字段 |
| Embedding | 文本到向量的转换，用于语义搜索 |
| Vector Index | 内存中的向量索引，用于余弦相似度搜索 |
