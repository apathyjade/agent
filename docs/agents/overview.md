# 项目结构与架构概览

## 技术栈

| 层 | 技术 |
|----|------|
| 框架 | Tauri 2.x |
| 后端 | Rust（Tokio、reqwest、rusqlite） |
| AI 框架 | **Rig v0.37**（LLM provider、agent、embeddings、extraction） |
| 前端 | React 18 + TypeScript + Vite |
| 状态管理 | Zustand |
| 样式 | TailwindCSS + 自定义紫色主题 |
| 数据库 | SQLite |

## 目录结构

```
agent/
├── src-tauri/                        # Tauri Rust 后端
│   ├── src/
│   │   ├── lib.rs                   # 入口：模块声明 + invoke_handler
│   │   ├── main.rs                  # 二进制入口
│   │   ├── config.rs                # AppConfig、ModelProvider 枚举
│   │   ├── state.rs                 # AppState（Arc<Mutex<>> 共享状态）
│   │   ├── error.rs                 # AppError + Result<T>
│   │   ├── keychain.rs              # 安全凭据存储
│   │   ├── agent/loop.rs            # Agent 执行循环
│   │   ├── api/                     # LLM API 客户端（openai, anthropic）
│   │   ├── commands/                # IPC 命令，按功能拆分
│   │   ├── db/                      # SQLite 存储（models + repository）
│   │   ├── tools/                   # 工具系统（calculator, file_system 等）
│   │   ├── environment/             # 运行时环境管理
│   │   ├── mcp/                     # Model Context Protocol
│   │   ├── memory/                  # 记忆系统
│   │   ├── pipeline/                # 工作流引擎
│   │   ├── skills/                  # 技能系统
│   │   └── tests/                   # 测试
│   ├── Cargo.toml
│   └── tauri.conf.json              # CSP 白名单配置
├── src-ui/                          # React 前端
│   ├── src/
│   │   ├── api/tauri.ts             # IPC invoke() 封装
│   │   ├── store/                   # Zustand slices
│   │   ├── types/index.ts           # TS 接口定义（严格模式）
│   │   └── components/              # ~30 个组件
│   └── package.json + 构建配置
├── scripts/                         # AI 工作流脚本
└── docs/                            # 项目文档
```

## AI 基础设施 — Rig Framework

### 核心依赖

