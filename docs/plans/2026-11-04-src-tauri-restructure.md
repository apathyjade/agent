# src-tauri 源码结构重组 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 `src-tauri/src` 从 25 个顶层项重组为 `app / core / llm / agent / features / runtime` 六层，并让依赖方向单向。

**Architecture:** 纯文件移动 + 路径改写 + 3 处类型下沉，不改业务逻辑。分 7 波，每波一个 commit；每波以 `cargo check --all-targets` 通过为门禁，波 3/5/7 追加 `cargo test`。现有 199 个内联测试 + 62 个集成测试是回归网，本次不新增行为测试。

**Tech Stack:** Rust 1.77.2（edition 2021）、Tauri 2、Cargo、PowerShell 5.1 改写脚本。

**Spec:** `docs/design/2026-11-04-src-tauri-restructure.md`

## Global Constraints

- 不改任何 Tauri 命令名、参数、返回结构；`src-ui/` 零改动。
- 不改业务逻辑、错误文案、SQL、CLI 调用参数、Tauri 事件名（`stream_chunk`、`plan_progress` 等）。
- 不新增依赖，不改 `src-tauri/Cargo.toml`。
- **不运行 `cargo fmt`**（仓库基线不是 rustfmt-clean，见 spec §7）。
- 所有移动用 `git mv`，禁止删除后重建。
- 源码统一 UTF-8 无 BOM，工作区行尾 CRLF；改写脚本必须原样保留编码与行尾。
- 每波独立 commit；波末 `cargo check --all-targets` 必须通过。
- `cargo clippy --all-targets` 警告数 ≤ 88（基线）。
- 依赖规则对 `#[cfg(test)]` 代码同样生效。
- **所有命令从仓库根目录执行**，不使用 `cd`：cargo 一律带 `--manifest-path src-tauri/Cargo.toml`，文件路径一律带 `src-tauri/` 前缀。

## Review Focus

1. **`use super::*` 在目录整体搬迁后的解析**：31 处测试模块依赖 `super`，若某文件被单独移动而非随目录移动，`super` 会指向错误父模块且编译不报错。→ Task 1/4/5 的移动步骤一律按目录整体 `git mv`，不用单文件搬运。
2. **Tauri 事件名与 payload 不变**：前端订阅 `stream_chunk`/`plan_progress`/`install_progress` 等事件；改名即静默破坏 UI。→ Task 6 提交前核对 `emit(` 调用的事件名清单。
3. **`db` 记录类型的 `pub use` 再导出**：`ExecutionPlanRecord`/`PlanStepRecord`/`WorkflowRunRecord` 下沉后原位置必须保留再导出，否则 `commands/*` 与 `repository.rs` 导入断裂。→ Task 1 的 Step 8 明确写入再导出，Step 9 由 `cargo check` 兜底。
4. **`tauri::generate_handler!` 的 130 个路径**：宏展开失败会报 `cannot find function`，但报错量大、容易漏改。→ Task 6 用脚本批量改写 + Step 8 单独核对 handler 列表。
5. **Windows 专用分支的路径拼接**：`runtime/toolchain` 内有 `#[cfg(windows)]` 的 cmd shim 逻辑与 `dirs` 路径；移动目录不得改变任何路径常量。→ Task 2 用 `git diff -w --stat` 确认移动文件除 import 行外无内容变化。

---

## Task 1: 建立 `core/` 层并下沉共享类型

**Files:**
- Create: `scripts/rewrite-crate-paths.ps1`
- Move: `src-tauri/src/error.rs` → `src-tauri/src/core/error.rs`
- Move: `src-tauri/src/keychain.rs` → `src-tauri/src/core/keychain.rs`
- Move: `src-tauri/src/db/` → `src-tauri/src/core/db/`
- Create: `src-tauri/src/core/mod.rs`
- Create: `src-tauri/src/core/ipc.rs`
- Modify: `src-tauri/src/commands/mod.rs`（切出 2 个 struct）
- Modify: `src-tauri/src/orchestrator/plan_types.rs`（切出 2 个 struct）
- Modify: `src-tauri/src/pipeline/models.rs`（切出 1 个 struct）
- Modify: `src-tauri/src/core/db/models.rs`（接收 3 个 struct）
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Produces: `crate::core::error::{AppError, Result}`、`crate::core::keychain`、`crate::core::db::{models, repository}`、`crate::core::ipc::{StreamChunk, ToolCallEvent}`。
- Produces: `crate::core::db::models::{ExecutionPlanRecord, PlanStepRecord, WorkflowRunRecord}`，并由原模块 `pub use` 再导出。

