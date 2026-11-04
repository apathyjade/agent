# 意图管理 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把意图从硬编码常量改造成按领域组织、可在 SQLite 里增删改查的业务数据，并让现有 `IntentRouter` 直接读这份数据。

**Architecture:** 两张表（`intent_domains` / `intents`）落在现有 `core/db`，仓储方法放新文件 `core/db/intent_repo.rs`；内置领域在 `app/state.rs` 启动时同步播种；分类器拆成可单测的纯函数，`IntentRouter` 每次分类从库里读活跃领域的意图。`AppConfig.intent_routing` 删除，只留 `enabled` 与 `classifier_model_id` 两个应用级开关。

**Tech Stack:** Rust / rusqlite / serde_json / Tauri 2 / React + Zustand。

**Spec:** `docs/design/2026-11-04-intent-management-design.md`

## Global Constraints

- 分层单向：`core` 不得引用 `llm|agent|features|app|runtime`；`features` 不得引用 `app`。播种放在 `app/state.rs`，因为它允许依赖所有层。
- **不打开 SQLite 外键**。`init_tables` 保持 `PRAGMA foreign_keys = OFF`；删领域时显式 `DELETE FROM intents WHERE domain_id = ?1`。
- 不新增依赖，不动 `Cargo.toml`。
- 不改任何 Tauri 命令的既有名称/参数/返回结构；新增命令照 `app/commands/persona.rs` 的写法。
- 不改 Tauri 事件名。现有集合：`debug_messages, execution_log, install_progress, orchestrator_done, plan_generated, plan_progress, stream_chunk`。
- 不运行 `cargo fmt`（仓库基线非 rustfmt-clean）。
- 所有命令从仓库根执行：`cargo test --manifest-path src-tauri/Cargo.toml`，路径带 `src-tauri/` 前缀。
- 既有 199 个单测 + 集成 3/7/11/3/38 必须保持全绿；`cargo clippy --all-targets` 警告不超过 88。
- 源码 UTF-8 无 BOM，工作区 CRLF。`.rs` 文件从 PowerShell 写入时必须 `[System.IO.File]::WriteAllText($p, $t, (New-Object System.Text.UTF8Encoding($false)))`。
- **前端命令不在本 worktree 跑**：本 worktree 没有 `src-ui/node_modules`，且不允许自行安装。所有 `npx tsc` / `npx vitest` / `npm run build` 都到主 worktree 执行：

  ```powershell
  $env:PATH = "$env:APPDATA\fnm\aliases\default;$env:PATH"
  cd D:\space\github\apathyjade\agent\src-ui
  ```

  这会校验到同一份源码：整条分支没有改过 `src-ui/` 之外的差异，`package.json` 与 `pnpm-lock.yaml` 除行尾外一致。`dist/` 已 gitignore，`npm run build` 不会留下被跟踪的改动。**改完的文件仍然 `git add` 本 worktree 的路径。**

## Review Focus

最容易被实现者忽略、但会直接伤到用户的五件事。每条都在后面标了由哪个任务的测试钉住。

1. **把领域里的最后一个启用意图停用或删掉** —— 预期：操作被拒绝并说明原因，而不是让路由进入"零意图"状态。（Task 2、Task 3）
2. **导入一份畸形 JSON**（空数组 / 缺字段 / 不是数组 / 某条 `name` 重复）—— 预期：整批回滚，库里不留半个领域，错误信息指出第几条。（Task 5、Task 8）
3. **删掉当前活跃领域** —— 预期：拒绝，而不是留下"没有活跃领域"的库。（Task 2）
4. **旧配置文件里带着 `intent_routing` 启动** —— 预期：正常启动，键被忽略，不 panic、不覆盖用户数据。（Task 7）
5. **重复播种**（第二次启动、或用户在界面上改过内置领域后再启动）—— 预期：不重复插入，不覆盖用户编辑。（Task 4）

---

### Task 1: 表结构与记录类型

**Files:**
- Modify: `src-tauri/src/core/db/models.rs`（文件末尾追加两个结构体）
- Modify: `src-tauri/src/core/db/repository.rs:46`（`init_tables` 的 `execute_batch` 里追加两段 DDL）
- Create: `src-tauri/src/core/db/intent_repo.rs`
- Modify: `src-tauri/src/core/db/mod.rs`（加 `pub mod intent_repo;`）
- Test: `src-tauri/src/core/db/intent_repo.rs` 内的 `#[cfg(test)] mod tests`

