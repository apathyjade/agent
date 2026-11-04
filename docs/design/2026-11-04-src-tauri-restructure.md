# src-tauri 源码结构重组设计

**状态：** 待批准
**日期：** 2026-11-04
**范围：** `src-tauri/src/` 的模块布局与依赖方向；不改运行时行为、不改 IPC 名称、不改前端。

---

## 1. 背景

`src-tauri/src/` 顶层现有 **19 个目录 + 6 个散落文件（共 25 项）**，127 个 `.rs` 文件。问题不在文件放错位置，而在：

- **没有分层**：`error.rs` 和 `orchestrator/` 平级，读者无法从目录名判断依赖方向。
- **能力被切成两半**：`skills/` 是服务，`commands/skill.rs` 是它的适配器，二者隔着一个 15 文件的 `commands/` 平铺目录。
- **三处反向依赖**：
  1. `db/repository.rs` 引用 `orchestrator::plan_types` 与 `pipeline::models` 的 DB 记录类型 —— 最底层依赖最上层。
  2. `orchestrator/event_bridge.rs` 引用 `commands::StreamChunk` —— 领域层依赖 Tauri 适配层。
  3. `config.rs` 聚合了 `intent`、`mcp`、`toolchain` 的配置类型 —— 配置层被迫依赖三个域。

## 2. 目标

1. 顶层从 25 项收敛到 **6 个目录 + `lib.rs` + `main.rs`**。
2. 建立单向依赖阶梯：`core → runtime → llm → agent → features → app`。
3. 每个能力域一个目录，服务与它的 Tauri 适配器同域命名（适配器位置见 §8 的偏差说明）。
4. 依赖倒置清零：`db` 不引用任何上层类型；`agent` 不引用 `app`。

**非目标：**

- 不拆分超大文件（`app/commands/session.rs` 28KB、`features/skills/mod.rs` 43KB、`core/db/repository.rs` 60KB）—— 独立任务。
- 不改任何 Tauri 命令名、参数、返回结构；前端零改动。
- 不改任何业务逻辑、错误文案、SQL、CLI 调用参数。
- 不引入新依赖，不改 `Cargo.toml`。

## 3. 目标结构

```
src-tauri/src/
├── main.rs
├── lib.rs
├── core/                       # 第 1 层：无内部依赖
│   ├── mod.rs
│   ├── error.rs                # ← error.rs
│   ├── keychain.rs             # ← keychain.rs
│   ├── ipc.rs                  # 新：StreamChunk, ToolCallEvent（从 commands/mod.rs 迁出）
│   └── db/
│       ├── mod.rs              # ← db/mod.rs
│       ├── models.rs           # ← db/models.rs（+ 3 个 DB 记录类型下沉）
│       └── repository.rs       # ← db/repository.rs
├── runtime/                    # 第 2 层：本地运行时引擎，独立于 app
│   ├── mod.rs
│   ├── toolchain/              # ← toolchain/（内部结构不变）
│   └── project_env/            # ← project_env/
├── llm/                        # 第 3 层：模型接入与意图路由
│   ├── mod.rs
│   ├── model.rs                # 新：ModelProvider, ModelConfig（从 config.rs 迁出）
│   ├── catalog.rs              # 新：ModelInfo, PROVIDER_OPTIONS, PROVIDER_MODELS（从 commands/mod.rs 迁出）
│   ├── provider.rs             # ← api/provider.rs
│   ├── rig.rs                  # ← api/rig.rs
│   ├── util.rs                 # ← api/util.rs
│   └── intent/                 # ← intent/
│       ├── mod.rs
│       ├── classifier.rs
│       └── router.rs
├── agent/                      # 第 4 层：Agent 运行时
│   ├── mod.rs
│   ├── fast_loop.rs            # ← agent/loop.rs（去掉 r#loop）
│   ├── tools/                  # ← tools/
│   │   ├── mod.rs
│   │   ├── base.rs             # ← tools/trait.rs（去掉 r#trait）
│   │   └── …                   # calculator, code_executor, file_system, registry, run_workflow, web_search
│   ├── workers/                # ← workers/
│   ├── critic/                 # ← critic/
│   └── orchestrator/           # ← orchestrator/
│       ├── mod.rs, agent.rs, dispatcher.rs, event_bridge.rs, pipeline_adapter.rs,
│       ├── plan_error.rs, plan_types.rs, planner.rs, task_graph.rs,
│       └── runtime.rs, runtime/executor.rs
├── features/                   # 第 5 层：业务能力
│   ├── mod.rs
│   ├── session/                # 会话域
│   │   ├── mod.rs
│   │   └── lifecycle/          # ← lifecycle/（config, titler, summarizer, compactor, archiver）
│   ├── memory/                 # ← memory/
│   ├── persona/                # ← persona/
│   ├── skills/                 # ← skills/
│   ├── mcp/                    # ← mcp/
│   ├── workspace/              # ← workspace/
│   └── workflow/               # ← pipeline/（engine, models, scanner）
└── app/                        # 第 6 层：组合根与 Tauri 适配层
    ├── mod.rs
    ├── config.rs               # ← config.rs（AppConfig + load/save + runtime_options）
    ├── cli/                    # ← cli/（CLI 前端，组合 config + db + runtime）
    │   ├── mod.rs
    │   └── toolchain.rs
    ├── state.rs                # ← state.rs
    └── commands/               # ← commands/（按域命名，不再平铺在顶层）
        ├── mod.rs
        ├── session.rs          # ← commands/session.rs
        ├── lifecycle.rs        # ← commands/lifecycle.rs
        ├── project.rs          # ← commands/project.rs
        ├── execution.rs        # ← commands/execution.rs
        ├── orchestrate.rs      # ← commands/orchestrator.rs
        ├── prompt.rs           # ← commands/prompt.rs
        ├── memory.rs           # ← commands/memory.rs
        ├── persona.rs          # ← commands/persona.rs
        ├── skills.rs           # ← commands/skill.rs
        ├── mcp.rs              # ← commands/mcp.rs
        ├── workflow.rs         # ← commands/pipeline.rs
        ├── toolchain.rs        # ← commands/toolchain.rs
        ├── project_env.rs      # ← commands/project_env.rs
        ├── model.rs            # ← commands/model.rs
        ├── provider.rs         # ← commands/provider.rs
        ├── settings.rs         # ← commands/settings.rs
        ├── tool.rs             # ← commands/tool.rs
        └── window.rs           # ← commands/window.rs
```