- [ ] **Step 1: 确认分支并记录基线**

```bash
git rev-parse --abbrev-ref HEAD
cargo test --manifest-path src-tauri/Cargo.toml 2>&1 | Select-String 'test result:'
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets 2>&1 | Select-String '^(warning|error)' | Measure-Object
```

Expected: 当前分支已是 `refactor/src-restructure`（**不要新建分支**）；测试 `199 passed` + 集成 `3 / 7 / 11 / 3 / 38 passed`；clippy `Count : 88`。

- [ ] **Step 2: 写路径改写脚本**

Create `scripts/rewrite-crate-paths.ps1`:

```powershell
param(
    [Parameter(Mandatory = $true)][string[]]$Map,
    [string[]]$Roots = @('src-tauri/src', 'src-tauri/tests')
)
$ErrorActionPreference = 'Stop'
$utf8 = New-Object System.Text.UTF8Encoding($false)
$pairs = foreach ($entry in $Map) {
    $i = $entry.IndexOf('=')
    [pscustomobject]@{ From = $entry.Substring(0, $i); To = $entry.Substring($i + 1) }
}
foreach ($root in $Roots) {
    Get-ChildItem -Path $root -Recurse -Filter *.rs -File | ForEach-Object {
        $path = $_.FullName
        $text = [System.IO.File]::ReadAllText($path, $utf8)
        $new = $text
        foreach ($p in $pairs) {
            $new = [regex]::Replace($new, [regex]::Escape($p.From) + '(?![\w])', $p.To)
        }
        if ($new -ne $text) { [System.IO.File]::WriteAllText($path, $new, $utf8) }
    }
}
```

- [ ] **Step 3: 移动文件**

```bash
New-Item -ItemType Directory -Force src-tauri/src/core | Out-Null
git mv src-tauri/src/error.rs src-tauri/src/core/error.rs
git mv src-tauri/src/keychain.rs src-tauri/src/core/keychain.rs
git mv src-tauri/src/db src-tauri/src/core/db
```

- [ ] **Step 4: 观察预期的编译失败**

```bash
cargo check --manifest-path src-tauri/Cargo.toml --all-targets 2>&1 | Select-String 'E0583|E0432' | Select-Object -First 5
```

Expected: `error[E0583]: file not found for module 'error'`（`lib.rs` 仍声明 `pub mod error;`）。

- [ ] **Step 5: 改写路径**

```bash
powershell -NoProfile -File .\scripts\rewrite-crate-paths.ps1 -Map 'crate::error=crate::core::error','crate::keychain=crate::core::keychain','crate::db=crate::core::db'
```

- [ ] **Step 6: 更新 `lib.rs` 模块声明**

删除 `pub mod db;`、`pub mod error;`、`pub mod keychain;`，新增 `pub mod core;`（保持在字母序位置）。

- [ ] **Step 7: 切出 IPC DTO 到 `core/ipc.rs`**

把 `src-tauri/src/commands/mod.rs` 的 `pub struct StreamChunk` 与 `pub struct ToolCallEvent`（含全部 derive 与字段）原样剪切到新建的 `src-tauri/src/core/ipc.rs`，文件头补 `use serde::{Deserialize, Serialize};`。在 `commands/mod.rs` 原位置替换为：

```rust
pub use crate::core::ipc::{StreamChunk, ToolCallEvent};
```

Create `src-tauri/src/core/mod.rs`:

```rust
pub mod db;
pub mod error;
pub mod ipc;
pub mod keychain;
```

- [ ] **Step 8: 下沉 3 个 DB 记录类型**

把 `orchestrator/plan_types.rs` 的 `pub struct ExecutionPlanRecord` 与 `pub struct PlanStepRecord`（含 derive 与注释）、`pipeline/models.rs` 的 `pub struct WorkflowRunRecord`（含 derive 与注释）原样剪切到 `core/db/models.rs` 末尾。原位置各替换为：

