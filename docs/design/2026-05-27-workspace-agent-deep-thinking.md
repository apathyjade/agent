# Workspace Agent with Deep Thinking — 架构设计

> 日期: 2026-05-27
> 状态: Draft
> 关联: 基于现有 Agent 项目（Tauri + Rig v0.37）的架构演进

---

## 1. 概述

### 1.1 目标

将现有 AI 聊天客户端改造为 **分层 Agent 系统**（Hierarchical Agent System），具备：

- **Workspace 感知**：理解代码库/工作区的结构、依赖、Git 状态
- **深度思考**：Chain-of-Thought 推理 + 自我反思 + 快/慢分层思维
- **通用可扩展**：既做开发者代码助手，也做通用 AI 桌面网关

### 1.2 设计原则

- **增量迁移**：新模块与现有代码并存，现有关键模块（工具系统、记忆、MCP、Pipeline）保留并复用
- **Rig 原生**：利用 Rig v0.37 的 Agent/Tool 框架构建 Worker Agents，而非自造轮子
- **可观测性**：思考过程向用户透明展示，不隐藏推理链
- **渐进复杂度**：简单查询走 Fast Path，复杂任务走 Deep Path

---

## 2. 整体架构

```
┌───────────────────────────────────────────────────────────┐
│                     Interface Layer                        │
│   Frontend (React + Zustand)                              │
│   ThinkingPanel │ TaskGraphView │ WorkspaceTree           │
│   IPC: invoke() + Tauri Events (stream_chunk)             │
└───────────────────────┬───────────────────────────────────┘
                        │
┌───────────────────────▼───────────────────────────────────┐
│                  Orchestration Layer                       │
│                                                            │
│   ┌──────────────┐    ┌──────────────────┐                │
│   │IntentRouter  │───▶│ OrchestratorAgent │               │
│   │(复用现有)     │    │                  │                │
│   └──────────────┘    │ Task Decomposer  │                │
│                       │ Dispatch Queue   │                │
│                       │ Result Synthesizer│               │
│                       └────────┬─────────┘                │
└────────────────────────────────┼──────────────────────────┘
                                 │
           ┌─────────────────────┼─────────────────────┐
           │                     │                     │
┌──────────▼──────────┐ ┌───────▼───────┐ ┌───────────▼────┐
│   Thinker Worker    │ │Code Explorer  │ │ Code Editor    │
│   (CoT / Planning)  │ │Worker         │ │ Worker         │
│   — 无工具，纯 LLM  │ │ — glob, grep  │ │ — write, edit  │
│   — 结构化推理       │ │ — tree, read  │ │ — create, del  │
└─────────────────────┘ └───────────────┘ └────────────────┘
           │                     │                     │
┌──────────▼──────────┐ ┌───────▼───────┐ ┌───────────▼────┐
│   Shell Worker      │ │Web Worker     │ │ Memory Worker  │
│   — command_exec    │ │ — web_search  │ │ — store/search  │
│   — git ops         │ │ — web_fetch   │ │ └── 复用现有    │
│   — test runner     │ │               │ │                 │
└─────────────────────┘ └───────────────┘ └────────────────┘
           │
┌──────────▼──────────┐
│   MCP Bridge Worker │
│   — 动态注册 MCP    │
│   — 外部工具调用    │
│   └── 复用现有 mcp/ │
└─────────────────────┘
           │
┌──────────▼────────────────────────────────────────────────┐
│                    Quality Layer                           │
│                                                            │
│   Critic Agent                                            │
│   — Correctness Check: 输出是否正确实现目标               │
│   — Quality Check: 代码质量、错误处理、边缘情况           │
│   — Go/No-Go: 通过则继续，否则触发重试                   │
└───────────────────────────────────────────────────────────┘
           │
┌──────────▼────────────────────────────────────────────────┐
│               Workspace Context Layer                      │
│                                                            │
│   ┌──────────────┐  ┌──────────────┐  ┌──────────────┐   │
│   │Codebase      │  │File Watcher  │  │Search Engine │   │
│   │Indexer       │  │(notify crate)│  │(ripgrep/glob)│   │
│   │— 语言检测     │  │— 增量索引    │  │— 全文搜索    │   │
│   │— 依赖分析     │  │— 排除 noise  │  │— AST 感知    │   │
│   └──────────────┘  └──────────────┘  └──────────────┘   │
│   ┌──────────────┐                                        │
│   │Git Tracker   │                                        │
│   │— status/diff │                                        │
│   │— log/blame   │                                        │
│   └──────────────┘                                        │
└───────────────────────────────────────────────────────────┘
```