项目基于 **[Rig](https://rig.rs/) v0.37** 作为统一的 AI 基础设施层，涵盖 LLM 通信、embeddings、结构化提取等所有 AI 能力。Rig 抽象了 20+ 模型 provider 的差异，提供一致的 `CompletionClient` / `EmbeddingModel` / `Extractor` 接口。

**关键 Cargo 依赖：**

```toml
rig = "0.37"        # 完整 Rig 框架（core + 所有 provider）
schemars = "1.2"    # JSON Schema derive（匹配 Rig 内部版本）
```

### 架构分层

```
┌─────────────────────────────────────────────────────┐
│   AgentLoop / Lifecycle / Intent / Execution        │  ← AI 消费者模块
├─────────────────────────────────────────────────────┤
│               LLMProvider trait                      │  ← 统一适配层
│    chat() → ChatResponse                            │
│    chat_stream() → BoxStream<StreamPayload>         │
│    chat_with_tools() → ChatResponse (+ tool_calls)  │
├─────────────────────────────────────────────────────┤
│           RigProvider<C: CompletionClient>           │  ← Rig 泛型适配器
│    do_chat() / chat_with_tools() / stream_chat()    │
├─────────────────────────────────────────────────────┤
│    rig::providers::openai / anthropic / gemini / …  │  ← Rig provider 客户端
│    rig::completion::{Chat, Completion, CompletionModel} │
│    rig::embeddings::EmbeddingModel                  │
│    rig::streaming::StreamingChat                    │
│    rig::extractor::Extractor                        │
└─────────────────────────────────────────────────────┘
```

### Provider 映射

所有 11 个模型 provider 统一通过 Rig 驱动。每个 provider 对应一个 `rig-providers` 下的客户端：

| Config 标识 | Rig 客户端 | 说明 |
|------------|-----------|------|
| `openai` | `rig::providers::openai` | 直接映射 |
| `anthropic` | `rig::providers::anthropic` | 直接映射 |
| `google` | `rig::providers::gemini` | Google Gemini |
| `groq` | `rig::providers::groq` | Groq 高速推理 |
| `deepseek` | `rig::providers::deepseek` | DeepSeek |
| `ollama` | `rig::providers::ollama` | 本地模型，无需 API key |
| `moonshot` | `rig::providers::moonshot` | 月之暗面 Kimi |
| `zhipu` | `rig::providers::openai`（兼容） | 通过 base_url 切换 |
| `siliconflow` | `rig::providers::openai`（兼容） | 通过 base_url 切换 |
| `lmstudio` | `rig::providers::openai`（兼容） | 本地，无需 API key |
| `custom` | `rig::providers::openai`（兼容） | 任意 OpenAI 兼容 API |

### 核心能力

| 能力 | Rust API | 实现位置 |
|------|----------|----------|
| **LLM 对话** | `LLMProvider::chat()` / `chat_stream()` | `api/rig.rs` — `RigProvider` |
| **工具调用** | `chat_with_tools()` — 通过 Rig Completion API 返回原始 tool_calls | `api/rig.rs` — `chat_with_tools()` |
| **流式传输** | `StreamingChat::stream_chat()` — 真正的增量 SSE | `api/rig.rs` — `chat_stream()` |
| **语义搜索** | `rig::embeddings::EmbeddingModel` + `InMemoryVectorIndex` | `memory/mod.rs` — `retrieve_relevant()` |
| **结构化提取** | `rig::extractor::Extractor<T>` — 编译期类型安全 | `api/rig.rs` — `extract_structured::<T>()` |
| **工具执行** | `agent/loop.rs` + `ToolRegistry` — AgentLoop 执行 tool_calls | `agent/loop.rs` — `run()` / `run_stream()` |

### 关键设计决策

- **适配器而非替换**：通过 `RigProvider<C: CompletionClient>` 泛型包装器适配到内部 `LLMProvider` trait，而非直接暴露 Rig 类型到消费者模块
- **泛型工厂**：`create_rig_provider()` 根据 `ModelConfig.provider` 映射到正确的 Rig 客户端
- **构建时单态化**：Rig 使用编译期泛型，通过 `Arc<dyn LLMProvider>` 桥接到运行时多态
- **降级策略**：embedding 不可用时 → LIKE 搜索；extraction 失败时 → 回退到 chat JSON 解析

### 进程启动

应用启动时，`AppState::new()` 按以下顺序初始化 AI 基础设施：

1. `config.json` → `ProviderRegistry::new()` → 为每个启用的 model 创建 `RigProvider`
2. `MemoryManager::new()` — 传入 OpenAI API key → 可选初始化 `EmbeddingModel`
3. `IntentRouter::new()` — 创建 `LlmClassifier`（Rig extraction 优先）
4. `seed_defaults()` / `reconcile_skills()` — 预先注入内置记忆和技能

## IPC 通信

约 100 个 IPC 命令，通过 Tauri `invoke()` 调用。Rust 端 `snake_case` 命名，前端 `invoke()` 使用驼峰参数。
流式传输通过 Tauri 事件 `stream_chunk` 实现，前端通过 `listen('stream_chunk', callback)` 接收。

## Agent 循环（`agent/loop.rs`）

- 重试 ×3，指数退避，仅重试可恢复错误（网络超时等）
- 上下文窗口优化：保留 system 消息，从后往前裁剪
- 每轮最多 10 次工具迭代
- 流式事件：`Content` / `ToolCall` / `ToolResult` / `Done`
- 记忆注入：system prompt 后自动插入 `<remembered_context>`

## 工具系统

| 工具 | 默认启用 | 说明 |
|------|----------|------|
| `calculator` | ✅ | 算术计算 |
| `file_system` | ❌ | 文件读写 |
| `web_search` | ❌ | 网络搜索 |
| `code_executor` | ✅ | 代码执行沙箱 |
| 动态脚本工具 | 按需 | 由 SkillManager 注册 |

## 记忆系统

由 `MemoryManager` 管理，支持语义搜索和关键词搜索双模式。

| 模式 | 原理 | 依赖 |
|------|------|------|
| **语义搜索**（优先） | Rig `EmbeddingModel` → 余弦相似度 top-k | OpenAI API key |
| **关键词搜索**（降级） | SQLite `LIKE '%keyword%'` | 无条件 |

**数据流**：
```
存入记忆 → SQLite 存储 + (可选) Rig embedding → InMemoryVectorIndex
检索记忆 → Rig embedding → VectorIndex.search() → 取 SQLite 完整记录
         → (不可用时) SQLite LIKE 搜索
```

## 数据库

- [开发流程](workflow.md) — 了解如何开始编码
- [OpenSpec 工作流](openspec-workflow.md) — 功能开发的标准化流程
