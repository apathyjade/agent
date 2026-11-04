# 项目结构与架构概览

## 技术栈

| 层 | 技术 |
|----|------|
| 框架 | Tauri 2.x |
| 后端 | Rust（Tokio、reqwest、rusqlite） |
| AI 框架 | **Rig v0.46**（LLM provider、agent、embeddings、extraction） |
| 前端 | React 18 + TypeScript + Vite |
| 状态管理 | Zustand |
| 样式 | TailwindCSS + 自定义紫色主题 |
| 数据库 | SQLite |

## 目录结构

```
agent/
├── src-tauri/                        # Tauri Rust 后端
│   ├── src/
│   │   ├── main.rs                  # 二进制入口
│   │   ├── lib.rs                   # 入口：模块声明 + invoke_handler（组合根）
│   │   ├── core/                    # 第 1 层：无内部依赖
│   │   │   ├── error.rs             # AppError + Result<T>
│   │   │   ├── keychain.rs          # 安全凭据存储
│   │   │   ├── ipc.rs               # IPC 事件类型（StreamChunk 等）
│   │   │   └── db/                  # SQLite 存储（models + repository）
│   │   ├── runtime/                 # 第 2 层：本地运行时引擎
│   │   │   ├── toolchain/           # 独立运行时引擎（检测、安装、解析、别名）
│   │   │   └── project_env/         # 应用侧项目绑定（建立在 toolchain 之上）
│   │   ├── llm/                     # 第 3 层（与 runtime 并列，互不依赖）：模型接入与意图路由
│   │   │   ├── model.rs             # ModelProvider、ModelConfig
│   │   │   ├── catalog.rs           # ModelInfo、PROVIDER_OPTIONS、PROVIDER_MODELS
│   │   │   ├── provider.rs          # ProviderBox 适配层
│   │   │   ├── rig.rs               # RigProvider（DynModel<Completion>）
│   │   │   ├── util.rs
│   │   │   └── intent/              # 意图路由（classifier + router）
│   │   ├── agent/                   # 第 4 层：Agent 运行时
│   │   │   ├── fast_loop.rs         # Agent 执行循环
│   │   │   ├── tools/               # 工具系统（base + registry + calculator 等）
│   │   │   ├── workers/
│   │   │   ├── critic/
│   │   │   └── orchestrator/
│   │   ├── features/                # 第 5 层：业务能力
│   │   │   ├── session/             # 会话域（lifecycle/ 生命周期管理）
│   │   │   ├── memory/              # 记忆系统
│   │   │   ├── persona/             # 人格设定
│   │   │   ├── skills/              # 技能系统（外部 skills CLI 桥接）
│   │   │   ├── mcp/                 # Model Context Protocol
│   │   │   ├── workspace/           # 工作区与代码索引
│   │   │   └── workflow/            # 工作流引擎（engine + models + scanner）
│   │   └── app/                     # 第 6 层：组合根与 Tauri 适配层
│   │       ├── config.rs            # AppConfig（load/save、runtime_options）
│   │       ├── state.rs             # AppState（Arc<Mutex<>> 共享状态）
│   │       ├── cli/                 # CLI 入口（toolchain 子命令，独立于桌面应用）
│   │       └── commands/            # IPC 命令，按功能拆分
│   ├── tests/                       # 集成测试
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

## 模块分层与依赖规则

顶层只有 6 个目录 + `lib.rs` + `main.rs`，依赖方向单向下行：读目录名就能判断谁能引用谁。

**允许方向（只允许自上而下引用）：**

| 层 | 可依赖 |
|---|---|
| `core` | 仅外部 crate |
| `runtime` | `core` |
| `llm` | `core` |
| `agent` | `core`, `llm` |
| `features` | `core`, `runtime`, `llm`, `agent` |
| `app` | 全部 |

`runtime` 与 `llm` 是并列的两条腿，互不依赖。

**已记录例外（3 处，均为「Agent 运行时调用具体能力」，允许保留）：**

| 文件 | 引用 | 理由 |
|---|---|---|
| `agent/orchestrator/pipeline_adapter.rs` | `features::workflow` | 把 workflow 定义转换成执行计划，是 orchestrator 的适配器 |
| `agent/tools/run_workflow.rs` | `features::workflow` | 工具直接驱动工作流引擎 |
| `agent/workers/code_explorer.rs` | `features::workspace::search` | 代码探索 Worker 使用工作区索引 |

> 这三处要清零，做法是把对应实现移入 `features/`、由 `app/state.rs` 注入——那属于行为重构，不在结构重组范围内。

**硬性约束（可自动检查，`#[cfg(test)]` 代码同样适用）：**

- `core/` 下不得出现 `crate::llm|agent|features|app|runtime`。
- `core/db/` 下不得出现 `crate::agent|features`（DB 记录类型已下沉到 `core/db/models.rs`）。
- `app/` 之外不得出现 `crate::app`（含 `crate::app::config`、`crate::app::commands`、`crate::app::state`）；唯一例外是 crate 根 `lib.rs` 作为组合根的那行导入。
- `llm/` 下不得出现 `crate::app`。

## AI 基础设施 — Rig Framework

### 核心依赖