### 2.1 各层职责

| 层 | 职责 | 关键组件 |
|----|------|----------|
| **Interface** | 用户交互、思考过程可视化 | ThinkingPanel, TaskGraphView, WorkspaceTree |
| **Orchestration** | 意图路由、任务分解、调度合成 | OrchestratorAgent, IntentRouter |
| **Execution** | 领域专用任务执行 | Thinker, Code Explorer, Code Editor, Shell, Web, Memory, MCP Bridge |
| **Quality** | 输出验证、自反思 | Critic Agent |
| **Workspace Context** | 工作区状态管理 | CodebaseIndexer, FileWatcher, SearchEngine, GitTracker |

---

## 3. 深度思考系统（Three-Tier Thinking）

### 3.1 三层思维模型

借鉴 Kahneman 双系统理论，结合 Agent 分层实现三层渐进复杂度：

```
Layer 1: 直觉响应 (Fast Path)
  ┌─────────────────────────────────────────────┐
  │ 触发条件: 简单问答、问候、已知信息检索       │
  │ 路径:     IntentRouter → Thinker(短路径)     │
  │ 目标:     < 2s 响应                          │
  │ 展示:     直接反馈，无思考面板               │
  └─────────────────────────────────────────────┘

Layer 2: 程序化推理 (Moderate Path)
  ┌─────────────────────────────────────────────┐
  │ 触发条件: 编码、调试、标准分析               │
  │ 路径:     Orchestrator → 拆任务 → Workers    │
  │           执行 → Critic 验证                 │
  │ 目标:     5-30s，有进度展示                   │
  │ 展示:     折叠思考面板，展示任务 DAG 进度    │
  └─────────────────────────────────────────────┘

Layer 3: 深度反思 (Intensive Path)
  ┌─────────────────────────────────────────────┐
  │ 触发条件: 复杂架构决策、跨文件重构、深分    │
  │           析、bug 深入排查                    │
  │ 路径:     Orchestrator → Thinker CoT(完整)   │
  │            → Workers 执行 → Critic 反思      │
  │            → 可能需要重试循环                │
  │ 目标:     30s-5min，展示完整思考链            │
  │ 展示:     完整展开的思考面板，CoT 步骤化     │
  └─────────────────────────────────────────────┘
```

### 3.2 CoT 结构化提示

Thinker Worker 使用的 System Prompt 模板：

```
You are a deep thinking module. Structure your reasoning in phases:

[STEP 1: Problem Analysis]
- What does the user truly need?
- What are implicit requirements?
- What constraints exist?

[STEP 2: Context Gathering]
- What information is available?
- What is missing? What assumptions needed?
- What parts of the workspace are relevant?

[STEP 3: Approach Exploration]
- Consider 2-3 approaches
- Evaluate trade-offs for each
- Identify risks and unknowns

[STEP 4: Selected Approach]
- Choose best approach
- Justify the choice
- Define success criteria

[STEP 5: Execution Plan]
- Step-by-step plan with clear checkpoints
- Parallel opportunities noted
- Verification criteria for each step
```

### 3.3 自我反思循环

Critic Agent 在每次 Worker 产出后执行：

```
1. Correctness: 输出是否正确满足任务目标？
2. Completeness: 是否有遗漏的边界情况？
3. Quality: 代码质量、错误处理、性能？
4. Consistency: 是否与现有代码风格一致？
5. Decision: Go (继续) / Revise (重试并附反馈) / Escalate (上报 Orchestrator)
```

重试时，Critic 的反馈注入 Worker 的下一次调用：

```rust
pub struct Critique {
    pub decision: CritiqueDecision,  // Go | Revise | Escalate
    pub issues: Vec<String>,
    pub suggestions: Vec<String>,
    pub severity: CritiqueSeverity,  // Info | Warning | Blocking
}
```

---

## 4. Worker Agent 详细设计

### 4.1 Worker 接口

所有 Worker 实现统一 trait，利用 Rig Agent 作为执行引擎：

```rust
#[async_trait]
pub trait WorkerAgent: Send + Sync {
    fn kind(&self) -> WorkerKind;
    fn description(&self) -> &str;
    
    /// 执行一个子任务，返回结果
    async fn execute(&self, task: SubTask) -> Result<WorkerResult>;
    
    /// 带 Critic 反馈的重试
    async fn execute_with_feedback(
        &self, 
        task: SubTask, 
        feedback: &Critique,
    ) -> Result<WorkerResult>;
}
```

Worker 基于 Rig `Agent` 构建：