```rust
pub use crate::core::db::models::{ExecutionPlanRecord, PlanStepRecord};   // plan_types.rs
pub use crate::core::db::models::WorkflowRunRecord;                       // pipeline/models.rs
```

删除原文件里因此不再使用的 `use`（由编译器提示）。

- [ ] **Step 9: 编译并跑测试**

```bash
cargo check --manifest-path src-tauri/Cargo.toml --all-targets
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: `cargo check` 无错误；测试数量与 Step 1 一致。

- [ ] **Step 10: 提交**

```bash
git add -A
git commit -m "refactor(core): extract core layer and sink shared types"
```

---

## Task 2: 建立 `runtime/` 层

**Files:**
- Move: `src-tauri/src/toolchain/` → `src-tauri/src/runtime/toolchain/`（23 文件）
- Move: `src-tauri/src/project_env/` → `src-tauri/src/runtime/project_env/`（3 文件）
- Create: `src-tauri/src/runtime/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/tests/project_detector_tests.rs`
- Modify: `src-tauri/tests/toolchain_tests.rs`
- Modify: `src-tauri/tests/toolchain_cli_tests.rs`

**Interfaces:**
- Consumes: `crate::core::error::Result`。
- Produces: `crate::runtime::toolchain::*`、`crate::runtime::project_env::*`（内部 API 不变）。

- [ ] **Step 1: 移动目录**

```bash
New-Item -ItemType Directory -Force src-tauri/src/runtime | Out-Null
git mv src-tauri/src/toolchain src-tauri/src/runtime/toolchain
git mv src-tauri/src/project_env src-tauri/src/runtime/project_env
```

- [ ] **Step 2: 观察预期的编译失败**

```bash
cargo check --manifest-path src-tauri/Cargo.toml --all-targets 2>&1 | Select-String 'E0583|E0432|E0433' | Select-Object -First 5
```

Expected: `file not found for module 'toolchain'`。

- [ ] **Step 3: 改写路径**

```bash
powershell -NoProfile -File .\scripts\rewrite-crate-paths.ps1 -Map 'crate::toolchain=crate::runtime::toolchain','crate::project_env=crate::runtime::project_env','agent_lib::toolchain=agent_lib::runtime::toolchain','agent_lib::project_env=agent_lib::runtime::project_env'
```

- [ ] **Step 4: 更新 `lib.rs` 与新建 `runtime/mod.rs`**

删除 `pub mod project_env;`、`pub mod toolchain;`，新增 `pub mod runtime;`。

Create `src-tauri/src/runtime/mod.rs`:

```rust
pub mod project_env;
pub mod toolchain;
```

- [ ] **Step 5: 编译并确认内容未变**

```bash
cargo check --manifest-path src-tauri/Cargo.toml --all-targets
git diff -w --stat HEAD -- src-tauri/src/runtime
```

Expected: `cargo check` 通过；diff 只包含 import 行（`crate::` 前缀变化），无逻辑改动。

- [ ] **Step 6: 提交**

```bash
git add -A
git commit -m "refactor(runtime): move toolchain and project_env into runtime layer"
```

---

## Task 3: 建立 `llm/` 层，拆出 model 与 catalog

**Files:**
- Move: `src-tauri/src/api/{mod,provider,rig,util}.rs` → `src-tauri/src/llm/`
- Move: `src-tauri/src/intent/` → `src-tauri/src/llm/intent/`
- Create: `src-tauri/src/llm/model.rs`（从 `config.rs` 切出 `ModelProvider`、`ModelConfig` 及其 impl，新增 `default_model`）
- Create: `src-tauri/src/llm/catalog.rs`（从 `commands/mod.rs` 切出 `ModelInfo`、`PROVIDER_OPTIONS`、`PROVIDER_MODELS`、`impl From<&ModelConfig> for ModelInfo`）
- Modify: `src-tauri/src/config.rs`
- Modify: `src-tauri/src/llm/provider.rs`（原 `api/provider.rs`）
- Modify: `src-tauri/src/state.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/commands/provider.rs`
- Modify: `src-tauri/tests/llm_tests.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Produces: `crate::llm::model::{ModelProvider, ModelConfig, default_model}`。
- Produces: `pub fn ProviderRegistry::new(models: &[ModelConfig]) -> Self`。
- Produces: `crate::llm::catalog::{ModelInfo, PROVIDER_OPTIONS, PROVIDER_MODELS}`。
- Consumes: `crate::core::error::{AppError, Result}`。

