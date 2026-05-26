# Phase 1: ExecutionRuntime + Plan 基础 — 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 建立统一的 ExecutionRuntime 执行层，支持接收预定义的 ExecutionPlan 并逐步执行 ExecStep，每步结果持久化到 DB，前端可查看状态和控制暂停/取消。

**Architecture:** 新增 `execution/` 模块（types + runtime），扩展 Session 结构（mode/status），新增 5 个 IPC 命令，前端新增状态栏和时间线组件。

**Tech Stack:** Rust (Tokio, serde, rusqlite), TypeScript (React, Zustand, Tauri invoke)

**设计文档:** `docs/design/2026-05-26-session-autonomous-execution.md`

---

## 文件清单

### 新建文件
| 文件 | 职责 |
|------|------|
| `src-tauri/src/execution/mod.rs` | 模块声明 |
| `src-tauri/src/execution/types.rs` | ExecStep, ExecutionPlan, PlanStep, ExecStatus, ExecEvent 等核心类型 |
| `src-tauri/src/execution/error.rs` | ExecutionError 错误类型 |
| `src-tauri/src/execution/runtime.rs` | ExecutionRuntime 核心执行引擎 |
| `src-tauri/src/commands/execution.rs` | IPC 命令：execute_plan, pause, resume, cancel, get_status, get_plan_detail |
| `src-ui/src/components/ExecutionStatusBar.tsx` | 会话模式/执行状态指示器 |
| `src-ui/src/components/PlanTimeline.tsx` | 步骤执行进度时间线 |

### 修改文件
| 文件 | 变更内容 |
|------|----------|
| `src-tauri/src/lib.rs` | 注册 execution 模块 + commands |
| `src-tauri/src/state.rs` | 新增 active_executions 字段，管理运行中的执行句柄 |
| `src-tauri/src/error.rs` | 增加 Execution 错误变体 |
| `src-tauri/src/db/models.rs` | Session 加 mode, execution_status, active_plan_id 字段 |
| `src-tauri/src/db/repository.rs` | 加 execution_plans + steps 表的 CRUD + 迁移 |
| `src-ui/src/types/index.ts` | 加 execution 相关 TS 类型 |
| `src-ui/src/api/tauri.ts` | 加 executePlan, pauseExecution 等 IPC 包装 |
| `src-ui/src/store/sessionSlice.ts` | 扩展 mode, executionStatus, activePlanId |

---

### Task 1: 核心类型（execution/types.rs + mod.rs + error.rs）

**Files:**
- Create: `src-tauri/src/execution/mod.rs`
- Create: `src-tauri/src/execution/types.rs`
- Create: `src-tauri/src/execution/error.rs`

- [ ] **Step 1: 创建 execution/mod.rs**

```rust
pub mod error;
pub mod runtime;
pub mod types;
```

- [ ] **Step 2: 创建 execution/types.rs，定义核心类型**

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Session 执行模式
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SessionMode {
    Chat,
    Autonomous,
}

impl Default for SessionMode {
    fn default() -> Self {
        Self::Chat
    }
}

/// Session 执行状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ExecStatus {
    Idle,
    Running {
        step_index: usize,
        started_at: String,
    },
    Paused {
        step_index: usize,
        reason: String,
    },
    Completed {
        finished_at: String,
    },
    Failed {
        step_index: usize,
        error: String,
    },
    Cancelled,
}

impl Default for ExecStatus {
    fn default() -> Self {
        Self::Idle
    }
}

/// 统一执行步骤
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExecStep {
    AgentTask {
        instruction: String,
        #[serde(default)]
        model_id: Option<String>,
        #[serde(default)]
        max_iterations: Option<usize>,
        #[serde(default)]
        allowed_tools: Option<Vec<String>>,
        #[serde(default)]
        temperature: Option<f32>,
    },
    LlmCall {
        prompt: String,
        #[serde(default)]
        system_prompt: Option<String>,
        #[serde(default)]
        model_id: Option<String>,
        #[serde(default)]
        temperature: Option<f32>,
        #[serde(default)]
        max_tokens: Option<u32>,
    },
    ToolCall {
        tool: String,
        #[serde(default)]
        params: HashMap<String, Value>,
        #[serde(default)]
        retry: Option<RetryConfig>,
        #[serde(default)]
        timeout_seconds: Option<u64>,
    },
    Condition {
        expression: String,
        #[serde(default)]
        on_true: BranchTarget,
        #[serde(default)]
        on_false: BranchTarget,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BranchTarget {
    Continue,
    End,
    Goto(String),
}

impl Default for BranchTarget {
    fn default() -> Self {
        Self::Continue
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    pub max: u32,
    #[serde(default = "default_retry_delay")]
    pub delay_seconds: u64,
}

fn default_retry_delay() -> u64 { 3 }

/// Plan 来源
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PlanSource {
    Dynamic {
        goal: String,
        generated_by: String,
    },
    Static {
        workflow_name: String,
    },
}

/// 执行计划
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub id: String,
    pub session_id: String,
    pub source: PlanSource,
    pub steps: Vec<PlanStep>,
    pub status: PlanStatus,
    pub created_at: String,
    #[serde(default)]
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// Plan 中的单个步骤
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub id: String,
    pub label: String,
    pub execution: ExecStep,
    #[serde(default)]
    pub status: StepStatus,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
}

impl Default for StepStatus {
    fn default() -> Self {
        Self::Pending
    }
}

/// 执行进度事件（通过 Tauri event 发送到前端）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanProgressEvent {
    pub plan_id: String,
    pub session_id: String,
    pub event_type: String, // "step_started" | "step_completed" | "step_failed" | "plan_completed" | "plan_failed" | "paused" | "cancelled"
    pub step_index: Option<usize>,
    pub step_label: Option<String>,
    pub result_summary: Option<String>,
    pub error: Option<String>,
    pub total_steps: usize,
    pub completed_steps: usize,
}

/// DB record for execution_plans table
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlanRecord {
    pub id: String,
    pub session_id: String,
    pub source: String,
    pub goal: Option<String>,
    pub plan_json: String,
    pub status: String,
    pub created_at: String,
    pub finished_at: Option<String>,
}

/// DB record for execution_plan_steps table
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStepRecord {
    pub id: String,
    pub plan_id: String,
    pub step_index: i32,
    pub label: String,
    pub step_type: String,
    pub status: String,
    pub result_json: Option<String>,
    pub error: Option<String>,
    pub started_at: Option<String>,
    pub duration_ms: Option<i64>,
}

/// 将 ExecStep 变体映射为类型字符串（用于 checkpoint）
pub fn exec_step_type_name(step: &ExecStep) -> &'static str {
    match step {
        ExecStep::AgentTask { .. } => "agent_task",
        ExecStep::LlmCall { .. } => "llm_call",
        ExecStep::ToolCall { .. } => "tool_call",
        ExecStep::Condition { .. } => "condition",
    }
}