```rust
// 每个 Worker 内部封装一个 Rig Agent
pub struct CodeExplorerWorker {
    agent: rig::agent::Agent<rig::providers::openai::Client>,
    tools: Vec<Arc<dyn Tool>>,  // glob, grep, read, tree 等
}
```

### 4.2 Worker 清单

| Worker | 实现位置 | Rig Agent？ | 绑定工具 | 依赖的现有模块 |
|--------|----------|-------------|----------|---------------|
| Thinker | `workers/thinker.rs` | ✅ | 无（纯 LLM） | `api/provider.rs` |
| Code Explorer | `workers/code_explorer.rs` | ✅ | glob, grep, read, tree, AST | `tools/file_system.rs` |
| Code Editor | `workers/code_editor.rs` | ✅ | write, edit, create, rename, delete | `tools/file_system.rs` |
| Shell | `workers/shell.rs` | ✅ | command_exec, git_* | `tools/code_executor.rs` |
| Web | `workers/web.rs` | ✅ | web_search, web_fetch | `tools/web_search.rs` |
| Memory | `workers/memory.rs` | ✅ | memory_store, memory_search | `memory/mod.rs` |
| MCP Bridge | `workers/mcp_bridge.rs` | ❌ 直接调用 | 动态注册 MCP tools | `mcp/manager.rs` |

### 4.3 Task Graph 数据模型

Orchestrator 生成的 DAG 任务图：

```rust
pub struct TaskGraph {
    pub id: String,
    pub goal: String,
    pub nodes: Vec<TaskNode>,
    pub edges: Vec<TaskEdge>,  // dependency edges
}

pub struct TaskNode {
    pub id: String,
    pub label: String,
    pub worker_kind: WorkerKind,
    pub instruction: String,
    pub status: NodeStatus,  // Pending | Running | Completed | Failed | Skipped
    pub result: Option<WorkerResult>,
    pub critique: Option<Critique>,
}

pub struct TaskEdge {
    pub from: String,
    pub to: String,  // 'to' depends on 'from'
}
```

---

## 5. Workspace Context Layer

### 5.1 Codebase Indexer

```rust
pub struct CodebaseIndex {
    pub root: PathBuf,
    pub language: Option<ProjectLanguage>,  // Rust, TypeScript, Python, etc.
    pub framework: Option<String>,          // Tauri, Next.js, Actix, etc.
    pub dependencies: Vec<Dependency>,
    pub file_count: usize,
    pub dir_count: usize,
    pub last_indexed: SystemTime,
}

pub struct CodebaseIndexer {
    db: Arc<Mutex<Database>>,  // 索引持久化到 SQLite
}

impl CodebaseIndexer {
    pub async fn index(&self, path: &Path) -> Result<CodebaseIndex>;
    pub async fn get_index(&self, path: &Path) -> Option<CodebaseIndex>;
    pub async fn invalidate(&self, path: &Path);
}
```

### 5.2 File Watcher

基于 `notify` crate 实现文件变更监听：

```rust
pub struct FileWatcher {
    watched_dirs: Vec<PathBuf>,
    exclude_patterns: Vec<String>,  // .git, node_modules, target
    change_tx: mpsc::Sender<FileChangeEvent>,
}

pub enum FileChangeEvent {
    Created(PathBuf),
    Modified(PathBuf),
    Deleted(PathBuf),
}
```

文件变更触发：索引增量更新 + 通知 Orchestrator（例如自动检测新文件）。

### 5.3 搜索集成

| 搜索方式 | 工具 | 用途 |
|----------|------|------|
| 全文搜索 | ripgrep (`rg`) | 关键词搜索，超快 |
| 文件匹配 | glob | 模式匹配查找文件 |
| AST 感知 | ast-grep / tree-sitter | 语义级代码搜索（找到所有函数定义、类、导入等） |
| 语义搜索 | Rig Embedding | 自然语言搜索代码（现有 Memory 系统扩展） |

### 5.4 Git Integration

```rust
pub struct GitIntegration {
    repo: Option<git2::Repository>,
}

impl GitIntegration {
    pub fn status(&self) -> Result<Vec<GitFileStatus>>;    // git status
    pub fn diff(&self, path: &str) -> Result<String>;       // git diff
    pub fn log(&self, n: usize) -> Result<Vec<CommitInfo>>; // git log -n
    pub fn blame(&self, path: &str) -> Result<Vec<BlameLine>>;
}
```

---

## 6. 前端变化