## 4. 依赖规则

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

**已记录例外（3 处，均为"Agent 运行时调用具体能力"，允许保留）：**

| 文件 | 引用 | 理由 |
|---|---|---|
| `agent/orchestrator/pipeline_adapter.rs` | `features::workflow` | 把 workflow 定义转换成执行计划，是 orchestrator 的适配器 |
| `agent/tools/run_workflow.rs` | `features::workflow` | 工具直接驱动工作流引擎 |
| `agent/workers/code_explorer.rs` | `features::workspace::search` | 代码探索 Worker 使用工作区索引 |

> 这三处如果日后要清零，做法是把对应实现移入 `features/` 并由 `app/state.rs` 注入；本次不做，避免把结构重组变成行为重构。

**硬性约束（可自动检查，`#[cfg(test)]` 代码同样适用）：**

- `core/` 下不得出现 `crate::llm|agent|features|app|runtime`。
- `db/` 下不得出现 `crate::orchestrator|pipeline`（类型已下沉）。
- `app/` 之外不得出现 `crate::app`（含 `crate::app::config`、`crate::app::commands`、`crate::app::state`）。唯一例外：crate 根 `lib.rs` 作为组合根自身的 `use crate::app::…`——它不在任何层内，是装配点。
- `llm/` 下不得出现 `crate::app`。

## 5. 类型归位

### 5.1 配置拆分

`config.rs` 现在同时装了三样东西，按归属拆开：

| 类型 | 新位置 | 理由 |
|---|---|---|
| `ModelProvider`, `ModelConfig`（含 `default_base_url` 等 3 个方法）+ 自由函数 `default_model(&[ModelConfig])` | `llm/model.rs` | 模型接入域的类型；`llm` 与 `app` 都用 |
| `AppConfig`（含 `runtime_options()`、`load`/`save`、模型增删查方法） | `app/config.rs` | 桌面应用的持久化设置聚合，天然属于组合根；模型查询方法委托 `llm::model::default_model` |
| `McpServerConfig`, `StartupPolicy`, `ToolConfig`, `ConfirmationMode`, `ConnectionStatus` | 留在 `features/mcp/config.rs` | 含 `effective_runtime()`，依赖 `runtime::toolchain::RuntimeType` |
| `IntentConfig` | 留在 `llm/intent/mod.rs` | 意图路由域的类型 |

结果：`app/config.rs` 只向下引用 `llm::model`、`llm::intent`、`features::mcp::config`；`core` 不再需要知道任何配置结构。`cli` 因为要 `AppConfig::load()`，整体归入 `app/cli/`（它本来就是二进制入口的组合根）。

### 5.2 DB 记录类型下沉