/// 运行时执行句柄，用于外部控制暂停/取消
pub struct ExecutionHandle {
    pub cancel_flag: Arc<AtomicBool>,
    pub pause_flag: Arc<AtomicBool>,
    pub session_id: String,
    pub plan_id: String,
}

impl ExecutionHandle {
    pub fn new(session_id: String, plan_id: String) -> Self {
        Self {
            cancel_flag: Arc::new(AtomicBool::new(false)),
            pause_flag: Arc::new(AtomicBool::new(false)),
            session_id,
            plan_id,
        }
    }

    pub fn cancel(&self) {
        self.cancel_flag.store(true, Ordering::Relaxed);
    }

    pub fn pause(&self) {
        self.pause_flag.store(true, Ordering::Relaxed);
    }

    pub fn resume(&self) {
        self.pause_flag.store(false, Ordering::Relaxed);
    }
}
```

- [ ] **Step 3: 创建 execution/error.rs**

```rust
use crate::error::AppError;

#[derive(Debug, thiserror::Error)]
pub enum ExecutionError {
    #[error("Plan not found: {0}")]
    PlanNotFound(String),
    #[error("Step execution failed at step {step}: {message}")]
    StepFailed { step: usize, message: String },
    #[error("Execution cancelled")]
    Cancelled,
    #[error("Execution paused")]
    Paused,
    #[error("Max retries exceeded for step {step}")]
    MaxRetries { step: usize },
    #[error("Internal execution error: {0}")]
    Internal(String),
}

impl From<ExecutionError> for AppError {
    fn from(e: ExecutionError) -> Self {
        match e {
            ExecutionError::PlanNotFound(id) => AppError::NotFound(format!("Execution plan: {}", id)),
            ExecutionError::StepFailed { step, message } => AppError::Execution(format!("Step {} failed: {}", step, message)),
            ExecutionError::Cancelled => AppError::Execution("Execution cancelled by user".to_string()),
            ExecutionError::Paused => AppError::Execution("Execution paused".to_string()),
            ExecutionError::MaxRetries { step } => AppError::Execution(format!("Max retries exceeded for step {}", step)),
            ExecutionError::Internal(msg) => AppError::Execution(msg),
        }
    }
}
```

Note: Check if `AppError::Execution` variant exists in `error.rs`. If not, add it or use `AppError::Tool` instead.

---

### Task 2: DB schema + Repository

**Files:**
- Modify: `src-tauri/src/db/models.rs`
- Modify: `src-tauri/src/db/repository.rs`

- [ ] **Step 1: 扩展 Session model（models.rs）**

在 Session struct 末尾添加字段：

```rust
/// A session — a series of messages with a title, model, and optional system prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    // ... 现有字段保持不变 ...
    pub id: String,
    pub title: String,
    pub model_id: String,
    pub system_prompt: Option<String>,
    pub persona_id: Option<String>,
    pub config: Option<String>,
    pub title_source: String,
    pub archived: bool,
    pub created_at: String,
    pub updated_at: String,
    // 新增 Phase 1 字段：
    #[serde(default)]
    pub mode: String,  // "chat" | "autonomous"
    #[serde(default)]
    pub execution_status: String, // JSON serialized ExecStatus
    #[serde(default)]
    pub active_plan_id: Option<String>,
}
```

- [ ] **Step 2: 添加 DB 迁移（repository.rs migrate_tables）**

在 `migrate_tables()` 末尾添加 migration v9：

```rust
// Migration v9: add mode, execution_status, active_plan_id to sessions
let has_mode = conn.query_row(
    "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name='mode'",
    [],
    |row| row.get::<_, i32>(0),
).unwrap_or(0);