**Interfaces:**
- Produces:
  - `pub struct IntentDomainRecord { pub id: String, pub name: String, pub description: String, pub is_active: bool, pub source: String, pub version: i64, pub created_at: String, pub updated_at: String }`
  - `pub struct IntentRecord { pub id: String, pub domain_id: String, pub name: String, pub description: String, pub examples: Vec<String>, pub workflow: String, pub allowed_tools: Vec<String>, pub default_agent: String, pub scope: String, pub enabled: bool, pub version: i64, pub confidence_threshold: f64, pub fallback_intent: String, pub path: String, pub auto_escalate: bool, pub system_prompt_appendix: String, pub model_id: String, pub max_iterations: Option<usize>, pub sort_order: i64, pub created_at: String, pub updated_at: String }`
  - 两者都 `#[derive(Debug, Clone, Serialize, Deserialize)]`，与 `PersonaRecord` 同款。
  - `examples` / `allowed_tools` 在结构体里是 `Vec<String>`，在库里是 JSON 文本。本任务只定义结构体与表，不做映射。

- [ ] **Step 1: 写失败的 schema 测试**

在新建的 `src-tauri/src/core/db/intent_repo.rs` 末尾：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intent_tables_exist_with_expected_columns() {
        let db = Database::new_test().unwrap();
        let domains = db.count_columns("intent_domains");
        let intents = db.count_columns("intents");
        assert_eq!(domains, 8, "intent_domains 列数不符");
        assert_eq!(intents, 21, "intents 列数不符");
    }
}
```

并在同文件顶部实现测试辅助：

```rust
impl Database {
    #[cfg(test)]
    pub fn count_columns(&self, table: &str) -> i64 {
        self.conn
            .query_row(&format!("SELECT COUNT(*) FROM pragma_table_info('{table}')"), [], |r| {
                r.get(0)
            })
            .unwrap_or(-1)
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml count_columns`
Expected: FAIL —— `count_columns` 用到 `self.conn` 但 `intent_repo.rs` 还没被 `mod` 声明，编译报 `unresolved module`；或断言得到 `-1`。

- [ ] **Step 3: 加 DDL**

在 `repository.rs` 的 `init_tables` 里，`personas` 那段之后、`session_summaries` 之前插入：

```sql
CREATE TABLE IF NOT EXISTS intent_domains (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    is_active INTEGER NOT NULL DEFAULT 0,
    source TEXT NOT NULL DEFAULT 'user',
    version INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS intents (
    id TEXT PRIMARY KEY,
    domain_id TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    examples TEXT NOT NULL DEFAULT '[]',
    workflow TEXT NOT NULL DEFAULT '',
    allowed_tools TEXT NOT NULL DEFAULT '[]',
    default_agent TEXT NOT NULL DEFAULT '',
    scope TEXT NOT NULL DEFAULT 'global',
    enabled INTEGER NOT NULL DEFAULT 1,
    version INTEGER NOT NULL DEFAULT 1,
    confidence_threshold REAL NOT NULL DEFAULT 0.7,
    fallback_intent TEXT NOT NULL DEFAULT '',
    path TEXT NOT NULL DEFAULT 'fast',
    auto_escalate INTEGER NOT NULL DEFAULT 0,
    system_prompt_appendix TEXT NOT NULL DEFAULT '',
    model_id TEXT NOT NULL DEFAULT '',
    max_iterations INTEGER,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(domain_id, name)
);

CREATE INDEX IF NOT EXISTS idx_intents_domain ON intents(domain_id, sort_order);
```

- [ ] **Step 4: 加结构体**

在 `core/db/models.rs` 末尾追加 `IntentDomainRecord` 与 `IntentRecord`，字段与上面的 `Interfaces` 逐字一致。

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml intent_tables_exist`
Expected: PASS（`intent_domains` 8 列、`intents` 21 列）。

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/core/db/
git commit -m "feat(db): add intent domain and intent tables"
```

---

### Task 2: 领域仓储

**Files:**
- Modify: `src-tauri/src/core/db/intent_repo.rs`
- Test: 同文件 `mod tests`

**Interfaces:**
- Consumes: `IntentDomainRecord`、`Database::conn`（同 crate 私有字段可见）。
- Produces（全部 `impl Database`）：
  - `pub fn list_intent_domains(&self) -> Result<Vec<IntentDomainRecord>>`
  - `pub fn get_intent_domain(&self, id: &str) -> Result<Option<IntentDomainRecord>>`
  - `pub fn get_active_intent_domain(&self) -> Result<Option<IntentDomainRecord>>`
  - `pub fn insert_intent_domain(&self, d: &IntentDomainRecord) -> Result<()>`
  - `pub fn update_intent_domain(&self, d: &IntentDomainRecord) -> Result<()>`
  - `pub fn delete_intent_domain(&self, id: &str) -> Result<()>`
  - `pub fn set_active_intent_domain(&self, id: &str) -> Result<()>`
  - `#[cfg(test)] pub fn count_intents(&self, domain_id: &str) -> i64`

**不变量（这三条是 Review Focus 1、3 的落点）**
- `delete_intent_domain`：目标 `is_active` 为真 → `Err(AppError::InvalidInput("不能删除当前活跃领域".into()))`。
- `set_active_intent_domain`：目标领域下 `enabled = 1` 的意图数为 0 → `Err(AppError::InvalidInput("该领域没有启用的意图，无法激活".into()))`。
- `set_active_intent_domain` 必须在事务里完成：`let tx = self.conn.unchecked_transaction()?;` → `UPDATE intent_domains SET is_active = 0` → `UPDATE intent_domains SET is_active = 1 WHERE id = ?1` → 校验 `tx.changes() == 1`（为 0 说明 id 不存在）→ `tx.commit()?`。用 `unchecked_transaction` 而不是 `transaction`，因为 `conn` 在 `Database` 里不是 `mut`。

- [ ] **Step 1: 写失败的测试**

```rust
fn mk_domain(id: &str, name: &str) -> IntentDomainRecord { /* 用固定 RFC3339 时间戳 */ }

#[test]
fn delete_active_domain_is_rejected() { /* insert 领域, set_active, 断言 delete 返回 InvalidInput */ }

#[test]
fn activating_domain_without_enabled_intents_is_rejected() { /* insert 空领域 → set_active 报错 */ }

#[test]
fn set_active_domain_is_exclusive() { /* 两个领域, 依次激活, 断言任一时刻只有一个 is_active=1 */ }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib intent_domain`
Expected: FAIL —— 方法不存在。

- [ ] **Step 3: 实现**

用 `rusqlite::params!`，`is_active` 在 SQL 里是 `INTEGER`，读出后 `!= 0`。`updated_at` 由调用方写入（仓储不造时间）。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib intent_domain`
Expected: PASS，3 个用例。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/core/db/intent_repo.rs
git commit -m "feat(db): add intent domain repository with activation guards"
```

---

### Task 3: 意图仓储

**Files:**
- Modify: `src-tauri/src/core/db/intent_repo.rs`
- Test: 同文件 `mod tests`

**Interfaces:**
- Consumes: `IntentRecord`、Task 2 的领域方法。
- Produces（全部 `impl Database`）：
  - `pub fn list_intents(&self, domain_id: &str) -> Result<Vec<IntentRecord>>`（按 `sort_order ASC, name ASC`）
  - `pub fn list_enabled_intents(&self, domain_id: &str) -> Result<Vec<IntentRecord>>`
  - `pub fn get_intent(&self, id: &str) -> Result<Option<IntentRecord>>`
  - `pub fn insert_intent(&self, i: &IntentRecord) -> Result<()>`
  - `pub fn update_intent(&self, i: &IntentRecord) -> Result<()>`
  - `pub fn delete_intent(&self, id: &str) -> Result<()>`
  - `pub fn set_intent_enabled(&self, id: &str, enabled: bool, updated_at: &str) -> Result<()>`（仓储不造时间，时间戳由命令层传）
  - `pub fn count_enabled_intents(&self, domain_id: &str) -> Result<i64>`

**不变量（Review Focus 1）**
- `delete_intent`：该意图 `enabled = 1` 且所在领域启用意图数为 1 → `Err(AppError::InvalidInput("不能删除领域内最后一个启用的意图".into()))`。
- `set_intent_enabled(id, false)`：同条件 → 同样错误。
- `update_intent` 每次把 `version` 加一（读取旧值 +1，不信任调用方传进来的 `version`）。

**JSON 列映射**：`examples` / `allowed_tools` 用 `serde_json::to_string(&i.examples)?` 写入，读取时 `serde_json::from_str(&s).unwrap_or_default()`——**解析失败不要 panic**，坏数据当空数组。

- [ ] **Step 1: 写失败的测试**

```rust
#[test]
fn disabling_last_enabled_intent_is_rejected() {}
#[test]
fn deleting_last_enabled_intent_is_rejected() {}
#[test]
fn update_intent_increments_version() {}
#[test]
fn malformed_json_column_reads_as_empty_vec() { /* 直接用 SQL 写入 "not-json"，断言 list 出来是 vec![] */ }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib intent_repo`
Expected: FAIL —— 方法不存在。

- [ ] **Step 3: 实现**

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib intent_repo`
Expected: PASS，4 个用例。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/core/db/intent_repo.rs
git commit -m "feat(db): add intent repository with last-enabled guards"
```

---

### Task 4: 内置「程序开发」领域种子

**Files:**
- Create: `src-tauri/src/features/intent_management/mod.rs`（`pub mod seeds;`）
- Create: `src-tauri/src/features/intent_management/seeds.rs`
- Modify: `src-tauri/src/features/mod.rs`（加 `pub mod intent_management;`）
- Modify: `src-tauri/src/app/state.rs:50`（`Database::new()` 之后立刻播种）

**Interfaces:**
- Consumes: `IntentDomainRecord`、`IntentRecord`、`Database`。
- Produces:
  - `pub fn builtin_software_dev() -> (IntentDomainRecord, Vec<IntentRecord>)` —— 领域 id `builtin.software_dev`，含 12 条意图，内容逐字取自设计文档 §6.3。
  - `pub fn seed_builtin_intents(db: &Database) -> Result<()>` —— 若 `list_intent_domains()` 已包含 `builtin.software_dev` 则直接返回；否则按此顺序写入：**先插领域、再逐条插 12 个意图、最后才 `set_active_intent_domain`**。顺序不能反——Task 2 的守卫会拒绝激活零启用意图的领域。

**为什么播种在 `app/state.rs`**：`core` 不能依赖 `features`（分层约束），所以不能在 `Database::new()` 里播种。`AppState::new` 允许依赖一切，且它天然在构造 `IntentRouter` 之前。

- [ ] **Step 1: 写失败的测试**

```rust
#[test]
fn seeding_is_idempotent() {
    let db = Database::new_test().unwrap();
    seed_builtin_intents(&db).unwrap();
    seed_builtin_intents(&db).unwrap();
    assert_eq!(db.list_intent_domains().unwrap().len(), 1);
    assert_eq!(db.count_intents("builtin.software_dev"), 12);
}

#[test]
fn seeding_preserves_user_edits() {
    // 播种 → 改一条意图的 name → 再播种 → 断言 name 仍是改后的值
}

#[test]
fn seeded_domain_is_active() {
    // 断言 get_active_intent_domain() 的 id == "builtin.software_dev"
}

#[test]
fn seed_matches_design_document() {
    // 断言 12 条的 id 集合与设计文档 §6.3 一致，且每条 allowed_tools 都在
    // ["file_system","code_executor","web_search","calculator","run_workflow"] 之内，
    // default_agent 都在 ["","thinker","code_explorer","code_editor","shell","web","memory","mcp_bridge"] 之内
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib seeding`
Expected: FAIL —— `seed_builtin_intents` 不存在。

- [ ] **Step 3: 实现种子数据与播种函数**

`builtin_software_dev()` 里 12 条意图逐字照抄设计文档 §6.3 的 JSON：`plan / spec / implement / debug / test / review / refactor / explain / lookup / execute / clarify / chat`。`created_at` / `updated_at` 用 `chrono::Utc::now().to_rfc3339()`（`features/persona/seeds.rs` 同款）。`sort_order` 用数组下标 × 10。

- [ ] **Step 4: 在 `AppState::new` 里播种**

`state.rs` 里 `let db = Database::new()?;` 之后加一行：

```rust
crate::features::intent_management::seeds::seed_builtin_intents(&db)?;
```

必须在 `let db_arc = Arc::new(Mutex::new(db));` 之前，且早于 `IntentRouter::new(...)`。

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib seeding` 然后 `cargo test --manifest-path src-tauri/Cargo.toml --lib seed_matches`
Expected: PASS，4 个用例。

- [ ] **Step 6: 全量门禁 + Commit**

Run: `cargo check --manifest-path src-tauri/Cargo.toml --all-targets`
Expected: 无错误。

```bash
git add src-tauri/src/features/ src-tauri/src/app/state.rs
git commit -m "feat(intent): seed builtin software-dev domain"
```

---

### Task 5: 导入 / 导出 JSON

**Files:**
- Create: `src-tauri/src/features/intent_management/json.rs`
- Modify: `src-tauri/src/features/intent_management/mod.rs`（`pub mod json;`）
- Test: 同文件 `mod tests`

**Interfaces:**
- Consumes: `IntentRecord`、`IntentDomainRecord`。
- Produces:
  - `pub struct IntentJson { pub id: Option<String>, pub name: String, pub description: String, pub examples: Vec<String>, pub workflow: String, pub allowed_tools: Vec<String>, pub default_agent: String, pub scope: String, pub enabled: bool, pub confidence_threshold: f64, pub fallback_intent: String, pub path: String, pub auto_escalate: bool, pub system_prompt_appendix: String, pub model_id: String, pub max_iterations: Option<usize> }` —— 全部 `#[serde(default)]`，`id` 为 `Option` 因为导入时忽略它。
  - `pub fn parse_intent_array(json: &str) -> Result<Vec<IntentJson>>` —— 非数组、空数组都返回 `Err(AppError::InvalidInput)`。
  - `pub fn to_records(domain_id: &str, inputs: &[IntentJson], now: &str) -> Result<Vec<IntentRecord>>` —— 生成 uuid 作 id，校验：`name` 非空且领域内不重复、`path ∈ {"fast","deep"}`、`confidence_threshold ∈ [0.0,1.0]`、`fallback_intent` 若非空必须指向本批次的某条 `id` 或 `name`，否则报错并指出是第几条（下标从 1 开始）。`fallback_intent` 在输出里统一换成最终生成的 id。
  - `pub fn to_json_array(intents: &[IntentRecord]) -> Result<String>`

**Review Focus 2 的落点**：整批校验在内存里做完再写库，任何一条不合法就整批失败。

- [ ] **Step 1: 写失败的测试**

```rust
#[test]
fn rejects_non_array_input() {}
#[test]
fn rejects_empty_array() {}
#[test]
fn rejects_duplicate_names() {}
#[test]
fn rejects_out_of_range_threshold() {}
#[test]
fn unknown_fallback_names_the_offending_index() {
    // 断言错误文案里出现 "第 3 条"
}
#[test]
fn roundtrip_preserves_business_fields() {
    // 12 条 → to_records → to_json_array → parse → to_records(另一领域)
    // 逐字段比较，不比较 id；fallback 按解析后的 id 关系比较
}
#[test]
fn reimporting_exported_json_does_not_collide() {
    // 同一份 JSON 导入两次，两次得到的 id 集合不相交
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib intent_json`
Expected: FAIL —— 模块不存在。

- [ ] **Step 3: 实现**

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib intent_json`
Expected: PASS，7 个用例。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/features/intent_management/
git commit -m "feat(intent): add intent JSON import/export with validation"
```

---

### Task 6: 分类器纯函数

**Files:**
- Modify: `src-tauri/src/llm/intent/classifier.rs`
- Test: 同文件 `mod tests`（本任务新建）

**Interfaces:**
- Consumes: `IntentRecord`（`llm` 可以依赖 `core`，`IntentRecord` 在 `core::db::models`，合法）。
- Produces:
  - `pub fn build_classifier_prompt(intents: &[IntentRecord]) -> String` —— 由数据生成，每条形如 `- id: <id>\n  name: <name>\n  when: <description>\n  examples: <a> | <b> | ...`；末尾要求模型输出 `{ "intent": "<id>", "confidence": 0.0-1.0, "reason": "..." }`。
  - `pub fn score_intents(message: &str, intents: &[IntentRecord]) -> Option<(String, f32)>` —— 回退打分：对每条意图，命中任一 `example`（大小写不敏感子串）得 1.0；否则按 `description` 与本条 `name` 的中文二元组重合率算分；取最高分；全部为 0 返回 `None`。
  - `pub fn apply_confidence(intents: &[IntentRecord], chosen: &str, score: f32) -> String` —— 若 `score < 该意图的 confidence_threshold`，返回 `fallback_intent`（非空且存在）否则返回该领域里 `id` 以 `.clarify` 结尾的那条，再否则返回 `chosen`。
  - `impl LlmClassifier`：
    - `pub fn new(providers: Arc<Mutex<ProviderRegistry>>, model_id: Option<String>) -> Self` —— **不再读 `OPENAI_API_KEY`**，也不再在构造时持有 key。
    - `pub async fn classify(&self, message: &str, intents: &[IntentRecord]) -> Option<(String, f32)>` —— 用 `build_classifier_prompt(intents)` 组提示词，走 `resolve(model_id)` 拿 provider，结构化提取 `{ intent, confidence }`；provider 不存在、请求失败、JSON 解析失败、返回的 intent 不在 `intents` 里，四种情况一律返回 `None`，由调用方落回退打分。
  - 旧的中英文关键词表与 `fallback_classify` **删除**——关键词能力由 `score_intents` 从数据的 `examples` 里提供。

**注意**：本任务不碰 `IntentRouter`，也不删 `IntentConfig`；`classify` 暂时保留旧签名（用不了的数据由 Task 7 接）。

- [ ] **Step 1: 写失败的测试**

```rust
#[test]
fn prompt_lists_every_intent_id() {}
#[test]
fn prompt_mentions_confidence_field() {}
#[test]
fn score_matches_example_substring() {}
#[test]
fn score_prefers_longer_example_match() {}   // "把这段改成异步" 应命中 implement 而不是 explain
#[test]
fn score_returns_none_when_nothing_matches() {}
#[test]
fn low_confidence_falls_back_to_neighbour() {}
#[test]
fn empty_fallback_is_terminal() {}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib classifier`
Expected: FAIL —— 函数不存在。

- [ ] **Step 3: 实现三个纯函数**

中文二元组：把字符串按 `char` 收集成 `Vec<char>`，取相邻两字组成 `String`，放进 `HashSet`；重合率 = 交集 / 并集。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib classifier`
Expected: PASS，7 个用例。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/llm/intent/classifier.rs
git commit -m "feat(intent): data-driven classifier prompt and scoring"
```

---

### Task 7: Router 改读库，删掉 `intent_routing`

**Files:**
- Modify: `src-tauri/src/llm/intent/router.rs`（重写）
- Modify: `src-tauri/src/llm/intent/mod.rs`（删 `IntentRouterConfig` / `default_intents()`，保留 `IntentConfig` / `IntentResult` / `PathSelection`）
- Modify: `src-tauri/src/app/config.rs`（删 `intent_routing`，加 `intent: IntentAppConfig`）
- Modify: `src-tauri/src/app/state.rs`（`IntentRouter::new` 新签名）
- Modify: `src-tauri/src/app/commands/session.rs:361`（`should_auto_escalate` → `config.auto_escalate`）

**Interfaces:**
- Produces:
  - `IntentRouter::new(db: Arc<Mutex<Database>>, providers: Arc<Mutex<ProviderRegistry>>, classifier_model_id: Option<String>, enabled: bool) -> Self`
  - `pub async fn classify(&self, content: &str) -> IntentResult`
  - `IntentResult { pub id: String, pub name: String, pub config: IntentConfig, pub path: PathSelection, pub confidence: f32 }`
  - `IntentConfig` **字段不变**（`system_prompt_appendix` / `enabled_tools` / `model_id` / `max_iterations` / `auto_escalate`），这样 `session.rs:563/614/621` 三处调用点不用改。
  - `resolve_tools` 行为与签名不变。
  - **`should_auto_escalate` 删除**，调用点改用 `_intent_result.config.auto_escalate`。

**行为约定**
- `enabled == false` → 不查库，直接返回 id `builtin.software_dev.chat` 的空配置、fast 路径。
- 无活跃领域，或活跃领域启用意图数为 0 → `log::warn!` 一次，返回 fast / 无工具 / 无迭代上限的结果（Review Focus 1 的兜底）。
- 正常路径：`list_enabled_intents(active.id)` → `build_classifier_prompt` → LLM → 解析 → `apply_confidence` → 按最终 id 取 `IntentRecord` → 投影成 `IntentResult`。
- LLM 不可用（无 provider / 报错 / JSON 解析失败）→ `score_intents` → `apply_confidence`；仍无结果 → `clarify`（若存在）否则第一条。

- [ ] **Step 1: 写失败的测试**

```rust
#[test]
fn disabled_router_returns_fast_path() {}
#[test]
fn no_active_domain_returns_fast_path_without_panicking() {}
#[test]
fn domain_with_zero_enabled_intents_returns_fast_path() {}
#[test]
fn path_comes_from_the_intent_row() {}  // 插入 path=deep 的意图，断言 IntentResult.path == Deep
#[test]
fn config_projects_row_fields() {}      // enabled_tools / max_iterations / model_id / auto_escalate 逐项对上
#[test]
fn old_config_json_with_intent_routing_still_loads() {
    // 直接 serde_json::from_str::<AppConfig>(r#"{"intent_routing":{...},"models":[]}"#)
    // 断言 Ok（不 panic、不报 unknown field）
}
```

这些用例需要构造 `ProviderRegistry`。统一用 `ProviderRegistry::new(&[])`（空表即可，LLM 路径会失败并落回退，正是要测的）。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib intent_router`
Expected: 编译失败 —— 新签名尚未存在。

- [ ] **Step 3: 重写 `router.rs` 与 `mod.rs`**

- [ ] **Step 4: 改 `app/config.rs`**

删 `pub intent_routing: IntentRouterConfig`，加：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentAppConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub classifier_model_id: Option<String>,
}
impl Default for IntentAppConfig {
    fn default() -> Self { Self { enabled: true, classifier_model_id: None } }
}
```

`AppConfig` 里加 `#[serde(default)] pub intent: IntentAppConfig,`。若文件里已有 `default_true` 就用它，否则新增。

- [ ] **Step 5: 改 `app/state.rs` 与 `session.rs` 调用点**

`state.rs`：`IntentRouter::new(db_arc.clone(), providers_arc.clone(), config.intent.classifier_model_id.clone(), config.intent.enabled)`。
`session.rs:361`：`let should_escalate = _intent_result.config.auto_escalate;`

- [ ] **Step 6: 全量门禁**

Run: `cargo check --manifest-path src-tauri/Cargo.toml --all-targets`
Expected: 无错误。若报 `intent_routing` 残留，说明漏改调用点。

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: 全绿。

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/llm/intent/ src-tauri/src/app/
git commit -m "refactor(intent): route from the intent database, drop intent_routing config"
```

---

### Task 8: IPC 命令

**Files:**
- Create: `src-tauri/src/app/commands/intent.rs`
- Modify: `src-tauri/src/app/commands/mod.rs`（`pub mod intent;` + `pub use intent::*;`）
- Modify: `src-tauri/src/lib.rs`（`generate_handler!` 追加 14 个条目）

**Interfaces:**
- Consumes: Task 2/3 的仓储方法、Task 5 的 `json` 模块。
- Produces（`#[tauri::command]`，全部 `Result<T>`）：
  - `list_intent_domains`, `get_active_intent_domain`, `create_intent_domain(name, description)`, `update_intent_domain(id, name, description)`, `delete_intent_domain(id)`, `set_active_intent_domain(id)`
  - `list_intents(domain_id)`, `get_intent(id)`, `create_intent(domain_id, intent)`, `update_intent(id, intent)`, `delete_intent(id)`, `set_intent_enabled(id, enabled)`
  - `import_intent_domain(json, name)`, `export_intent_domain(domain_id)`
- DTO：`IntentDomainInfo` / `IntentInfo` 直接 `#[derive(serde::Serialize)]`，字段与记录结构体一致（`examples` / `allowed_tools` 是数组，不是字符串——前端不该解析 JSON 文本）。
- **时间戳由命令层生成**：`set_intent_enabled` 与所有 `update_*` 命令用 `chrono::Utc::now().to_rfc3339()` 填 `updated_at` 再交给仓储；仓储层不调用 `Utc::now()`。

**导入的 id / 重名规则（Review Focus 2）**
- `import_intent_domain` 忽略来件 `id`，见 Task 5。
- 领域重名自动加后缀 `(2)`、`(3)`，最多尝试 20 次后报错。
- 导入结果零启用意图 → 整批拒绝。

- [ ] **Step 1: 写失败的测试**

命令层不好直接单测（需要 `AppHandle`）。本任务的验证靠两步：`cargo check --all-targets` 编译通过 + 一个针对重名后缀的纯函数测试：

```rust
// 在 intent.rs 里抽一个纯函数：
pub(crate) fn unique_domain_name(existing: &[String], wanted: &str) -> String
#[test]
fn unique_name_appends_suffix() {}
#[test]
fn unique_name_returns_base_when_free() {}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib unique_domain_name`
Expected: FAIL。

- [ ] **Step 3: 实现命令与 DTO**

- [ ] **Step 4: 注册**

`commands/mod.rs` 加 `pub mod intent;` 与 `pub use intent::*;`（注意 `pub use` 列表当前按字母序，`intent` 放在 `execution` 之后）。
`lib.rs` 的 `generate_handler![...]` 里按同样风格追加 `app::commands::list_intent_domains,` 等 14 行。

- [ ] **Step 5: 全量门禁**

Run: `cargo check --manifest-path src-tauri/Cargo.toml --all-targets` 与 `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: 全绿。

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/app/commands/ src-tauri/src/lib.rs
git commit -m "feat(intent): add IPC commands for intent management"
```

---

### Task 9: 前端 API 层

**Files:**
- Create: `src-ui/src/api/intent.ts`
- Modify: `src-ui/src/api/tauri.ts`（加 `export * from './intent';`）

**Interfaces:**
- Produces: `IntentDomainInfo` / `IntentInfo` 两个 TS interface，字段与后端 DTO 一一对应（camelCase 不转换，Tauri 对结构体字段不做改名）；14 个函数，命名与命令名对应：`listIntentDomains()`、`getActiveIntentDomain()`、`createIntentDomain(name, description)`、`updateIntentDomain(id, name, description)`、`deleteIntentDomain(id)`、`setActiveIntentDomain(id)`、`listIntents(domainId)`、`getIntent(id)`、`createIntent(domainId, intent)`、`updateIntent(id, intent)`、`deleteIntent(id)`、`setIntentEnabled(id, enabled)`、`importIntentDomain(json, name)`、`exportIntentDomain(domainId)`。

**注意**：Tauri v2 默认把**命令参数**从 camelCase 转成 snake_case，所以前端传 `domainId`、后端接 `domain_id`。

- [ ] **Step 1: 写失败的测试**

`src-ui/src/api/__tests__/intent.test.ts`（沿用既有 `__tests__` 目录的 vitest 写法）：

```ts
it('setActiveIntentDomain invokes the right command', async () => { /* mock invoke, 断言 'set_active_intent_domain' 与 { id } */ })
it('importIntentDomain passes json and name', async () => {})
```

- [ ] **Step 2: 跑测试确认失败**

Run（在主 worktree，因为本 worktree 无 `node_modules`）：`npx vitest run src/api/__tests__/intent.test.ts`
Expected: FAIL —— 模块不存在。

- [ ] **Step 3: 实现**

- [ ] **Step 4: 跑测试确认通过**

Run: `npx vitest run src/api/__tests__/intent.test.ts`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add src-ui/src/api/
git commit -m "feat(ui): add intent management api client"
```

---

### Task 10: 前端意图管理页

**Files:**
- Create: `src-ui/src/components/management/IntentManagerPage.tsx`
- Modify: `src-ui/src/store/uiSlice.ts:9`（`AppView` 加 `'intent-manager'`）
- Modify: `src-ui/src/components/common/ModuleBar.tsx`（加一个图标按钮）
- Modify: `src-ui/src/App.tsx`（渲染分支 + 顶栏标题映射）

**Interfaces:**
- Consumes: Task 9 的全部函数。
- Produces: 无（终端 UI）。

**布局**：照 `PersonaManagerPage.tsx` 的外壳。左列领域，右列意图；编辑用右侧抽屉。

**必须处理的空态**：`getActiveIntentDomain()` 返回 `null` 时显示「没有活跃领域，请选择或新建一个」，不要渲染右列。

- [ ] **Step 1: 写失败的测试**

`src-ui/src/components/management/__tests__/IntentManagerPage.test.tsx`（沿用既有 `__tests__` 的渲染测试写法）：

```tsx
it('renders empty state when no active domain', async () => {})
it('disables the enable switch on the last enabled intent', async () => {})
it('shows an error toast when delete is rejected', async () => {})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `npx vitest run src/components/management/__tests__/IntentManagerPage.test.tsx`
Expected: FAIL。

- [ ] **Step 3: 实现页面**

- [ ] **Step 4: 接线路由**

`uiSlice.ts` 的 `AppView` 加 `'intent-manager'`；`ModuleBar.tsx` 加 `handleIntentClick` 与按钮（图标用 `lucide-react` 的 `Target` 或 `Route`，与既有图标风格一致）；`App.tsx` 加 `if (currentView === 'intent-manager') return <IntentManagerPage />;` 与标题映射 `'意图管理'`。

- [ ] **Step 5: 跑测试确认通过**

Run: `npx vitest run src/components/management/__tests__/IntentManagerPage.test.tsx`
Expected: PASS，3 个用例。

- [ ] **Step 6: 全量前端门禁**

Run: `npx tsc --noEmit` 与 `npx vitest run` 与 `npm run build`
Expected: tsc 0 error；vitest 全绿；build 成功。

- [ ] **Step 7: Commit**

```bash
git add src-ui/src/
git commit -m "feat(ui): add intent manager page"
```

---

## 收尾

- [ ] `cargo check --manifest-path src-tauri/Cargo.toml --all-targets`
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets`（警告 ≤ 88）
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml`
- [ ] `cargo run --manifest-path src-tauri/Cargo.toml -- toolchain status`（确认 CLI 仍可用）
- [ ] 反向验证 Review Focus 五条：单测里都应已覆盖，逐条对照文档确认
- [ ] 更新 `docs/agents/overview.md`：`features/intent_management/` 出现在目录树里；`app/commands/intent.rs` 在命令清单里
- [ ] 更新 `AGENTS.md` 速查表：加一行「添加意图 → 界面里新建领域/意图，或 `features/intent_management/seeds.rs` 加内置模板」