- [ ] **Step 1: 移动目录与文件**

```bash
New-Item -ItemType Directory -Force src-tauri/src/llm | Out-Null
git mv src-tauri/src/api/mod.rs src-tauri/src/llm/mod.rs
git mv src-tauri/src/api/provider.rs src-tauri/src/llm/provider.rs
git mv src-tauri/src/api/rig.rs src-tauri/src/llm/rig.rs
git mv src-tauri/src/api/util.rs src-tauri/src/llm/util.rs
git mv src-tauri/src/intent src-tauri/src/llm/intent
```

- [ ] **Step 2: 观察预期的编译失败**

```bash
cargo check --manifest-path src-tauri/Cargo.toml --all-targets 2>&1 | Select-String 'E0583|E0432|E0433' | Select-Object -First 5
```

Expected: `file not found for module 'api'`。

- [ ] **Step 3: 切出 `llm/model.rs`**

把 `config.rs` 的 `ModelProvider`（含 `Display`、`FromStr`）、`ModelConfig`（含 3 个 impl 方法）原样移到 `src-tauri/src/llm/model.rs`，并新增：

```rust
/// Default model: first `is_default && enabled`, else first `enabled`.
pub fn default_model(models: &[ModelConfig]) -> Option<&ModelConfig> {
    models
        .iter()
        .find(|m| m.is_default && m.enabled)
        .or_else(|| models.iter().find(|m| m.enabled))
}
```

`config.rs` 顶部补 `use crate::llm::model::{ModelConfig, ModelProvider};`，并把 `get_default_model` 改为委托：

```rust
pub fn get_default_model(&self) -> Option<&ModelConfig> {
    crate::llm::model::default_model(&self.models)
}
```

- [ ] **Step 4: 切出 `llm/catalog.rs` 并改 `ProviderRegistry`**

把 `commands/mod.rs` 的 `ModelInfo`、`impl From<&crate::config::ModelConfig> for ModelInfo`、`PROVIDER_OPTIONS`、`PROVIDER_MODELS` 原样移到 `src-tauri/src/llm/catalog.rs`（`impl From` 里的路径由 Step 5 的脚本改写）。`commands/mod.rs` 原位置替换为：

```rust
pub use crate::llm::catalog::{ModelInfo, PROVIDER_OPTIONS, PROVIDER_MODELS};
```

`llm/provider.rs`：删除 `use crate::config::AppConfig;`，补 `use crate::llm::model::ModelConfig;`，改签名为（语义不变）：

```rust
pub fn new(models: &[ModelConfig]) -> Self   // 遍历 models；default_id 用 crate::llm::model::default_model(models)
pub fn add_model(&mut self, model: ModelConfig)
```

`state.rs`：`ProviderRegistry::new(&config.models)`；`use crate::config::{AppConfig, ModelProvider};` 拆成 `use crate::config::AppConfig;` 与 `use crate::llm::model::ModelProvider;`。

- [ ] **Step 4b: 改掉 13 处测试构造（必须在本任务内完成）**

签名改成 `&[ModelConfig]` 后，`#[cfg(test)]` 里的 13 处 `ProviderRegistry::new(&crate::config::AppConfig::default())` 会让本任务的 `cargo check --all-targets` 直接失败，所以在 Task 3 内改完：

```bash
Get-ChildItem -Recurse src-tauri/src -Filter *.rs | ForEach-Object {
  $p = $_.FullName
  $t = [System.IO.File]::ReadAllText($p)
  $n = [regex]::Replace($t, 'ProviderRegistry::new\(\s*&crate::config::AppConfig::default\(\),?\s*\)', 'ProviderRegistry::new(&[])')
  if ($n -ne $t) { [System.IO.File]::WriteAllText($p, $n, (New-Object System.Text.UTF8Encoding($false))) }
}
```

涉及文件（13 处）：`critic/agent.rs`、`orchestrator/agent.rs`（2 处）、`workers/{code_editor,code_explorer,mcp_bridge×6,shell,thinker}.rs`。`state.rs:51` 的 `ProviderRegistry::new(&config)` 不在本步范围，由 Step 4 单独改。

- [ ] **Step 5: 改写路径**

