use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::agent::orchestrator::plan_error::ExecutionError;

// ── Session Mode & Status ──

/// Session 执行模式
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SessionMode {
    Chat,
    Autonomous,
}

/// Execution scope for a plan run.
///
/// `Autonomous` keeps the original behavior: the runtime owns the session
/// execution state (`sessions.mode`, `active_plan_id`, `execution_status`).
/// `Interactive` is used for a chat turn: the plan is still persisted, but
/// session state is left untouched so the session remains a normal `chat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanRunMode {
    Autonomous,
    Interactive,
}

impl Default for SessionMode {
    fn default() -> Self {
        Self::Chat
    }
}

/// Session 执行状态（序列化为 JSON 存储在 DB）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ExecStatus {
    Idle,
    Running { step_index: usize, started_at: String },
    Paused { step_index: usize, reason: String },
    Completed { finished_at: String },
    Failed { step_index: usize, error: String },
    Cancelled,
}

impl Default for ExecStatus {
    fn default() -> Self {
        Self::Idle
    }
}

// ── Execution Step Types ──

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
    /// 执行一个预定义的 YAML 工作流作为子 Plan
    Pipeline {
        name: String,
        #[serde(default)]
        params: HashMap<String, String>,
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

fn default_retry_delay() -> u64 {
    3
}

/// 将 ExecStep 变体映射为类型字符串（用于 checkpoint）
pub fn exec_step_type_name(step: &ExecStep) -> &'static str {
    match step {
        ExecStep::AgentTask { .. } => "agent_task",
        ExecStep::LlmCall { .. } => "llm_call",
        ExecStep::ToolCall { .. } => "tool_call",
        ExecStep::Condition { .. } => "condition",
        ExecStep::Pipeline { .. } => "pipeline",
    }
}

// ── Plan ──

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

// ── Run Outcome ──

/// A step that finished successfully during a plan run.
#[derive(Debug, Clone)]
pub struct CompletedStep {
    pub index: usize,
    pub id: String,
    pub label: String,
    pub result: Value,
}

/// Result of a plan run where every step completed.
#[derive(Debug, Clone, Default)]
pub struct PlanOutcome {
    pub plan_id: String,
    pub session_id: String,
    pub completed: Vec<CompletedStep>,
}

impl PlanOutcome {
    /// Join the non-empty textual results of the completed steps.
    pub fn combined_result(&self) -> Option<String> {
        combine_step_results(&self.completed)
    }
}

/// Failure after a plan was accepted and execution had already started.
///
/// Carries the results of the steps that finished before the failure so the
/// caller can surface partial output instead of discarding side effects.
#[derive(Debug, Clone)]
pub struct PlanFailure {
    pub completed: Vec<CompletedStep>,
    pub failed_step: String,
    pub message: String,
}

impl PlanFailure {
    /// Join the non-empty textual results of the completed steps.
    pub fn combined_result(&self) -> Option<String> {
        combine_step_results(&self.completed)
    }

    /// Convert back to the legacy error type so existing callers are unaffected.
    pub fn to_execution_error(&self) -> ExecutionError {
        if self.message == ExecutionError::Cancelled.to_string() {
            ExecutionError::Cancelled
        } else {
            ExecutionError::StepFailed {
                step: self.completed.len(),
                message: self.message.clone(),
            }
        }
    }
}

fn combine_step_results(steps: &[CompletedStep]) -> Option<String> {
    let parts: Vec<String> = steps
        .iter()
        .filter_map(|step| match &step.result {
            Value::String(text) if !text.trim().is_empty() => Some(text.clone()),
            Value::Null => None,
            other => Some(other.to_string()),
        })
        .collect();

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}

// ── Events ──

/// 执行日志条目（发送到前端用于调试）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionLogEntry {
    pub timestamp: String,
    pub level: String,       // "info" | "warn" | "error" | "debug"
    pub step: String,        // "intent" | "planner" | "execution" | "runtime"
    pub message: String,
    #[serde(default)]
    pub detail: Option<String>,
}

impl ExecutionLogEntry {
    pub fn new(level: &str, step: &str, message: String) -> Self {
        Self {
            timestamp: chrono::Utc::now().to_rfc3339(),
            level: level.to_string(),
            step: step.to_string(),
            message,
            detail: None,
        }
    }
    pub fn with_detail(mut self, detail: String) -> Self {
        self.detail = Some(detail);
        self
    }
}

/// 执行进度事件（通过 Tauri event 发送到前端）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanProgressEvent {
    pub plan_id: String,
    pub session_id: String,
    pub event_type: String,
    pub step_index: Option<usize>,
    pub step_label: Option<String>,
    pub result_summary: Option<String>,
    pub error: Option<String>,
    pub total_steps: usize,
    pub completed_steps: usize,
}

// ── DB Records ──

pub use crate::core::db::models::{ExecutionPlanRecord, PlanStepRecord};

// ── Execution Handle ──

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
