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
│   │   │   ├── tools/               # 工具系统（rig-tools + registry + approval）
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
│   │   │   ├── workflow/            # 工作流引擎（engine + models + scanner）
│   │   │   └── intent_management/   # 意图库（seeds + json 导入导出）
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
| `agent/workers/code_explorer.rs` | `features::workspace::search` | 代码探索 Worker 使用工作区索引 |

> 这三处要清零，做法是把对应实现移入 `features/`、由 `app/state.rs` 注入——那属于行为重构，不在结构重组范围内。

**硬性约束（可自动检查，`#[cfg(test)]` 代码同样适用）：**

- `core/` 下不得出现 `crate::llm|agent|features|app|runtime`。
- `core/db/` 下不得出现 `crate::agent|features`（DB 记录类型已下沉到 `core/db/models.rs`）。`agent/` 内仅允许 `agent/workers/memory.rs` 的 rustdoc 链接提及 `crate::features::…`，那不是依赖边。
- `app/` 之外不得出现 `crate::app`（含 `crate::app::config`、`crate::app::commands`、`crate::app::state`）；唯一例外是 crate 根 `lib.rs` 作为组合根的那行导入。
- `llm/` 下不得出现 `crate::app`。

## 代码注释约定

Rust 后端（`src-tauri/`）的注释统一使用**简体中文**：

| 注释类型 | 语言 | 说明 |
|---|---|---|
| `//` 行注释 | 中文 | 解释「为什么」，不逐行复述代码 |
| `///` 文档注释 | 中文 | 说明意图、约束与边界条件；不复述签名 |
| `//!` 模块文档 | 中文 | 说明模块职责 |

保持英文不变的部分：标识符与变量名、字符串字面量、`log::` 消息文本、`AppError` 的 Display 文本、IPC 事件名、`serde` 属性值。

- **同一文件内不中英混排**——混排本身就是可读性缺陷。
- 关键领域术语在中文句子里保留英文原文（`embedding`、`checkpoint`、`rerank`、`LTS` 等），这些术语本身就是英文，翻译反而增加检索成本。
- rustdoc 内部链接（`[`Foo`]`）属于代码，不翻译。

> 此前项目约定为「代码注释使用英文」，2026-11-05 起改为中文，理由与迁移范围见 `openspec/changes/src-tauri-readability/`。

## 意图管理

意图是**可管理的数据**，不是代码常量：两张表（`intent_domains` / `intents`）落在 `core/db`，仓库方法在 `core/db/intent_repo.rs`，内置模板与导入导出在 `features/intent_management/`，IPC 在 `app/commands/intent.rs`，界面是 `src-ui/src/components/management/IntentManagerPage.tsx`。

- **领域（domain）是一套可切换的工作集**，同一时刻恰好一个活跃；路由只读活跃领域里**启用**的意图。
- **首次启动播种**内置的「程序开发」领域（12 条意图），成功后写 `settings` 的 `intent.seeded` 标记，之后永不重播——用户删掉内置领域也不会复活。整段播种在一个事务里完成，进程中途崩溃不会留下半成品。
- **分类提示词由数据生成**（描述 + 示例），所以「加一条意图」不需要改提示词；LLM 走 `ProviderRegistry`，取不到就退回到示例/双字重合度打分，仍无结果再按 `confidence_threshold` 落到 `fallback_intent`。
- **`fallback_intent` 存的是意图 id**：导入时接受 id 或名字并改写成新 id，命令层同样归一化；删除意图会在同一事务里清空指向它的引用。
- **`allowed_tools: []` 表示「不允许任何工具」**，不是「不限制」；但这条只在快路径成立——`path = deep` 会先返回 orchestrator，所以「深路径 + 空工具清单」被 `IntentInput::validate` 直接拒绝。
- `workflow` / `default_agent` / `scope` 目前**只存不用**，等后续接入执行链；界面上标注为「后续接入」。

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
│   ToolLoop / Lifecycle / Intent / Execution         │  ← AI 消费者模块
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
| **工具调用** | `AgentBuilder::dynamic_tools()` + `rig::tool::DynamicTool`，agent 内部驱动工具循环 | `llm/rig.rs` / `agent/tools/registry.rs` — `tool_runtime()` |
| **人工确认** | `AgentHook::on_dispatch` + `ToolPolicyHook`；拒绝时 `DispatchAction::skip(reason)` | `agent/tools/policy.rs` / `agent/tools/approval.rs` |
| **流式传输** | `ProviderBox::stream_prompt()` → `BoxStream<Result<String>>`（内部把 `MultiTurnStreamItem` 归一为文本 delta）；`ProviderBox::stream_with_tools()` → `StreamingResult`，调用方直接匹配 `rig::streaming::StreamEvent::Text` / `FinalResponse` / ToolCall / ToolResult | `llm/rig.rs` — `stream_prompt()` / `stream_with_tools()` |
| **语义搜索** | `provider.embedding(id, None)` → `DynModel<Embedding>` + `InMemoryVectorIndex` | `features/memory/mod.rs` — `retrieve_relevant()` |
| **结构化提取** | `rig::extractor::ExtractorBuilder::<T>::new(model)` — 编译期类型安全 | `llm/rig.rs` — `extract_structured::<T>()` |
| **工具执行** | `agent/fast_loop.rs` + `ToolRegistry` — `ToolLoop` 执行 tool_calls | `agent/fast_loop.rs` — `run()` / `run_stream()` |