```bash
powershell -NoProfile -File .\scripts\rewrite-crate-paths.ps1 -Map 'crate::config::ModelConfig=crate::llm::model::ModelConfig','crate::config::ModelProvider=crate::llm::model::ModelProvider','crate::api=crate::llm','crate::intent=crate::llm::intent','agent_lib::api=agent_lib::llm'
```

- [ ] **Step 6: 手工修正 3 处 grouped import**

- `src-tauri/src/llm/rig.rs`：`use crate::config::{ModelConfig, ModelProvider};` → `use crate::llm::model::{ModelConfig, ModelProvider};`
- `src-tauri/src/commands/provider.rs`：同上
- `src-tauri/tests/llm_tests.rs`：`use agent_lib::config::{ModelConfig, ModelProvider};` → `use agent_lib::llm::model::{ModelConfig, ModelProvider};`

- [ ] **Step 7: 更新 `llm/mod.rs` 与 `lib.rs`**

`src-tauri/src/llm/mod.rs` 内容改为：

```rust
pub mod catalog;
pub mod intent;
pub mod model;
pub mod provider;
pub mod rig;
pub mod util;
```

`lib.rs` 删除 `pub mod api;`、`pub mod intent;`，新增 `pub mod llm;`。

- [ ] **Step 8: 编译并跑测试**

```bash
cargo check --manifest-path src-tauri/Cargo.toml --all-targets
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: 全绿；`llm_tests` 5 个用例通过。

- [ ] **Step 9: 提交**

```bash
git add -A
git commit -m "refactor(llm): move api and intent into llm layer"
```

---

## Task 4: 建立 `agent/` 层并去掉 `r#` 模块名

**Files:**
- Move: `src-tauri/src/agent/loop.rs` → `src-tauri/src/agent/fast_loop.rs`
- Move: `src-tauri/src/tools/` → `src-tauri/src/agent/tools/`，其中 `trait.rs` → `base.rs`
- Move: `src-tauri/src/workers/` → `src-tauri/src/agent/workers/`
- Move: `src-tauri/src/critic/` → `src-tauri/src/agent/critic/`
- Move: `src-tauri/src/orchestrator/` → `src-tauri/src/agent/orchestrator/`
- Modify: `src-tauri/src/agent/mod.rs`
- Modify: `src-tauri/src/agent/tools/mod.rs`
- Modify: 13 处测试构造（见 Step 5）
- Modify: `src-tauri/tests/tool_calculator.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Produces: `crate::agent::fast_loop`（原 `crate::agent::r#loop`）。
- Produces: `crate::agent::tools::base::{AgentTool, ToolInfo}`（原 `tools::r#trait`）。
- Produces: `crate::agent::{tools, workers, critic, orchestrator}`。

- [ ] **Step 1: 移动目录与文件**

```bash
git mv src-tauri/src/agent/loop.rs src-tauri/src/agent/fast_loop.rs
git mv src-tauri/src/tools src-tauri/src/agent/tools
git mv src-tauri/src/agent/tools/trait.rs src-tauri/src/agent/tools/base.rs
git mv src-tauri/src/workers src-tauri/src/agent/workers
git mv src-tauri/src/critic src-tauri/src/agent/critic
git mv src-tauri/src/orchestrator src-tauri/src/agent/orchestrator
```

- [ ] **Step 2: 观察预期的编译失败**

```bash
cargo check --manifest-path src-tauri/Cargo.toml --all-targets 2>&1 | Select-String 'E0583|E0432|E0433' | Select-Object -First 5
```

Expected: `file not found for module 'tools'`。

- [ ] **Step 3: 改写路径（顺序不可调换）**

```bash
powershell -NoProfile -File .\scripts\rewrite-crate-paths.ps1 -Map 'crate::agent::r#loop=crate::agent::fast_loop','crate::tools::r#trait=crate::agent::tools::base','crate::tools=crate::agent::tools','crate::workers=crate::agent::workers','crate::critic=crate::agent::critic','crate::orchestrator=crate::agent::orchestrator','agent_lib::tools::r#trait=agent_lib::agent::tools::base','agent_lib::tools=agent_lib::agent::tools'
```

- [ ] **Step 4: 更新模块声明**

`src-tauri/src/agent/mod.rs`：