项目基于 **[Rig](https://rig.rs/) v0.46** 作为统一的 AI 基础设施层，涵盖 LLM 通信、embeddings、结构化提取等所有 AI 能力。Rig 抽象了 20+ 模型 provider 的差异：客户端（如 `openai::OpenAI`）构造具体 **model**，model 被擦除为 `DynModel<Completion>`，再由 `rig::agent::AgentBuilder` 组装成 agent。

**关键 Cargo 依赖：**

```toml
rig = "0.46"        # 完整 Rig 框架（core + agent 运行时 + 所有 provider）
schemars = "1.2"    # JSON Schema derive（匹配 Rig 内部版本）
```

### 架构分层

```
┌─────────────────────────────────────────────────────┐
│   AgentLoop / Lifecycle / Intent / Execution        │  ← AI 消费者模块
├─────────────────────────────────────────────────────┤
│               ProviderBox trait                      │  ← 统一适配层
│    prompt() / chat() / prompt_with_tools()          │
│    stream_prompt() / stream_with_tools()            │
├─────────────────────────────────────────────────────┤
│        RigProvider { DynModel<Completion> }         │  ← 类型擦除适配器
│    AgentBuilder::new(model).preamble(..).build()    │
├─────────────────────────────────────────────────────┤
│    rig::providers::openai / anthropic / gemini / …  │  ← Rig provider 客户端
│    client.chat(id) / client.completion(id)          │     → Model, erase() → DynModel
│    rig::agent::{AgentBuilder, Agent, AgentRunner}   │  ← agent 运行时 + 工具循环
│    rig::tool::{DynamicTool, ToolSet, ToolOutput}    │
│    rig::extractor::ExtractorBuilder                 │
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
| **LLM 对话** | `ProviderBox::prompt()` / `chat()` → `rig::agent::Agent` | `llm/rig.rs` — `RigProvider` |
| **工具调用** | `AgentBuilder::dynamic_tools()` + `rig::tool::DynamicTool`，agent 内部驱动工具循环 | `llm/rig.rs` / `agent/tools/registry.rs` — `to_rig_tools()` |
| **流式传输** | `AgentRunner::stream()` → `MultiTurnStreamItem`（取 `StreamEvent::Text` 与 `FinalResponse`） | `llm/rig.rs` — `stream_prompt()` / `stream_with_tools()` |
| **语义搜索** | `provider.embedding(id, None)` → `DynModel<Embedding>` + `InMemoryVectorIndex` | `features/memory/mod.rs` — `retrieve_relevant()` |
| **结构化提取** | `rig::extractor::ExtractorBuilder::<T>::new(model)` — 编译期类型安全 | `llm/rig.rs` — `extract_structured::<T>()` |
| **工具执行** | `agent/fast_loop.rs` + `ToolRegistry` — AgentLoop 执行 tool_calls | `agent/fast_loop.rs` — `run()` / `run_stream()` |

### 关键设计决策

- **类型擦除一次**：`create_rig_provider()` 在配置加载时把各 provider 的 `Model<W>` 擦除为 `DynModel<Completion>`，因此所有后端共享同一个 `RigProvider`，可以直接放进 `Arc<dyn ProviderBox>`
- **适配器而非替换**：`ProviderBox` trait 隔离内部模块与 Rig 类型；Rig 的 `Message`、`DynamicTool` 仅在适配层和工具层出现
- **agent 按请求构建**：`AgentBuilder::new(model.clone()).preamble(..).build()` 每次调用构建，preamble 由调用方传入
- **工具契约自有**：Rig 0.45 起不再有 object-safe 的 `ToolDyn`，项目保留 `AgentTool`（`agent/tools/base.rs`），在构建工具集时转成 `DynamicTool`
- **降级策略**：embedding 不可用时 → LIKE 搜索；extraction 失败时 → 回退到 chat JSON 解析

### 进程启动

应用启动时，`AppState::new()` 按以下顺序初始化 AI 基础设施：

1. `config.json` → `ProviderRegistry::new()` → 为每个启用的 model 创建 `RigProvider`（内部持有 `DynModel<Completion>`）
2. `MemoryManager::new()` — 传入 OpenAI API key → 可选初始化 `DynModel<Embedding>`
3. `IntentRouter::new()` — 创建 `LlmClassifier`（Rig extraction 优先）
4. `seed_defaults()` — 预先注入内置记忆与人格设定（已填充则跳过）

## IPC 通信

约 100 个 IPC 命令，通过 Tauri `invoke()` 调用。Rust 端 `snake_case` 命名，前端 `invoke()` 使用驼峰参数。
流式传输通过 Tauri 事件 `stream_chunk` 实现，前端通过 `listen('stream_chunk', callback)` 接收。

## Agent 循环（`agent/fast_loop.rs`）

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

> 技能（Skill）不在上表中：技能是 SKILL.md 指令包，由外部 `skills` CLI 安装到各编码代理目录，
> 供 Claude Code / Codex / OpenCode 等外部代理读取，**不**注册为本应用的 `ToolRegistry` 工具。

## 记忆系统

由 `MemoryManager` 管理，支持语义搜索和关键词搜索双模式。

| 模式 | 原理 | 依赖 |
|------|------|------|
| **语义搜索**（优先） | `DynModel<Embedding>::embed_text()` → 余弦相似度 top-k | OpenAI API key |
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