| 类型 | 现在 | 新位置 | 原位置处理 |
|---|---|---|---|
| `ExecutionPlanRecord`, `PlanStepRecord` | `orchestrator/plan_types.rs` | `core/db/models.rs` | `plan_types.rs` 保留 `pub use` 再导出 |
| `WorkflowRunRecord` | `pipeline/models.rs` | `core/db/models.rs` | `features/workflow/models.rs` 保留 `pub use` 再导出 |

再导出让调用点零改动，同时让 `db` 变成叶子。

### 5.3 IPC DTO 下沉

| 类型 | 现在 | 新位置 |
|---|---|---|
| `StreamChunk`, `ToolCallEvent` | `commands/mod.rs` | `core/ipc.rs` |
| `ModelInfo`, `PROVIDER_OPTIONS`, `PROVIDER_MODELS` | `commands/mod.rs` | `llm/catalog.rs` |

`app/commands/mod.rs` 保留再导出，`lib.rs` 的 handler 列表只改模块路径。`orchestrator/event_bridge.rs` 改为引用 `core::ipc`，反向依赖消失。

### 5.4 去 `r#` 重命名

| 现在 | 新名 | 引用数 |
|---|---|---|
| `agent::r#loop` | `agent::fast_loop` | 11 |
| `tools::r#trait` | `agent::tools::base` | 12 |

纯重命名，无逻辑变化。

### 5.5 `ProviderRegistry` 与 `AppConfig` 解耦

`llm/provider.rs` 目前是 `llm` 唯一依赖 `AppConfig` 的生产代码（`new(&AppConfig)` + `config.get_default_model()`）。把 `AppConfig` 挪到 `app/` 后会形成 `llm → app`，必须同时改签名：

```rust
// llm/model.rs
pub fn default_model(models: &[ModelConfig]) -> Option<&ModelConfig>;

// llm/provider.rs
impl ProviderRegistry {
    pub fn new(models: &[ModelConfig]) -> Self;   // 内部调用 llm::model::default_model
    pub fn add_model(&mut self, model: ModelConfig);
}

// app/config.rs
impl AppConfig {
    pub fn get_default_model(&self) -> Option<&ModelConfig> {
        crate::llm::model::default_model(&self.models)
    }
}

// app/state.rs
let providers = ProviderRegistry::new(&config.models);
```

语义保持不变（`is_default && enabled` 优先，否则第一个 `enabled`）。`AppConfig` 的 JSON 形状不变，`commands/model.rs`、`commands/provider.rs`、`commands/session.rs` 的调用点不变。

**测试代码同样适用依赖规则**：`agent/` 下 8 个测试模块用 `ProviderRegistry::new(&AppConfig::default())` 只是要一个空注册表，改为 `ProviderRegistry::new(&[])` 即可，不再引用 `app`。

## 6. 与初选方案 B 的偏差（已定，批准时可推翻）

方案 B 写的是"命令随能力走"（`features/skills/commands.rs`）。落地时发现会制造**新的反向依赖**：Tauri handler 的签名是 `State<'_, AppState>`，而 `AppState` 聚合了全部管理器、位于组合根。把 handler 放进 `features/` 就等于让 `features → app`，与本次目标相反。

因此调整为：**服务在 `features/`，handler 统一放 `app/commands/<域>.rs`**，文件名与域一一对应。收益保留（按域找代码），代价是新增能力要落两个文件（`features/x/` + `app/commands/x.rs`），换来依赖方向单一。

## 7. 迁移波次

每一波是一个独立 commit，波末跑门禁；失败只回滚当前波。

| 波 | 内容 | 门禁 |
|---|---|---|
| 0 | `git switch -c refactor/src-restructure`；记录基线：`cargo test` 用例数、`cargo clippy --all-targets` 警告数（当前 88） | 基线可复现 |
| 1 | 建 `core/`：移动 `error`、`keychain`、`db`；新建 `core/ipc.rs`；下沉 3 个 DB 记录类型；全量改写 `crate::error`/`crate::keychain`/`crate::db` 路径 | `cargo check --all-targets` |
| 2 | 建 `runtime/`：移动 `toolchain`、`project_env`；改写路径 | `cargo check --all-targets` |
| 3 | 建 `llm/`：移动 `api`、`intent`；拆出 `llm/model.rs`、`llm/catalog.rs`；按 §5.5 改 `ProviderRegistry`；改写路径 | `cargo check --all-targets` + `cargo test` |
| 4 | 建 `agent/`：移动 `agent/loop`、`tools`、`workers`、`critic`、`orchestrator`；执行 §5.4 重命名；测试改用 `ProviderRegistry::new(&[])` | `cargo check --all-targets` |
| 5 | 建 `features/`：移动 `memory`、`persona`、`skills`、`mcp`、`workspace`、`pipeline→workflow`、`lifecycle→session/lifecycle` | `cargo check --all-targets` + `cargo test` |
| 6 | 建 `app/`：移动 `config`、`cli`、`state`；`commands/` 整体迁入并按 §3 改名；改写 `lib.rs` handler 列表 | `cargo check --all-targets` |
| 7 | 收尾：`lib.rs` 模块声明收敛为 6 个；更新 `main.rs`、`src-tauri/tests/*`、文档 | 全量验证 |