```rust
pub mod critic;
pub mod fast_loop;
pub mod orchestrator;
pub mod tools;
pub mod workers;
```

`src-tauri/src/agent/tools/mod.rs`：把 `pub mod r#trait;` 改为 `pub mod base;`。

`lib.rs` 删除 `pub mod critic;`、`pub mod orchestrator;`、`pub mod tools;`、`pub mod workers;`。

- [ ] **Step 5: 确认测试已不依赖 `AppConfig`**

13 处构造在 Task 3 的 Step 4b 已经改成 `ProviderRegistry::new(&[])`，本步只做核对，不改代码：

```bash
Select-String -Path (Get-ChildItem -Recurse src-tauri/src -Filter *.rs).FullName -Pattern 'ProviderRegistry::new\(&crate::config::AppConfig'
```

Expected: 无匹配。若有残留，说明 Task 3 未按 Step 4b 完成，就地补改成 `ProviderRegistry::new(&[])`。

- [ ] **Step 6: 编译并跑测试**

```bash
cargo check --manifest-path src-tauri/Cargo.toml --all-targets
cargo test --manifest-path src-tauri/Cargo.toml
Select-String -Path (Get-ChildItem -Recurse src-tauri/src -Filter *.rs).FullName -Pattern 'r#loop|r#trait'
```

Expected: 全绿；最后一条无匹配。

- [ ] **Step 7: 提交**

```bash
git add -A
git commit -m "refactor(agent): group agent runtime modules and drop raw identifiers"
```

---

## Task 5: 建立 `features/` 层

