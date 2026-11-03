# src-tauri 架构梳理与优化方向 — 2026-11-03

> 范围: `src-tauri/` 下 Rust 后端整体代码审查
> 类型: 架构分析 / 优化规划（设计文档）

## 1. 代码规模现状

`src-tauri/src` 下共 **124 个 Rust 文件、约 26.5k 行**（不含 `target/`）。

按文件体量（行数）排序的核心文件：

| 文件 | 行数 | 性质 |
|------|------|------|
| `db/repository.rs` | 1,595 | 上帝仓库：session/message/setting/skill/memory/persona/execution-plan/project 等 CRUD 集中在一个 `impl Database` 中 |
| `toolchain/installer.rs` | 1,000 | 安装引擎主体 |
| `commands/session.rs` | 769 | 会话域 IPC 命令，内嵌大量编排逻辑 |
| `toolchain/*` 管理集群 | 约 2.2k | manager/manager_detector/manager_executor/lifecycle/node_integration/node_toolchain 等协同，集中在 `toolchain/` 域 |
| `orchestrator/runtime.rs` | 659 | 执行运行时：plan 执行 + step + LLM + pipeline + condition |
| `orchestrator/agent.rs` | 533 | 分层 Agent 流程（plan → execute → reflect → synthesize） |
| `workspace/indexer.rs` | 715 | 代码库索引 |
| `pipeline/engine.rs` | 352 | Workflow 引擎 |

结论: 单文件级体量可控，但**职责混合**（god object / fat module）集中在 `db`、`toolchain`、`orchestrator/runtime`、`commands/session` 等少数文件上。

## 2. 当前架构分层

```
src-tauri/src/
├── main.rs / lib.rs        # 双入口: CLI(agent toolchain …) + Tauri(桌面端)
├── state.rs                # AppState 组合根(16+ 子系统)
├── config.rs / error.rs    # AppConfig + AppError
├── commands/               # IPC 层(19 子域, mod.rs 用 pub use * 做 barrel)
├── agent/                  # ToolLoop(LLM 调用 + 工具循环骨架)
├── orchestrator/           # agent/dispatcher/event_bridge/planner/runtime/task_graph/plan_types/pipeline_adapter
├── pipeline/               # WorkflowDef + PipelineEngine
├── api/                    # ProviderBox / RigProvider / util(JSON 抽取/token 估算)
├── tools/                  # AgentTool trait + ToolRegistry
├── workers/                # Thinker/CodeExplorer/CodeEditor/Shell/Web/Memory/MCPBridge
├── db/                     # models + repository(1595 行单 impl)
├── memory/ mcp/ lifecycle/ skills/ persona/ intent/ critic/ project_env/ workspace/ toolchain/ config/ keychain/
└── cli/                    # 独立于 Tauri 的命令行
```

依赖方向(当前): `commands → orchestrator/agent → api → rig; commands → db; commands → lifecycle; commands → toolchain; commands → workers...`

## 3. 主链路数据流

1. **普通会话消息**: `commands::send_message` / `send_message_stream`
   → 组装 `Vec<Message>`(system + persona + memory context + history)→ lifecycle compactor → `ToolLoop::run/run_stream` → `ProviderBox(RigProvider)` → Rig Agent → 写 assistant 消息 → fire-and-forget title/summary。
2. **分层编排**: `commands::orchestrate_message` → `OrchestratorAgent::process_goal` → Planner → TaskGraph → `ExecutionRuntime::execute` → Dispatcher(worker) → Critic → event_bridge → Tauri events。
3. **Workflow**: `commands::run_workflow` → `PipelineEngine` → `PipelineAdapter` → `ExecutionRuntime`。

## 4. 已确认的主要问题

1. **工具循环未接线(高优先级架构缺口)**
   `agent/loop.rs` 的 `ToolLoop::run` 中 `tools_enabled` / `allowed_tools` 参数被 `_` 前缀忽略,实际走 `provider.prompt()`;`run_with_agent` 同样丢弃 `allowed_tools`。
   `ProviderBox` 已预留 `prompt_with_tools` / `stream_with_tools`(内部用 `AgentBuilder.dynamic_tools`),Rig 侧设计也已就绪。说明架构预案已定,但实现切换未完成——**当前主路径不会真正执行工具调用**。

2. **数据访问层过胖**
   `db/repository.rs`(1,595 行)单 `impl Database` 内混多个聚合的 CRUD;事务与错误吞弃(`let _ = db.xxx()`)普遍存在。

3. **命令层内嵌业务逻辑**
   `commands/session.rs` 将消息组装、persona/memory/compactor、ToolLoop 调用、lifecycle 派发全部写在 IPC 命令内;同类 `commands/toolchain.rs`(542 行)也在做较多编排。

4. **执行引擎职责混合**
   `orchestrator/runtime.rs` 同时承担 plan 持久化、step 执行、LLM 调用、pipeline 执行、条件分支、状态写库,内部方法密集。

5. **错误处理偏扁平**
   `AppError` 中 `RigError/Tool/Worker/Critic/Workspace` 等为 `String` 包装;`is_retryable` 为手工穷举 match。