if has_mode == 0 {
    conn.execute_batch(
        "ALTER TABLE sessions ADD COLUMN mode TEXT NOT NULL DEFAULT 'chat';
         ALTER TABLE sessions ADD COLUMN execution_status TEXT NOT NULL DEFAULT 'idle';
         ALTER TABLE sessions ADD COLUMN active_plan_id TEXT;",
    )?;
}
```

- [ ] **Step 3: 创建 execution_plans 和 execution_plan_steps 表（init_tables）**

在 `init_tables()` 末尾（session_summaries 表之后）添加：

```rust
conn.execute_batch(
    "
    CREATE TABLE IF NOT EXISTS execution_plans (
        id TEXT PRIMARY KEY,
        session_id TEXT NOT NULL,
        source TEXT NOT NULL,
        goal TEXT,
        plan_json TEXT NOT NULL,
        status TEXT NOT NULL DEFAULT 'pending',
        created_at TEXT NOT NULL,
        finished_at TEXT,
        FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS execution_plan_steps (
        id TEXT PRIMARY KEY,
        plan_id TEXT NOT NULL,
        step_index INTEGER NOT NULL,
        label TEXT NOT NULL,
        step_type TEXT NOT NULL,
        status TEXT NOT NULL DEFAULT 'pending',
        result_json TEXT,
        error TEXT,
        started_at TEXT,
        duration_ms INTEGER,
        FOREIGN KEY (plan_id) REFERENCES execution_plans(id) ON DELETE CASCADE
    );

    CREATE INDEX IF NOT EXISTS idx_exec_plans_session ON execution_plans(session_id);
    CREATE INDEX IF NOT EXISTS idx_exec_steps_plan ON execution_plan_steps(plan_id);
    ",
)?;
```

- [ ] **Step 4: 添加 Session 查询方法支持新字段**

在 `list_sessions` 和 `get_session` 的 SQL 查询中添加 mode, execution_status, active_plan_id 列。

修改 `list_sessions()`:
```rust
pub fn list_sessions(&self) -> Result<Vec<Session>> {
    let mut stmt = self.conn.prepare(
        "SELECT id, title, model_id, system_prompt, persona_id, config, title_source, archived, created_at, updated_at, mode, execution_status, active_plan_id
         FROM sessions ORDER BY updated_at DESC",
    )?;
    let sessions = stmt.query_map([], |row| {
        Ok(Session {
            id: row.get(0)?,
            title: row.get(1)?,
            model_id: row.get(2)?,
            system_prompt: row.get(3)?,
            persona_id: row.get(4)?,
            config: row.get(5)?,
            title_source: row.get(6)?,
            archived: row.get::<_, i32>(7)? != 0,
            created_at: row.get(8)?,
            updated_at: row.get(9)?,
            mode: row.get(10)?,
            execution_status: row.get(11)?,
            active_plan_id: row.get(12)?,
        })
    })?.collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(sessions)
}
```

同样修改 `get_session()`。修改 `create_session()` 的 INSERT 添加 mode 和 execution_status 列。

- [ ] **Step 5: 添加 execution_plans CRUD**

在 repository.rs 末尾添加：

```rust
// ── Execution Plans CRUD ──

pub fn insert_execution_plan(&self, plan: &ExecutionPlanRecord) -> Result<()> {
    self.conn.execute(
        "INSERT INTO execution_plans (id, session_id, source, goal, plan_json, status, created_at, finished_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![plan.id, plan.session_id, plan.source, plan.goal, plan.plan_json, plan.status, plan.created_at, plan.finished_at],
    )?;
    Ok(())
}

pub fn update_execution_plan_status(&self, id: &str, status: &str, finished_at: Option<&str>) -> Result<()> {
    self.conn.execute(
        "UPDATE execution_plans SET status = ?2, finished_at = ?3 WHERE id = ?1",
        params![id, status, finished_at],
    )?;
    Ok(())
}

pub fn get_execution_plan(&self, id: &str) -> Result<Option<ExecutionPlanRecord>> {
    let mut stmt = self.conn.prepare(
        "SELECT id, session_id, source, goal, plan_json, status, created_at, finished_at
         FROM execution_plans WHERE id = ?1",
    )?;
    let plan = stmt.query_row(params![id], |row| {
        Ok(ExecutionPlanRecord {
            id: row.get(0)?,
            session_id: row.get(1)?,
            source: row.get(2)?,
            goal: row.get(3)?,
            plan_json: row.get(4)?,
            status: row.get(5)?,
            created_at: row.get(6)?,
            finished_at: row.get(7)?,
        })
    }).optional()?;
    Ok(plan)
}

pub fn list_execution_plans(&self, session_id: &str) -> Result<Vec<ExecutionPlanRecord>> {
    let mut stmt = self.conn.prepare(
        "SELECT id, session_id, source, goal, plan_json, status, created_at, finished_at
         FROM execution_plans WHERE session_id = ?1 ORDER BY created_at DESC",
    )?;
    let plans = stmt.query_map(params![session_id], |row| {
        Ok(ExecutionPlanRecord {
            id: row.get(0)?,
            session_id: row.get(1)?,
            source: row.get(2)?,
            goal: row.get(3)?,
            plan_json: row.get(4)?,
            status: row.get(5)?,
            created_at: row.get(6)?,
            finished_at: row.get(7)?,
        })
    })?.collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(plans)
}

/// 插入/更新单步 checkpoint
pub fn upsert_plan_step(&self, step: &PlanStepRecord) -> Result<()> {
    self.conn.execute(
        "INSERT INTO execution_plan_steps (id, plan_id, step_index, label, step_type, status, result_json, error, started_at, duration_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(id) DO UPDATE SET
            status = excluded.status,
            result_json = excluded.result_json,
            error = excluded.error,
            duration_ms = excluded.duration_ms",
        params![step.id, step.plan_id, step.step_index, step.label, step.step_type,
                step.status, step.result_json, step.error, step.started_at, step.duration_ms],
    )?;
    Ok(())
}

pub fn get_plan_steps(&self, plan_id: &str) -> Result<Vec<PlanStepRecord>> {
    let mut stmt = self.conn.prepare(
        "SELECT id, plan_id, step_index, label, step_type, status, result_json, error, started_at, duration_ms
         FROM execution_plan_steps WHERE plan_id = ?1 ORDER BY step_index ASC",
    )?;
    let steps = stmt.query_map(params![plan_id], |row| {
        Ok(PlanStepRecord {
            id: row.get(0)?,
            plan_id: row.get(1)?,
            step_index: row.get(2)?,
            label: row.get(3)?,
            step_type: row.get(4)?,
            status: row.get(5)?,
            result_json: row.get(6)?,
            error: row.get(7)?,
            started_at: row.get(8)?,
            duration_ms: row.get(9)?,
        })
    })?.collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(steps)
}
```

---

### Task 3: ExecutionRuntime 核心

**Files:**
- Create: `src-tauri/src/execution/runtime.rs`

- [ ] **Step 1: 实现 ExecutionRuntime**

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use chrono::Utc;
use serde_json::Value;
use tokio::sync::{Mutex, mpsc};

use crate::api::provider::ProviderRegistry;
use crate::api::types::{ChatRequest, Message, MessageRole};
use crate::db::repository::Database;
use crate::execution::error::ExecutionError;
use crate::execution::types::*;
use crate::tools::registry::ToolRegistry;

pub struct ExecutionRuntime {
    providers: Arc<Mutex<ProviderRegistry>>,
    tools: Arc<Mutex<ToolRegistry>>,
    db: Arc<Mutex<Database>>,
}

/// Handle 用于从外部控制正在运行的执行
pub struct ExecutionHandle {
    pub cancel_flag: Arc<AtomicBool>,
    pub pause_flag: Arc<AtomicBool>,
}

impl ExecutionRuntime {
    pub fn new(
        providers: Arc<Mutex<ProviderRegistry>>,
        tools: Arc<Mutex<ToolRegistry>>,
        db: Arc<Mutex<Database>>,
    ) -> Self {
        Self { providers, tools, db }
    }

    /// 执行一个 Plan，通过 event_tx 发射进度事件。
    /// 返回 ExecutionHandle 供外部控制暂停/取消。
    pub async fn execute(
        &self,
        plan: ExecutionPlan,
        event_tx: mpsc::Sender<PlanProgressEvent>,
        cancel_flag: Arc<AtomicBool>,
        pause_flag: Arc<AtomicBool>,
    ) -> Result<(), ExecutionError> {
        let plan_id = plan.id.clone();
        let session_id = plan.session_id.clone();
        let total_steps = plan.steps.len();

        // 持久化 Plan
        {
            let db = self.db.lock().await;
            let record = ExecutionPlanRecord {
                id: plan.id.clone(),
                session_id: plan.session_id.clone(),
                source: serde_json::to_string(&plan.source).unwrap_or_default(),
                goal: match &plan.source {
                    PlanSource::Dynamic { goal, .. } => Some(goal.clone()),
                    _ => None,
                },
                plan_json: serde_json::to_string(&plan).unwrap_or_default(),
                status: "running".to_string(),
                created_at: plan.created_at.clone(),
                finished_at: None,
            };
            let _ = db.insert_execution_plan(&record);
        }

        // 更新 session 状态
        {
            let db = self.db.lock().await;
            let status_json = serde_json::to_string(&ExecStatus::Running {
                step_index: 0,
                started_at: Utc::now().to_rfc3339(),
            })
            .unwrap_or_default();
            let _ = db.conn.execute(
                "UPDATE sessions SET mode = 'autonomous', execution_status = ?1, active_plan_id = ?2 WHERE id = ?3",
                rusqlite::params![status_json, plan_id, session_id],
            );
        }

        let mut completed_steps = 0;

        for (i, step) in plan.steps.iter().enumerate() {
            // 检查是否被取消
            if cancel_flag.load(Ordering::Relaxed) {
                self.set_plan_status(&plan_id, "cancelled", None).await;
                self.set_session_status(&session_id, ExecStatus::Cancelled).await;
                let _ = event_tx.send(PlanProgressEvent {
                    plan_id: plan_id.clone(),
                    session_id: session_id.clone(),
                    event_type: "cancelled".to_string(),
                    step_index: Some(i),
                    step_label: None,
                    result_summary: None,
                    error: None,
                    total_steps,
                    completed_steps,
                }).await;
                return Err(ExecutionError::Cancelled);
            }

            // 检查是否暂停
            while pause_flag.load(Ordering::Relaxed) {
                self.set_session_status(&session_id, ExecStatus::Paused {
                    step_index: i,
                    reason: "user_paused".to_string(),
                }).await;
                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                if cancel_flag.load(Ordering::Relaxed) {
                    self.set_plan_status(&plan_id, "cancelled", None).await;
                    self.set_session_status(&session_id, ExecStatus::Cancelled).await;
                    return Err(ExecutionError::Cancelled);
                }
            }

            // 发射 step_started 事件
            let _ = event_tx.send(PlanProgressEvent {
                plan_id: plan_id.clone(),
                session_id: session_id.clone(),
                event_type: "step_started".to_string(),
                step_index: Some(i),
                step_label: Some(step.label.clone()),
                result_summary: None,
                error: None,
                total_steps,
                completed_steps,
            }).await;

            // 执行步骤
            let step_start = Instant::now();
            let result = self.execute_step(&step.execution, &step.id, cancel_flag.clone()).await;

            match result {
                Ok(val) => {
                    let duration = step_start.elapsed().as_millis() as u64;
                    // 保存 checkpoint
                    let db = self.db.lock().await;
                    let record = PlanStepRecord {
                        id: step.id.clone(),
                        plan_id: plan_id.clone(),
                        step_index: i as i32,
                        label: step.label.clone(),
                        step_type: exec_step_type_name(&step.execution).to_string(),
                        status: "completed".to_string(),
                        result_json: Some(val.to_string()),
                        error: None,
                        started_at: Some(Utc::now().to_rfc3339()),
                        duration_ms: Some(duration as i64),
                    };
                    let _ = db.upsert_plan_step(&record);
                    drop(db);

                    completed_steps += 1;

                    let _ = event_tx.send(PlanProgressEvent {
                        plan_id: plan_id.clone(),
                        session_id: session_id.clone(),
                        event_type: "step_completed".to_string(),
                        step_index: Some(i),
                        step_label: Some(step.label.clone()),
                        result_summary: val.as_str().map(|s| s[..s.len().min(100)].to_string()),
                        error: None,
                        total_steps,
                        completed_steps,
                    }).await;
                }
                Err(e) => {
                    let duration = step_start.elapsed().as_millis() as u64;
                    // 保存失败 checkpoint
                    let db = self.db.lock().await;
                    let record = PlanStepRecord {
                        id: step.id.clone(),
                        plan_id: plan_id.clone(),
                        step_index: i as i32,
                        label: step.label.clone(),
                        step_type: exec_step_type_name(&step.execution).to_string(),
                        status: "failed".to_string(),
                        result_json: None,
                        error: Some(e.to_string()),
                        started_at: Some(Utc::now().to_rfc3339()),
                        duration_ms: Some(duration as i64),
                    };
                    let _ = db.upsert_plan_step(&record);
                    drop(db);

                    let _ = event_tx.send(PlanProgressEvent {
                        plan_id: plan_id.clone(),
                        session_id: session_id.clone(),
                        event_type: "step_failed".to_string(),
                        step_index: Some(i),
                        step_label: Some(step.label.clone()),
                        result_summary: None,
                        error: Some(e.to_string()),
                        total_steps,
                        completed_steps,
                    }).await;

                    self.set_plan_status(&plan_id, "failed", Some(&Utc::now().to_rfc3339())).await;
                    self.set_session_status(&session_id, ExecStatus::Failed {
                        step_index: i,
                        error: e.to_string(),
                    }).await;

                    let _ = event_tx.send(PlanProgressEvent {
                        plan_id: plan_id.clone(),
                        session_id: session_id.clone(),
                        event_type: "plan_failed".to_string(),
                        step_index: Some(i),
                        step_label: None,
                        result_summary: None,
                        error: Some(e.to_string()),
                        total_steps,
                        completed_steps,
                    }).await;

                    return Err(e);
                }
            }
        }

        // Plan 完成
        let finished_at = Utc::now().to_rfc3339();
        self.set_plan_status(&plan_id, "completed", Some(&finished_at)).await;
        self.set_session_status(&session_id, ExecStatus::Completed {
            finished_at: finished_at.clone(),
        }).await;

        let _ = event_tx.send(PlanProgressEvent {
            plan_id,
            session_id,
            event_type: "plan_completed".to_string(),
            step_index: None,
            step_label: None,
            result_summary: None,
            error: None,
            total_steps,
            completed_steps,
        }).await;

        Ok(())
    }

    /// 执行单个 ExecStep
    async fn execute_step(
        &self,
        step: &ExecStep,
        step_id: &str,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<Value, ExecutionError> {
        match step {
            ExecStep::ToolCall { tool, params, retry, timeout_seconds } => {
                self.execute_tool(tool, params, retry, *timeout_seconds, cancel_flag).await
            }
            ExecStep::LlmCall { prompt, system_prompt, model_id, temperature, max_tokens } => {
                self.execute_llm(prompt, system_prompt, model_id, *temperature, *max_tokens).await
            }
            ExecStep::AgentTask { instruction, model_id, max_iterations, allowed_tools, temperature } => {
                self.execute_agent_task(instruction, model_id, *max_iterations, allowed_tools, *temperature, cancel_flag).await
            }
            ExecStep::Condition { expression, on_true, on_false } => {
                self.execute_condition(expression, on_true, on_false).await
            }
        }
    }

    async fn execute_tool(
        &self,
        tool: &str,
        params: &std::collections::HashMap<String, Value>,
        retry: &Option<RetryConfig>,
        timeout_seconds: Option<u64>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<Value, ExecutionError> {
        let max_retries = retry.as_ref().map(|r| r.max).unwrap_or(1);
        let delay = retry.as_ref().map(|r| r.delay_seconds).unwrap_or(0);

        let input = Value::Object(params.clone());

        for attempt in 0..max_retries {
            if cancel_flag.load(Ordering::Relaxed) {
                return Err(ExecutionError::Cancelled);
            }

            let tools = self.tools.lock().await;
            let result = tools.execute(tool, input.clone()).await;
            drop(tools);

            match result {
                Ok(val) => return Ok(val),
                Err(e) if attempt < max_retries - 1 => {
                    if delay > 0 {
                        tokio::time::sleep(tokio::time::Duration::from_secs(delay as u64)).await;
                    }
                    continue;
                }
                Err(e) => {
                    return Err(ExecutionError::StepFailed {
                        step: 0, // caller will fill step index
                        message: format!("Tool '{}' failed after {} retries: {}", tool, max_retries, e),
                    });
                }
            }
        }
        Err(ExecutionError::MaxRetries { step: 0 })
    }

    async fn execute_llm(
        &self,
        prompt: &str,
        system_prompt: &Option<String>,
        model_id: &Option<String>,
        temperature: Option<f32>,
        max_tokens: Option<u32>,
    ) -> Result<Value, ExecutionError> {
        // ... LLM call through provider ...
        // Simplified for Phase 1 - will be fully implemented
        Err(ExecutionError::Internal("LLM call not yet implemented".to_string()))
    }

    async fn execute_agent_task(
        &self,
        instruction: &str,
        model_id: &Option<String>,
        max_iterations: Option<usize>,
        allowed_tools: &Option<Vec<String>>,
        temperature: Option<f32>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<Value, ExecutionError> {
        // ... Agent loop through provider ...
        // Simplified for Phase 1 - will be fully implemented in Phase 2
        Err(ExecutionError::Internal("Agent task not yet implemented".to_string()))
    }

    async fn execute_condition(
        &self,
        expression: &str,
        on_true: &BranchTarget,
        on_false: &BranchTarget,
    ) -> Result<Value, ExecutionError> {
        // Simple truthy check for Phase 1
        let trimmed = expression.trim().to_lowercase();
        let is_truthy = !expression.is_empty()
            && !matches!(trimmed.as_str(), "false" | "no" | "0" | "" | "null");
        Ok(Value::Bool(is_truthy))
    }

    async fn set_plan_status(&self, plan_id: &str, status: &str, finished_at: Option<&str>) {
        let db = self.db.lock().await;
        let _ = db.update_execution_plan_status(plan_id, status, finished_at);
    }

    async fn set_session_status(&self, session_id: &str, status: ExecStatus) {
        let db = self.db.lock().await;
        let status_json = serde_json::to_string(&status).unwrap_or_default();
        let _ = db.conn.execute(
            "UPDATE sessions SET execution_status = ?1 WHERE id = ?2",
            rusqlite::params![status_json, session_id],
        );
    }
}
```

Note: The `execute_llm` and `execute_agent_task` are stubs for Phase 1. They will be fully implemented in Phase 2 when the planner is added. Phase 1 focuses on the ToolCall and Condition step execution, plus the runtime infrastructure (checkpoint, pause/cancel, event emission).

---

### Task 4: IPC 命令

**Files:**
- Create: `src-tauri/src/commands/execution.rs`

- [ ] **Step 1: 创建 commands/execution.rs**

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::collections::HashMap;

use chrono::Utc;
use tauri::State;
use tokio::sync::{Mutex, mpsc};

use crate::execution::error::ExecutionError;
use crate::execution::runtime::ExecutionRuntime;
use crate::execution::types::*;
use crate::state::AppState;

#[tauri::command]
pub async fn execute_plan(
    state: State<'_, AppState>,
    session_id: String,
    plan_json: String,
) -> Result<(), String> {
    let plan: ExecutionPlan = serde_json::from_str(&plan_json)
        .map_err(|e| format!("Invalid plan JSON: {}", e))?;

    let handle = ExecutionHandle::new(session_id.clone(), plan.id.clone());

    let runtime = ExecutionRuntime::new(
        state.providers.clone(),
        state.tools.clone(),
        state.db.clone(),
    );

    let (event_tx, mut event_rx) = mpsc::channel::<PlanProgressEvent>(32);

    // Spawn execution in background
    let plan_id = plan.id.clone();
    let sid = session_id.clone();
    let cf = handle.cancel_flag.clone();
    let pf = handle.pause_flag.clone();
    let app_handle = state.app_handle.clone();

    tokio::spawn(async move {
        match runtime.execute(plan, event_tx, cf, pf).await {
            Ok(()) => {
                log::info!("Plan {} completed successfully", plan_id);
            }
            Err(e) => {
                log::error!("Plan {} failed: {}", plan_id, e);
                let _ = app_handle.emit("plan_progress", PlanProgressEvent {
                    plan_id: plan_id.clone(),
                    session_id: sid.clone(),
                    event_type: "plan_failed".to_string(),
                    step_index: None,
                    step_label: None,
                    result_summary: None,
                    error: Some(e.to_string()),
                    total_steps: 0,
                    completed_steps: 0,
                });
            }
        }
    });

    // Forward events from runtime to frontend
    let app_handle_clone = state.app_handle.clone();
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let _ = app_handle_clone.emit("plan_progress", event);
        }
    });

    // Store execution handle
    {
        let mut executions = state.active_executions.lock().await;
        executions.insert(session_id.clone(), handle);
    }

    Ok(())
}