**Files:**
- Move: `src-tauri/src/memory/` → `src-tauri/src/features/memory/`
- Move: `src-tauri/src/persona/` → `src-tauri/src/features/persona/`
- Move: `src-tauri/src/skills/` → `src-tauri/src/features/skills/`
- Move: `src-tauri/src/mcp/` → `src-tauri/src/features/mcp/`
- Move: `src-tauri/src/workspace/` → `src-tauri/src/features/workspace/`
- Move: `src-tauri/src/pipeline/` → `src-tauri/src/features/workflow/`
- Move: `src-tauri/src/lifecycle/` → `src-tauri/src/features/session/lifecycle/`
- Create: `src-tauri/src/features/mod.rs`、`src-tauri/src/features/session/mod.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Produces: `crate::features::{memory, persona, skills, mcp, workspace}`。
- Produces: `crate::features::workflow`（原 `crate::pipeline`）。
- Produces: `crate::features::session::lifecycle`（原 `crate::lifecycle`）。

- [ ] **Step 1: 移动目录**

```bash
New-Item -ItemType Directory -Force src-tauri/src/features/session | Out-Null
git mv src-tauri/src/memory src-tauri/src/features/memory
git mv src-tauri/src/persona src-tauri/src/features/persona
git mv src-tauri/src/skills src-tauri/src/features/skills
git mv src-tauri/src/mcp src-tauri/src/features/mcp
git mv src-tauri/src/workspace src-tauri/src/features/workspace
git mv src-tauri/src/pipeline src-tauri/src/features/workflow
git mv src-tauri/src/lifecycle src-tauri/src/features/session/lifecycle
```

- [ ] **Step 2: 观察预期的编译失败**

```bash
cargo check --manifest-path src-tauri/Cargo.toml --all-targets 2>&1 | Select-String 'E0583|E0432|E0433' | Select-Object -First 5
```

Expected: `file not found for module 'memory'`。

- [ ] **Step 3: 改写路径**

```bash
powershell -NoProfile -File .\scripts\rewrite-crate-paths.ps1 -Map 'crate::pipeline=crate::features::workflow','crate::lifecycle=crate::features::session::lifecycle','crate::memory=crate::features::memory','crate::persona=crate::features::persona','crate::skills=crate::features::skills','crate::mcp=crate::features::mcp','crate::workspace=crate::features::workspace'
```

- [ ] **Step 4: 更新模块声明**

Create `src-tauri/src/features/mod.rs`:

```rust
pub mod mcp;
pub mod memory;
pub mod persona;
pub mod session;
pub mod skills;
pub mod workspace;
pub mod workflow;
```

Create `src-tauri/src/features/session/mod.rs`:

```rust
pub mod lifecycle;
```

`lib.rs` 删除 `pub mod lifecycle;`、`pub mod mcp;`、`pub mod memory;`、`pub mod persona;`、`pub mod pipeline;`、`pub mod skills;`、`pub mod workspace;`，新增 `pub mod features;`。

- [ ] **Step 5: 编译并跑测试**

```bash
cargo check --manifest-path src-tauri/Cargo.toml --all-targets
cargo test --manifest-path src-tauri/Cargo.toml
```

Expected: 全绿。

- [ ] **Step 6: 提交**

```bash
git add -A
git commit -m "refactor(features): group business capabilities under features layer"
```

---

## Task 6: 建立 `app/` 层并收拢命令

**Files:**
- Move: `src-tauri/src/config.rs` → `src-tauri/src/app/config.rs`
- Move: `src-tauri/src/cli/` → `src-tauri/src/app/cli/`
- Move: `src-tauri/src/state.rs` → `src-tauri/src/app/state.rs`
- Move: `src-tauri/src/commands/` → `src-tauri/src/app/commands/`
- Rename: `app/commands/skill.rs` → `skills.rs`、`pipeline.rs` → `workflow.rs`、`orchestrator.rs` → `orchestrate.rs`
- Create: `src-tauri/src/app/mod.rs`
- Modify: `src-tauri/src/app/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/main.rs`
- Modify: `src-tauri/tests/toolchain_cli_tests.rs`

**Interfaces:**
- Produces: `crate::app::{config::AppConfig, state::AppState, cli::{Cli, run}, commands::*}`。
- Produces: `agent_lib::app::cli`（二进制与集成测试入口）。

- [ ] **Step 1: 移动文件与目录**

```bash
New-Item -ItemType Directory -Force src-tauri/src/app | Out-Null
git mv src-tauri/src/config.rs src-tauri/src/app/config.rs
git mv src-tauri/src/cli src-tauri/src/app/cli
git mv src-tauri/src/state.rs src-tauri/src/app/state.rs
git mv src-tauri/src/commands src-tauri/src/app/commands
git mv src-tauri/src/app/commands/skill.rs src-tauri/src/app/commands/skills.rs
git mv src-tauri/src/app/commands/pipeline.rs src-tauri/src/app/commands/workflow.rs
git mv src-tauri/src/app/commands/orchestrator.rs src-tauri/src/app/commands/orchestrate.rs
```

- [ ] **Step 2: 观察预期的编译失败**

```bash
cargo check --manifest-path src-tauri/Cargo.toml --all-targets 2>&1 | Select-String 'E0583|E0432|E0433' | Select-Object -First 5
```

Expected: `file not found for module 'config'`。

- [ ] **Step 3: 改写路径**

```bash
powershell -NoProfile -File .\scripts\rewrite-crate-paths.ps1 -Map 'crate::config=crate::app::config','crate::cli=crate::app::cli','crate::state=crate::app::state','crate::commands=crate::app::commands','agent_lib::config=agent_lib::app::config','agent_lib::cli=agent_lib::app::cli'
```

- [ ] **Step 4: 更新 `app/commands/mod.rs`**

模块声明改为（`orchestrator`→`orchestrate`、`skill`→`skills`、`pipeline`→`workflow`）：

```rust
pub mod execution;
pub mod lifecycle;
pub mod mcp;
pub mod memory;
pub mod model;
pub mod orchestrate;
pub mod persona;
pub mod project;
pub mod project_env;
pub mod prompt;
pub mod provider;
pub mod session;
pub mod settings;
pub mod skills;
pub mod tool;
pub mod toolchain;
pub mod window;
pub mod workflow;
```

`pub use` 列表同步：`skill::*` → `skills::*`、`pipeline::*` → `workflow::*`、`orchestrator::*` → `orchestrate::*`。

- [ ] **Step 5: 更新 `lib.rs` 的 handler 列表**

先替换 `commands::orchestrator::` → `commands::orchestrate::`，再替换全部 `commands::` → `app::commands::`（只执行一次）。最后 `pub mod` 声明收敛为：

```rust
pub mod agent;
pub mod app;
pub mod core;
pub mod features;
pub mod llm;
pub mod runtime;
```

- [ ] **Step 6: 更新 `app/mod.rs`**

Create `src-tauri/src/app/mod.rs`:

```rust
pub mod cli;
pub mod commands;
pub mod config;
pub mod state;
```

- [ ] **Step 7: 编译**

```bash
cargo check --manifest-path src-tauri/Cargo.toml --all-targets
```

Expected: 无错误。若出现 `cannot find function`，说明 handler 列表漏改，按报错补 `app::commands::` 前缀。

- [ ] **Step 8: 核对事件名与 handler 路径**

```bash
Select-String -Path src-tauri/src/app/commands/*.rs -Pattern '\.emit\("([a-z_]+)"' -AllMatches | ForEach-Object { $_.Matches } | ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique
Select-String -Path src-tauri/src/lib.rs -Pattern 'commands::' -AllMatches | Measure-Object
```

Expected: 事件名集合为 `debug_messages, execution_log, install_progress, orchestrator_done, plan_generated, plan_progress, stream_chunk`（与移动前一致）；`lib.rs` 中每个 handler 路径都带 `app::commands::` 前缀。

- [ ] **Step 9: 跑测试并提交**

```bash
cargo test --manifest-path src-tauri/Cargo.toml
git add -A
git commit -m "refactor(app): move composition root and command adapters into app layer"
```

Expected: 测试全绿。

---

## Task 7: 收尾验证与文档同步

**Files:**
- Modify: `AGENTS.md`
- Modify: `docs/agents/overview.md`
- Modify: `docs/agents/traps.md`（如有旧路径）
- Modify: `docs/agents/workflow.md`（如有旧路径）

**Interfaces:**
- Consumes: Task 1–6 产出的最终模块树。

- [ ] **Step 1: 结构验收**

```bash
Get-ChildItem src-tauri/src | Select-Object Name
Select-String -Path (Get-ChildItem -Recurse src-tauri/src -Filter *.rs).FullName -Pattern 'r#loop|r#trait'
Select-String -Path (Get-ChildItem -Recurse src-tauri/src -Filter *.rs | Where-Object { $_.FullName -notmatch '\\app\\' }).FullName -Pattern 'crate::app'
Select-String -Path (Get-ChildItem -Recurse src-tauri/src/core/db -Filter *.rs).FullName -Pattern 'crate::agent|crate::features'
```

Expected: 顶层为 `agent app core features llm runtime lib.rs main.rs`；后三条无匹配。

- [ ] **Step 2: 全量 Rust 验证**

```bash
cargo check --manifest-path src-tauri/Cargo.toml --all-targets
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets 2>&1 | Select-String '^(warning|error)' | Measure-Object
cargo test --manifest-path src-tauri/Cargo.toml
cargo run --manifest-path src-tauri/Cargo.toml -- toolchain status
```

Expected: check/test 全绿；clippy `Count ≤ 88`；CLI 打印运行时状态表。

- [ ] **Step 3: 前端回归（证明零改动）**

```bash
cd src-ui; npx tsc --noEmit
cd src-ui; npx vitest run
cd src-ui; npm run build
```

Expected: 与重组前一致（tsc 0 error、vitest 19 passed / 5 files、build 成功）。

- [ ] **Step 4: 更新 `AGENTS.md` 速查表**

替换路径：`commands/` → `app/commands/`；`tools/` → `agent/tools/`；`config.rs` 的 `ModelProvider` → `llm/model.rs`；`toolchain/registry.rs` → `runtime/toolchain/registry.rs`；`memory/seeds.rs` → `features/memory/seeds.rs`。并在"构建"章节后补一行分层说明：`core → runtime/llm → agent → features → app`。

- [ ] **Step 5: 更新 `docs/agents/overview.md`**

替换目录结构树为新结构，新增一节「模块分层与依赖规则」，内容取自 spec §4（含 3 处已记录例外）。

- [ ] **Step 6: 同步其余文档**

```bash
Select-String -Path docs/agents/*.md,AGENTS.md -Pattern 'src-tauri/src/(commands|tools|workers|orchestrator|pipeline|lifecycle|memory|persona|skills|mcp|workspace|toolchain|project_env|cli|config\.rs|state\.rs|db|error\.rs)'
```

按匹配结果逐条替换为新的模块路径。

- [ ] **Step 7: 最终提交**

```bash
git add -A
git commit -m "docs: document new src-tauri module layout"
git log --oneline master..HEAD
```

Expected: 7 个 commit（Task 1–7）。