**路径改写方式：** 逐波用脚本做前缀替换（`crate::db` → `crate::core::db` 等），不手工逐文件改；替换顺序从最深的模块开始，避免 `crate::agent::tools` 被后续规则二次命中。

**格式处理：** 全程**不运行** `cargo fmt`。仓库当前不是 rustfmt-clean（73/127 个文件失败，`rustfmt.toml` 为 `max_width=100`），且 CI 工作流只监听 `main` 分支而仓库用的是 `master`，所以格式化与 CI 修复是独立任务。移动文件用 `git mv`，除路径改写外内容逐字节不变，`git diff -w` 可核对。

## 8. 验证

**每波：** `cargo check --all-targets`。波 3、4、6 追加 `cargo clippy --all-targets` 并对比警告数（基线 88，只允许持平或下降）。

**波 3、5、7：** 追加 `cargo test`（基线 199 + 3 + 7 + 11 + 3 + 38）。

**收尾必须全绿：**

```bash
cd src-tauri
cargo check --all-targets
cargo clippy --all-targets          # 警告数 ≤ 88
cargo test
cargo run -- toolchain status      # CLI 路径可用（debug 构建）
```

```bash
cd src-ui
npx tsc --noEmit
npx vitest run
npm run build
```

前端三项是为了证明"零前端改动"确实没被打破（预期输出与重组前一致）。

**不设 `cargo fmt` 门禁**：仓库基线就不是 rustfmt-clean（73/127 文件失败），且 CI 工作流监听的是 `main` 而仓库分支为 `master`，格式化与 CI 修复属于独立任务（见 §7 格式处理）。

**结构验收（人工 + grep）：**

- `src/` 顶层只有 `app core features agent llm runtime lib.rs main.rs`。
- `grep -r "r#loop\|r#trait" src-tauri/src` 无结果。
- `grep -r "crate::app" src-tauri/src --include=*.rs` 唯一命中 `src-tauri/src/lib.rs`（组合根自身导入），其余无结果。
- `grep -r "crate::orchestrator\|crate::pipeline" src-tauri/src/core/db` 无结果。
- `git log --follow` 能追到移动后的文件历史。

## 9. 风险与回滚

| 风险 | 缓解 |
|---|---|
| 130 处路径漏改导致编译失败 | 每波 `cargo check --all-targets`；编译器会全量报错，不会静默 |
| `tauri::generate_handler!` 里 130+ 路径写错 | 波 6 单独提交，失败只回滚该 commit |
| 移动文件丢失 git 历史 | 全部用 `git mv`，不删后重建；不改格式，rename 检测更稳 |
| clippy 警告增加 | 每波对比基线 88，只允许持平或下降 |
| 与正在进行的其他分支冲突 | 当前工作区干净、无并行分支；重组期间不开新功能分支 |

回滚粒度：单波 commit `git revert`；整体 `git reset --hard <wave-0>`。

## 10. 文档同步（波 7 内完成）

| 文件 | 需要改的内容 |
|---|---|
| `AGENTS.md` | 速查表的路径：`commands/` → `app/commands/`、`tools/` → `agent/tools/`、`config.rs` → `app/config.rs` + `llm/model.rs`、`toolchain/registry.rs` → `runtime/toolchain/registry.rs`、`memory/seeds.rs` → `features/memory/seeds.rs` |
| `docs/agents/overview.md` | 目录结构树替换为新结构，补一节"模块分层与依赖规则" |
| `docs/agents/traps.md` | 若引用了旧路径，同步替换 |
| `docs/agents/workflow.md` | 若提到 `src-tauri/src/commands`，同步替换 |

## 11. 完成定义

- [ ] `src/` 顶层为 6 目录 + 2 文件。
- [ ] 依赖阶梯成立，仅保留 §4 的 3 处例外；`#[cfg(test)]` 代码同样满足。
- [ ] `cargo check --all-targets`、`cargo test` 全绿；`cargo clippy --all-targets` 警告数 ≤ 88。
- [ ] `npx tsc --noEmit`、`npx vitest run`、`npm run build` 结果与重组前一致。
- [ ] `cargo run -- toolchain status` 可用。
- [ ] 文档 4 处路径同步完成。
- [ ] 无业务逻辑、IPC 名称、前端改动（`git diff --stat` 中 `src-ui/` 为空）。