| 新组件 | 文件位置 | 用途 |
|--------|----------|------|
| `ThinkingPanel` | `src-ui/src/components/ThinkingPanel.tsx` | 可折叠思考过程面板，CoT 逐步展示 |
| `TaskGraphView` | `src-ui/src/components/TaskGraphView.tsx` | 任务 DAG 可视化，实时进度 |
| `WorkspaceTree` | `src-ui/src/components/WorkspaceTree.tsx` | 项目文件树，可点击预览 |
| `AgentStatusBar` | `src-ui/src/components/AgentStatusBar.tsx` | 当前 Worker 状态、耗时 |
| `CriticFeedback` | `src-ui/src/components/CriticFeedback.tsx` | Self-reflection 提示和建议 |

### 6.1 ThinkingPanel 设计

```
┌─ 🤔 Agent is thinking... ──────────────[−]┐
│                                            │
│  ┌─ Step 1: Analyzing the problem ──────┐ │
│  │  User wants to refactor auth module  │ │
│  │  Dependencies: user_service, db, jwt │ │
│  └──────────────────────────────────────┘ │
│                                           │
│  ┌─ Step 2: Exploring approaches ───────┐ │
│  │  1. Keep existing JWT, extract trait │ │
│  │  2. Switch to OAuth2 (overkill)      │ │
│  │  → Selected: Approach 1              │ │
│  └──────────────────────────────────────┘ │
│                                           │
│  ┌── Task Progress ─────────────────────┐ │
│  │  ✅ [Code Exp] Scan auth usages      │ │
│  │  ⏳ [Thinker] Design interface       │ │
│  │  ⬜ [Code Ed] Implement changes      │ │
│  │  ⬜ [Shell]  Run tests               │ │
│  └──────────────────────────────────────┘ │
└────────────────────────────────────────────┘
```

### 6.2 IPC 事件流

前端通过 Tauri `listen` 接收实时 Agent 状态：

```typescript
// Tauri 事件
interface AgentThinkingEvent {
  phase: 'analyzing' | 'planning' | 'executing' | 'reflecting' | 'synthesizing';
  stepId: string;
  stepLabel: string;
  content: string;        // CoT 文本
  nodeStatus: NodeStatus; // 当前节点状态
}

interface TaskGraphUpdate {
  graphId: string;
  nodes: TaskNodeStatus[];
  totalSteps: number;
  completedSteps: number;
}

interface CriticFeedbackEvent {
  stepId: string;
  decision: 'go' | 'revise' | 'escalate';
  issues: string[];
  suggestions: string[];
}
```

---

## 7. 与现有模块的集成策略

### 7.1 保留并复用

| 现有模块 | 集成方式 | 等级 |
|----------|----------|------|
| `tools/` (全部) | Worker 工具来源 | ✅ 直接复用 |
| `api/provider.rs` | Orchestrator/Worker 的 LLM provider | ✅ 直接复用 |
| `memory/` | Memory Worker 封装 | ✅ 直接复用 |
| `mcp/` | MCP Bridge Worker 封装 | ✅ 直接复用 |
| `pipeline/` | 可作为 Worker 的预定义工作流模板 | ✅ 保留 |
| `skills/` | Worker 可以加载技能作为行为增强 | ✅ 保留 |

### 7.2 改造

| 现有模块 | 改造方式 | 等级 |
|----------|----------|------|
| `agent/loop.rs` | 降级为 Fallback Path，当 Orchestrator 判定无需深度思考时直接使用 | 🔄 修改 |
| `execution/planner.rs` (LlmPlanner) | Task Decomposer 的底层实现（被 Orchestrator 调用而非独立使用） | 🔄 修改 |
| `intent/classifier.rs` | 扩展到 5 类：`chat / code / research / debug / deep_think` | 🔄 扩展 |

### 7.3 新增

| 新模块 | 位置 |
|--------|------|
| Orchestrator Agent | `orchestrator/` |
| Worker Agents | `workers/` |
| Critic Agent | `critic/` |
| Workspace Context | `workspace/` |
| 前端组件 | `src-ui/src/components/` |

---

## 8. 目录结构变动