6. **异步/同步边界**
   命令层经 `tokio::Mutex<Database>` 调用同步 rusqlite,存在阻塞风险;执行状态以 JSON 字符串(`status_json`/`plan_json`)落库,类型安全不足。

7. **事件与状态**
   事件桥为每次 orchestration spawn 独立 channel + orchestrator 持全局 `event_tx`(`set_event_tx` 覆盖),并发会话存在串扰风险;Stream 错误以 `Content("\n[Error: …]")` 文本注入。

## 5. 优化路线建议(分阶段)

- **Phase 0 — 收敛(低风险、建议立即启动)**
  1. `db/` 按聚合拆分 repository(保留 Facade),统一错误处理与事务。
  2. 从 `commands/` 抽业务服务层(`SessionService`、`ToolchainService` 等),命令文件只做 IPC 编排。
  3. `AppError` 细化/收敛重试语义;stream 增加结构化错误事件。

- **Phase 1 — 核心引擎打通**
  1. `ExecutionRuntime` 拆分为 executor 子模块(plan/step/llm/pipeline/condition)。
  2. 推进 `ToolLoop` 工具接线(`prompt_with_tools` / `dynamic_tools` / tool result 回填 history)。
  3. 事件系统收敛(per-session channel / EventBus),`emit` 事件名集中管理。

- **Phase 2 — 架构边界与长期演进**
  1. crate 拆分:`agent_core`(不依赖 tauri) + `tauri_app`(commands/state/lib/main 适配层)。
  2. DB schema 迁移抽象 + repository 拆分落地。
  3. 补充集成/单测与事件契约测试。

## 6. 后续动作建议

- 立即:输出本梳理报告;择其优先级启动 Phase 0。
- 若进入功能开发流程,建议走 OpenSpec(`/opsx-propose` → `/opsx-apply`)。

## 7. Deep-dive 第 1 项：ToolLoop 工具接线 — 实施进度（2026-11-03）

### 结论

工具接线断点在三个层级：**前端 ✅ / ToolLoop ❌ / ProviderBox+Rig ❌**，现已精确到 Rig 0.46 源码层面确认。

### 实施进展（按步骤推进）

- [x] Step 0:核实 Rig 0.46 工具闭环 API(`AgentBuilder::dynamic_tools`、`AgentRunner::{run,stream,max_turns,history}`、`MultiTurnStreamItem::{ToolCall,ToolResult,FinalResponse}`、`StreamEvent::{Text,ToolCall,ToolResult}`、`ToolCall{id,function}`、`ToolResult{call,name,content,is_error}`)。
- [x] Step 1:`ProviderBox`(`src-tauri/src/api/provider.rs`)— `prompt_with_tools` 增加 `max_turns`；新增 `chat_with_tools`；`stream_with_tools` 改为接收 `history/tools/max_turns` 并返回 `StreamingResult`（原始 item 流）。
- [x] Step 2:`ToolLoop`(`src-tauri/src/agent/loop.rs`)— `run` 接入 `prompt_with_tools`/`chat_with_tools`（`tools_enabled`/`allowed_tools`/`max_iterations` 生效）；`run_stream` 通过 `stream_with_tools` 消费流并转发 `Content`/`ToolCall`/`ToolResult`/`Done`；`run_with_agent` 接入 `prompt_with_tools`。
- [ ] Step 3:`ExecutionRuntime::run_agent` 经由 `ToolLoop` 调用自然生效(本次改动无需额外修改)。

### 风险 / 待办

1. `MultiTurnStreamItem` 为 `#[non_exhaustive]`，需 `_` 兜底分支（已处理）。
2. 前端 `is_error` 判定（`"Tool execution error:"` 前缀）与 Rig 侧 `ToolResult.is_error` 语义对齐待确认。
3. 取消能力（stream drop / max_turns）后续跟进。

## 8. Deep-dive 第 2 项：ExecutionRuntime 拆分（2026-11-03）

### 背景

`orchestrator/runtime.rs` 原 659 行，`ExecutionRuntime` 同时承担计划编排（execute 主循环、取消/暂停、状态持久化）与步骤级执行（tool/llm/agent/pipeline/condition），职责混合。

### 拆分结果

```
orchestrator/
├── runtime.rs            # ExecutionRuntime 结构 + execute 计划编排 + emit_log + 状态持久化（约 307 行）
└── runtime/
    └── executor.rs       # step 级执行：execute_step / execute_pipeline / execute_tool /
                          # execute_llm / execute_agent_task / run_agent / execute_condition（约 359 行）
```

- `runtime.rs` 顶部新增 `mod executor;`
- 子模块方法标记 `pub(super)`，供父模块 `execute` 调用；私有字段在同一模块树内可访问。
- `cargo check` 通过（0 error / 0 warning），orchestrator 单元测试通过。

### 后续可继续拆分（Phase 1 剩余）

1. `db/repository.rs`（1,595 行）按聚合拆分 repository。
2. `commands/session.rs`（769 行）抽取 `SessionService` 应用服务层。
3. 流式错误事件结构化（`StreamEvent::Error`）。