### 关键设计决策

- **类型擦除一次**：`create_rig_provider()` 在配置加载时把各 provider 的 `Model<W>` 擦除为 `DynModel<Completion>`，因此所有后端共享同一个 `RigProvider`，可以直接放进 `Arc<dyn ProviderBox>`
- **适配器而非替换**：`ProviderBox` trait 隔离内部模块与 Rig 类型；Rig 的 `Message`、`DynamicTool` 仅在适配层和工具层出现
- **agent 按请求构建**：`AgentBuilder::new(model.clone()).preamble(..).build()` 每次调用构建，preamble 由调用方传入
- **工具用官方 rig-tools**：`read` / `write` / `edit` / `search` / `list` / `shell` / `delete` / `move` 直接来自 `rig-tools` crate，自带 annotations 与 footprint；项目不再维护本地文件/执行工具，只保留注册表、审批与 MCP 包装
- **策略在注册表，执行在 Rig**：`ToolRegistry` 只维护启用状态、别名、来源与审批通道；参数校验、错误语义、工具循环全部由 Rig 负责
- **降级策略**：embedding 不可用时 → LIKE 搜索；extraction 失败时 → 回退到 chat JSON 解析

### 进程启动

应用启动时，`AppState::new()` 按以下顺序初始化 AI 基础设施：

1. `config.json` → `ProviderRegistry::new()` → 为每个启用的 model 创建 `RigProvider`（内部持有 `DynModel<Completion>`）
2. `MemoryManager::new()` — 传入 OpenAI API key → 可选初始化 `DynModel<Embedding>`
3. `IntentRouter::new()` — 创建 `LlmClassifier`（从 `ProviderRegistry` 解析 `ProviderBox`，走 `prompt` 后解析 JSON；`llm/intent/classifier.rs`）
4. `seed_defaults()` — 预先注入内置记忆与人格设定（已填充则跳过）

## IPC 通信

约 100 个 IPC 命令，通过 Tauri `invoke()` 调用。Rust 端 `snake_case` 命名，前端 `invoke()` 使用驼峰参数。
流式传输通过 Tauri 事件 `stream_chunk` 实现，前端通过 `listen('stream_chunk', callback)` 接收。

## Agent 循环（`agent/fast_loop.rs`）

- 每轮最多 10 次模型调用（`max_turns`），参数畸形允许重试 2 次，连续 3 次畸形终止
- 工具并发上限 4；同一轮里 footprint 冲突的调用会被跳过并回灌原因
- 流式事件：`Content` / `ToolCall` / `ToolResult` / `Error` / `Done`（`Done` 恰好一次）
- 记忆注入：system prompt 后自动插入 `<remembered_context>`

## 工具系统

| 工具 | 默认启用 | 说明 |
|------|----------|------|
| `read` | ✅ | 读文本 / 图片，支持 offset/limit 分页 |
| `write` | ✅ | 整文件写入，自动创建父目录 |
| `edit` | ✅ | 多处精确替换，返回 diff |
| `search` | ✅ | 正则搜索，遵循 `.gitignore` |
| `list` | ✅ | 列目录，支持深度与 glob |
| `shell` | ✅ | `sh -c` 执行命令（Windows 需 Git Bash/WSL） |
| `delete` | ✅ | 删除文件（拒绝目录） |
| `move` | ✅ | 移动/重命名文件 |

> 以上全部来自官方 `rig-tools` crate，项目不再维护本地文件/执行工具。
> 工具的实际启用状态来自 `AppConfig.enabled_tools`，`list_tools` 直接返回注册表里的状态。
> MCP 工具以 `server__tool` 注册，启用状态由各自的服务器配置决定，不受内置列表支配。
>
> 审批优先级：拒绝名单 → 放行名单 → 确认名单 → 调用级 footprint（写入需确认、只读直接执行）→
> 工具级 `destructive` 注解 → 放行。等待确认超过 120 秒按拒绝处理。

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