```
src-tauri/src/
├── agent/                          # [保留] 降级为 Fallback
│   ├── loop.rs
│   └── ...
├── orchestrator/                   # [新增] Orchestrator Agent
│   ├── mod.rs
│   ├── agent.rs                    # OrchestratorAgent 主实现
│   ├── task_graph.rs               # TaskGraph + TaskNode
│   └── dispatcher.rs               # Worker 调度队列
├── workers/                        # [新增] 所有 Worker 实现
│   ├── mod.rs
│   ├── thinker.rs                  # Thinker Worker (CoT)
│   ├── code_explorer.rs            # Code Explorer Worker
│   ├── code_editor.rs              # Code Editor Worker
│   ├── shell.rs                    # Shell Worker
│   ├── web.rs                      # Web Worker
│   ├── memory.rs                   # Memory Worker
│   └── mcp_bridge.rs               # MCP Bridge Worker
├── critic/                         # [新增] Critic Agent
│   ├── mod.rs
│   └── agent.rs                    # CriticAgent 实现
├── workspace/                      # [新增] Workspace Context
│   ├── mod.rs
│   ├── indexer.rs                  # CodebaseIndexer
│   ├── watcher.rs                  # FileWatcher
│   ├── search.rs                   # SearchEngine (rg/glob/ast)
│   └── git.rs                      # GitIntegration
└── ... (复用现有模块)

src-ui/src/
├── components/
│   ├── ThinkingPanel.tsx            # [新增]
│   ├── TaskGraphView.tsx            # [新增]
│   ├── WorkspaceTree.tsx            # [新增]
│   ├── AgentStatusBar.tsx           # [新增]
│   └── CriticFeedback.tsx           # [新增]
└── ... (复用现有组件)
```

---

## 9. 迁移阶段

### Phase 1: Foundation（基础建设）

**目标**: 新模块可独立构建和测试，不影响现有功能

- [ ] 创建 `orchestrator/` 模块：OrchestratorAgent 骨架 + TaskGraph
- [ ] 创建 `workers/` 模块：Thinker Worker + Code Explorer Worker
- [ ] 创建 `critic/` 模块：CriticAgent 基础实现
- [ ] 创建 `workspace/` 模块：CodebaseIndexer + SearchEngine
- [ ] 全部新模块单元测试通过

### Phase 2: Integration（系统集成）

**目标**: Orchestrator 接管现有 Agent 流程

- [ ] IntentRouter 扩展分类，指向 Orchestrator
- [ ] 现有 `agent/loop.rs` 降级为 Fallback
- [ ] Code Editor Worker + Shell Worker
- [ ] Workspace FileWatcher + GitIntegration
- [ ] 前端 ThinkingPanel + TaskGraphView
- [ ] Web Worker + Memory Worker + MCP Bridge Worker
- [ ] 端到端流程打通

### Phase 3: Optimization（优化完善）

**目标**: 深度思考体验调优

- [ ] Critic 自反思循环调优
- [ ] 三层思维自动路由（Fast/Moderate/Intensive）
- [ ] 思考过程流式展示优化
- [ ] 并行 Worker 执行
- [ ] 索引预热、缓存优化
- [ ] 性能基准测试

---

## 10. 关键决策记录

| 决策 | 选项 | 选择 | 理由 |
|------|------|------|------|
| Agent 框架 | 自建 vs Rig Agent | Rig Agent | Rig 已在项目中，Agent + Tool 原生支持，减少技术债务 |
| Worker 通信 | 共享内存 vs Channel | Channel (mpsc) | 与现有流式架构一致，支持跨 Worker 并行 |
| 任务图 | 顺序 vs DAG | DAG | 支持并行 Worker，复杂任务效率更高 |
| 索引持久化 | 内存 vs SQLite | SQLite | 复用现有 DB 层，重启后索引不丢失 |
| 搜索引擎 | 纯 Rust vs 外部工具 | ripgrep + ast-grep | 性能优于纯 Rust 实现，跨平台支持好 |
| CoT 展示 | 仅结果 vs 流式展示 | 流式展示 | 用户需要看到思考过程，而非黑盒 |

---

## 11. 风险与缓解

| 风险 | 影响 | 缓解 |
|------|------|------|
| 多 Worker 并行导致 LLM token 消耗激增 | 成本 | Layer 分级 + Token Budget 控制 |
| 文件监控误触发频繁重建索引 | 性能 | 去抖(debounce) + 排除噪音目录 |
| Critic 循环可能导致无限重试 | 用户体验 | 最大重试次数(3) + 强制输出 |
| 现有 agent/loop.rs 与 Orchestrator 冲突 | 架构混乱 | 明确降级路径，单元测试覆盖边界 |
| Rig v0.37 Agent 的能力限制 | 扩展性 | Worker 可绕过 Rig，直接调用 provider |

---

## 12. 参考

- [项目概览](../agents/overview.md) — 现有架构
- [OpenSpec 工作流](../agents/openspec-workflow.md) — 功能开发流程
- [Rig v0.37 Agent Documentation](https://docs.rig.rs/) — Worker 实现基础
- Kahneman, D. (2011). *Thinking, Fast and Slow* — 双系统理论