#[tauri::command]
pub async fn pause_execution(state: State<'_, AppState>, session_id: String) -> Result<(), String> {
    let mut executions = state.active_executions.lock().await;
    if let Some(exec) = executions.get(&session_id) {
        exec.pause();
        Ok(())
    } else {
        Err("No active execution for this session".to_string())
    }
}

#[tauri::command]
pub async fn resume_execution(state: State<'_, AppState>, session_id: String) -> Result<(), String> {
    let mut executions = state.active_executions.lock().await;
    if let Some(exec) = executions.get(&session_id) {
        exec.resume();
        Ok(())
    } else {
        Err("No active execution for this session".to_string())
    }
}

#[tauri::command]
pub async fn cancel_execution(state: State<'_, AppState>, session_id: String) -> Result<(), String> {
    let mut executions = state.active_executions.lock().await;
    if let Some(exec) = executions.remove(&session_id) {
        exec.cancel();
        Ok(())
    } else {
        Err("No active execution for this session".to_string())
    }
}

#[tauri::command]
pub async fn get_execution_status(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Option<String>, String> {
    let db = state.db.lock().await;
    let sess = db.get_session(&session_id)
        .map_err(|e| e.to_string())?;
    Ok(sess.map(|s| s.execution_status))
}

#[tauri::command]
pub async fn get_plan_detail(
    state: State<'_, AppState>,
    plan_id: String,
) -> Result<Option<serde_json::Value>, String> {
    let db = state.db.lock().await;

    let plan_record = db.get_execution_plan(&plan_id)
        .map_err(|e| e.to_string())?;

    let plan_record = match plan_record {
        Some(p) => p,
        None => return Ok(None),
    };

    let plan: ExecutionPlan = serde_json::from_str(&plan_record.plan_json)
        .map_err(|e| format!("Failed to parse plan: {}", e))?;

    let steps = db.get_plan_steps(&plan_id)
        .map_err(|e| e.to_string())?;

    Ok(Some(serde_json::json!({
        "plan": plan,
        "step_records": steps,
    })))
}
```

- [ ] **Step 2: 注册命令到 commands/mod.rs**

在 `commands/mod.rs` 中添加：

```rust
pub mod execution;
// ...
pub use execution::*;
```

---

### Task 5: 集成（state.rs + lib.rs + error.rs）

**Files:**
- Modify: `src-tauri/src/state.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/error.rs`

- [ ] **Step 1: 扩展 AppState（state.rs）**

在 struct 末尾添加：

```rust
use crate::execution::types::ExecutionHandle;
use std::collections::HashMap;
// ...
pub struct AppState {
    // ... 现有字段 ...
    pub active_executions: Arc<Mutex<HashMap<String, ExecutionHandle>>>,
}
```

在 `AppState::new()` 中初始化：

```rust
Ok(Self {
    // ... 现有字段 ...
    active_executions: Arc::new(Mutex::new(HashMap::new())),
})
```

- [ ] **Step 2: 添加 Execution 错误变体**

在 `error.rs` 的 `AppError` enum 中添加：

```rust
#[error("Execution error: {0}")]
Execution(String),
```

- [ ] **Step 3: 注册 execution 模块和命令到 lib.rs**

在 `lib.rs` 的 mod 声明区域添加：

```rust
pub mod execution;
```

在 invoke_handler 中添加：

```rust
commands::execute_plan,
commands::pause_execution,
commands::resume_execution,
commands::cancel_execution,
commands::get_execution_status,
commands::get_plan_detail,
```

---

### Task 6: 前端类型 + API

**Files:**
- Modify: `src-ui/src/types/index.ts`
- Modify: `src-ui/src/api/tauri.ts`

- [ ] **Step 1: 添加 TS 类型（types/index.ts）**

```typescript
// ── Execution Types ──

export type SessionMode = 'chat' | 'autonomous';

export type ExecStatus =
  | { type: 'idle' }
  | { type: 'running'; step_index: number; started_at: string }
  | { type: 'paused'; step_index: number; reason: string }
  | { type: 'completed'; finished_at: string }
  | { type: 'failed'; step_index: number; error: string }
  | { type: 'cancelled' };

export type ExecStep =
  | { type: 'agent_task'; instruction: string; model_id?: string; max_iterations?: number; allowed_tools?: string[]; temperature?: number }
  | { type: 'llm_call'; prompt: string; system_prompt?: string; model_id?: string; temperature?: number; max_tokens?: number }
  | { type: 'tool_call'; tool: string; params: Record<string, unknown>; retry?: { max: number; delay_seconds: number }; timeout_seconds?: number }
  | { type: 'condition'; expression: string; on_true?: string; on_false?: string };

export interface PlanStep {
  id: string;
  label: string;
  execution: ExecStep;
  status: 'pending' | 'running' | 'completed' | 'failed' | 'skipped';
  result?: unknown;
  error?: string | null;
  started_at?: string | null;
  duration_ms?: number | null;
}

export interface ExecutionPlan {
  id: string;
  session_id: string;
  source: { type: 'dynamic'; goal: string; generated_by: string } | { type: 'static'; workflow_name: string };
  steps: PlanStep[];
  status: 'pending' | 'running' | 'completed' | 'failed' | 'cancelled';
  created_at: string;
  finished_at?: string | null;
}

export interface PlanProgressEvent {
  plan_id: string;
  session_id: string;
  event_type: 'step_started' | 'step_completed' | 'step_failed' | 'plan_completed' | 'plan_failed' | 'paused' | 'cancelled';
  step_index?: number | null;
  step_label?: string | null;
  result_summary?: string | null;
  error?: string | null;
  total_steps: number;
  completed_steps: number;
}

export interface PlanDetail {
  plan: ExecutionPlan;
  step_records: Array<{
    id: string;
    plan_id: string;
    step_index: number;
    label: string;
    step_type: string;
    status: string;
    result_json?: string | null;
    error?: string | null;
    started_at?: string | null;
    duration_ms?: number | null;
  }>;
}
```

- [ ] **Step 2: 添加 API 包装（api/tauri.ts）**

```typescript
// ── Execution Commands ──

export async function executePlan(sessionId: string, planJson: string): Promise<void> {
  return invoke('execute_plan', { sessionId, planJson });
}

export async function pauseExecution(sessionId: string): Promise<void> {
  return invoke('pause_execution', { sessionId });
}

export async function resumeExecution(sessionId: string): Promise<void> {
  return invoke('resume_execution', { sessionId });
}

export async function cancelExecution(sessionId: string): Promise<void> {
  return invoke('cancel_execution', { sessionId });
}

export async function getExecutionStatus(sessionId: string): Promise<string | null> {
  return invoke('get_execution_status', { sessionId });
}

export async function getPlanDetail(planId: string): Promise<PlanDetail | null> {
  return invoke('get_plan_detail', { planId });
}
```

---

### Task 7: 前端 Store

**Files:**
- Modify: `src-ui/src/store/sessionSlice.ts`

- [ ] **Step 1: 扩展 SessionSlice**

添加新的状态和 actions：

```typescript
import type { SessionMode, ExecStatus, ExecutionPlan, PlanProgressEvent, PlanDetail } from '../types';

export interface SessionSlice {
  // ... 现有字段 ...
  sessionMode: SessionMode;
  executionStatus: ExecStatus;
  activePlan: ExecutionPlan | null;
  planProgress: PlanProgressEvent | null;
  planHistory: string[];

  // ... 现有 action ...
  setSessionMode: (mode: SessionMode) => void;
  setExecutionStatus: (status: ExecStatus) => void;
  setActivePlan: (plan: ExecutionPlan | null) => void;
  executePlan: (sessionId: string, plan: ExecutionPlan) => Promise<void>;
  pauseExecution: (sessionId: string) => Promise<void>;
  resumeExecution: (sessionId: string) => Promise<void>;
  cancelExecution: (sessionId: string) => Promise<void>;
  loadPlanDetail: (planId: string) => Promise<void>;
}
```

实现（在 createSessionSlice 中）：

```typescript
// 初始状态
sessionMode: 'chat',
executionStatus: { type: 'idle' },
activePlan: null,
planProgress: null,
planHistory: [],

// Actions
setSessionMode: (mode) => set({ sessionMode: mode }),
setExecutionStatus: (status) => set({ executionStatus: status }),
setActivePlan: (plan) => set({ activePlan: plan }),

executePlan: async (sessionId, plan) => {
  set({ sessionMode: 'autonomous', executionStatus: { type: 'running', step_index: 0, started_at: new Date().toISOString() }, activePlan: plan });
  try {
    await api.executePlan(sessionId, JSON.stringify(plan));
  } catch (err) {
    set({ executionStatus: { type: 'failed', step_index: 0, error: String(err) } });
  }
},

pauseExecution: async (sessionId) => {
  try {
    await api.pauseExecution(sessionId);
  } catch (err) {
    console.error('Failed to pause:', err);
  }
},

resumeExecution: async (sessionId) => {
  try {
    await api.resumeExecution(sessionId);
  } catch (err) {
    console.error('Failed to resume:', err);
  }
},

cancelExecution: async (sessionId) => {
  try {
    await api.cancelExecution(sessionId);
    set({ activePlan: null, executionStatus: { type: 'idle' }, planProgress: null });
  } catch (err) {
    console.error('Failed to cancel:', err);
  }
},

loadPlanDetail: async (planId) => {
  try {
    const detail = await api.getPlanDetail(planId);
    if (detail) {
      set({ activePlan: detail.plan });
    }
  } catch (err) {
    console.error('Failed to load plan detail:', err);
  }
},
```

- [ ] **Step 2: 添加 plan_progress 事件监听**

在 app 初始化或 SessionList 组件中，添加 Tauri 事件监听：
（在 `App.tsx` 或 `SessionContext` 中添加）

```typescript
import { listen } from '@tauri-apps/api/event';
import type { PlanProgressEvent } from '../types';

// In component useEffect:
useEffect(() => {
  const unlisten = listen<PlanProgressEvent>('plan_progress', (event) => {
    const payload = event.payload;
    // Update store based on event_type
    const store = useSessionStore.getState();
    
    switch (payload.event_type) {
      case 'step_completed':
      case 'step_failed':
      case 'step_started':
        store.setPlanProgress(payload);
        break;
      case 'plan_completed':
        store.setExecutionStatus({ type: 'completed', finished_at: new Date().toISOString() });
        store.setPlanProgress(payload);
        break;
      case 'plan_failed':
        store.setExecutionStatus({ type: 'failed', step_index: payload.step_index ?? 0, error: payload.error ?? 'Unknown error' });
        store.setPlanProgress(payload);
        break;
      case 'cancelled':
        store.setExecutionStatus({ type: 'idle' });
        store.setActivePlan(null);
        store.setPlanProgress(null);
        break;
      case 'paused':
        store.setExecutionStatus({ type: 'paused', step_index: payload.step_index ?? 0, reason: 'user_paused' });
        break;
    }
  });

  return () => { unlisten(); };
}, []);
```

---

### Task 8: 前端组件

**Files:**
- Create: `src-ui/src/components/ExecutionStatusBar.tsx`
- Create: `src-ui/src/components/PlanTimeline.tsx`

- [ ] **Step 1: ExecutionStatusBar 组件**

```tsx
import React from 'react';
import { useSessionStore } from '../store/sessionSlice'; // adjust import path

export const ExecutionStatusBar: React.FC = () => {
  const { sessionMode, executionStatus, activePlan, pauseExecution, resumeExecution, cancelExecution } = useSessionStore();
  const currentSessionId = useSessionStore((s: any) => s.currentSession?.id);

  if (sessionMode !== 'autonomous' || !activePlan) return null;

  const isRunning = executionStatus.type === 'running';
  const isPaused = executionStatus.type === 'paused';
  const isCompleted = executionStatus.type === 'completed';
  const isFailed = executionStatus.type === 'failed';

  return (
    <div className="flex items-center gap-2 px-3 py-1.5 bg-purple-50 dark:bg-purple-900/20 border-b border-purple-200 dark:border-purple-800 text-sm">
      <span className="flex items-center gap-1">
        {isRunning && <span className="w-2 h-2 bg-green-500 rounded-full animate-pulse" />}
        {isPaused && <span className="w-2 h-2 bg-yellow-500 rounded-full" />}
        {isCompleted && <span className="w-2 h-2 bg-blue-500 rounded-full" />}
        {isFailed && <span className="w-2 h-2 bg-red-500 rounded-full" />}
        <span className="font-medium text-purple-700 dark:text-purple-300">
          {isRunning && '执行中...'}
          {isPaused && '已暂停'}
          {isCompleted && '执行完成'}
          {isFailed && '执行失败'}
        </span>
      </span>

      <span className="text-gray-500 dark:text-gray-400 text-xs ml-2">
        {activePlan.steps.length} 步计划
      </span>

      <div className="ml-auto flex gap-1">
        {isRunning && (
          <>
            <button onClick={() => pauseExecution(currentSessionId)} className="px-2 py-0.5 text-xs rounded bg-yellow-100 dark:bg-yellow-800 hover:bg-yellow-200">
              暂停
            </button>
            <button onClick={() => cancelExecution(currentSessionId)} className="px-2 py-0.5 text-xs rounded bg-red-100 dark:bg-red-800 hover:bg-red-200">
              取消
            </button>
          </>
        )}
        {isPaused && (
          <>
            <button onClick={() => resumeExecution(currentSessionId)} className="px-2 py-0.5 text-xs rounded bg-green-100 dark:bg-green-800 hover:bg-green-200">
              继续
            </button>
            <button onClick={() => cancelExecution(currentSessionId)} className="px-2 py-0.5 text-xs rounded bg-red-100 dark:bg-red-800 hover:bg-red-200">
              取消
            </button>
          </>
        )}
      </div>
    </div>
  );
};
```

- [ ] **Step 2: PlanTimeline 组件**

```tsx
import React from 'react';
import { useSessionStore } from '../store/sessionSlice';

export const PlanTimeline: React.FC = () => {
  const { activePlan, planProgress, sessionMode } = useSessionStore();

  if (sessionMode !== 'autonomous' || !activePlan) return null;

  const progress = planProgress;

  return (
    <div className="mx-3 my-2 p-3 bg-gray-50 dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700">
      <div className="text-xs font-medium text-gray-500 dark:text-gray-400 mb-2">
        执行计划 · {progress ? `${progress.completed_steps}/${progress.total_steps}` : `${activePlan.steps.length} 步`}
      </div>
      <div className="space-y-1">
        {activePlan.steps.map((step, i) => {
          const isCurrent = progress?.step_index === i && progress?.event_type === 'step_started';
          const isDone = step.status === 'completed' || (progress?.step_index !== null && progress?.step_index !== undefined && i < progress.step_index);
          const isFailed = step.status === 'failed' || (progress?.step_index === i && progress?.event_type === 'step_failed');
          const isSkipped = step.status === 'skipped';

          let icon = '○';
          let color = 'text-gray-400';
          if (isDone) { icon = '●'; color = 'text-green-500'; }
          if (isCurrent) { icon = '◉'; color = 'text-blue-500'; }
          if (isFailed) { icon = '✕'; color = 'text-red-500'; }
          if (isSkipped) { icon = '—'; color = 'text-gray-300'; }

          return (
            <div key={step.id} className="flex items-center gap-2 text-xs">
              <span className={`${color} w-4 text-center`}>{icon}</span>
              <span className={`flex-1 ${isDone || isCurrent ? 'text-gray-900 dark:text-gray-100' : 'text-gray-500 dark:text-gray-500'}`}>
                {step.label}
              </span>
              {step.duration_ms != null && (
                <span className="text-gray-400 tabular-nums">{(step.duration_ms / 1000).toFixed(1)}s</span>
              )}
              {isFailed && step.error && (
                <span className="text-red-400 truncate max-w-[200px]" title={step.error}>{step.error}</span>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
};
```

- [ ] **Step 3: 集成到 App.tsx**

在 ChatArea 或主布局中添加：

```tsx
<>
  <ExecutionStatusBar />
  <PlanTimeline />
  {/* 现有消息列表 */}
</>
```

---

## 自审检查清单

- [ ] **Spec 覆盖**：Task 1(类型) → 2(DB) → 3(Runtime) → 4(IPC) → 5(集成) → 6-8(前端)，覆盖 Phase 1 所有需求
- [ ] **无占位符**：所有步骤包含具体代码，无 TBD/TODO
- [ ] **类型一致性**：ExecStep/ExecutionPlan/ExecStatus 在 Rust 和 TS 两侧定义一致
- [ ] **向后兼容**：Session 新增字段有 `#[serde(default)]`，DB 迁移使用 ALTER TABLE ADD COLUMN
- [ ] **错误处理**：ExecutionError 定义完整，通过 AppError::Execution 传递
