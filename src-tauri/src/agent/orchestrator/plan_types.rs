//! 执行计划的类型定义：会话模式、步骤类型、计划状态、进度事件与执行句柄。
//!
//! 本模块只描述数据结构与状态迁移的取值，不包含任何执行逻辑。
//! 约束：所有会写入 SQLite 的枚举必须保持 `serde` 的序列化取值不变，
//! 否则历史数据将无法反序列化；需要字符串形式时使用各枚举的 `as_str()`。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::agent::orchestrator::plan_error::ExecutionError;

// ── 会话模式与状态 ──

/// Session 执行模式
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum SessionMode {
    #[default]
    Chat,
    Autonomous,
}

/// 一次计划运行的执行范围。
///
/// `Autonomous` 保留原有行为：运行时接管会话执行状态
/// （`sessions.mode`、`active_plan_id`、`execution_status`）。
/// `Interactive` 用于聊天轮次：计划仍会被持久化，但不触碰会话状态，
/// 会话因此保持普通的 `chat` 模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanRunMode {
    Autonomous,
    Interactive,
}


/// Session 执行状态（序列化为 JSON 存储在 DB）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ExecStatus {
    #[default]
    Idle,
    Running { step_index: usize, started_at: String },
    Paused { step_index: usize, reason: String },
    Completed { finished_at: String },
    Failed { step_index: usize, error: String },
    Cancelled,
}


// ── 执行步骤类型 ──

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
#[derive(Default)]
pub enum BranchTarget {
    #[default]
    Continue,
    End,
    Goto(String),
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

// ── 计划（Plan） ──

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

impl PlanStatus {
    /// 返回写入 `execution_plans.status` 的字符串。
    ///
    /// 取值与 `serde` 的 `snake_case` 序列化结果完全一致，改动会破坏
    /// 已落库的历史数据，因此这里集中定义，调用点不再硬编码字面量。
    pub fn as_str(&self) -> &'static str {
        match self {
            PlanStatus::Pending => "pending",
            PlanStatus::Running => "running",
            PlanStatus::Completed => "completed",
            PlanStatus::Failed => "failed",
            PlanStatus::Cancelled => "cancelled",
        }
    }
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
#[derive(Default)]
pub enum StepStatus {
    #[default]
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
}

impl StepStatus {
    /// 返回写入 `plan_steps.status` 的字符串。
    ///
    /// 与 [`PlanStatus::as_str`] 相同，取值必须与 `serde` 序列化结果一致。
    pub fn as_str(&self) -> &'static str {
        match self {
            StepStatus::Pending => "pending",
            StepStatus::Running => "running",
            StepStatus::Completed => "completed",
            StepStatus::Failed => "failed",
            StepStatus::Skipped => "skipped",
        }
    }
}



// ── 运行结果 ──

/// 计划运行期间成功完成的单个步骤。
#[derive(Debug, Clone)]
pub struct CompletedStep {
    pub index: usize,
    pub id: String,
    pub label: String,
    pub result: Value,
}

/// 所有步骤均成功完成时的运行结果。
#[derive(Debug, Clone, Default)]
pub struct PlanOutcome {
    pub plan_id: String,
    pub session_id: String,
    pub completed: Vec<CompletedStep>,
}

impl PlanOutcome {
    /// 把已完成步骤中非空的文本结果拼接起来。
    pub fn combined_result(&self) -> Option<String> {
        combine_step_results(&self.completed)
    }
}

/// 计划被接受且执行已经开始之后的失败。
///
/// 携带失败前已完成步骤的结果，让调用方可以展示部分产出，
/// 而不是丢弃已经发生的副作用。
#[derive(Debug, Clone)]
pub struct PlanFailure {
    pub completed: Vec<CompletedStep>,
    pub failed_step: String,
    pub message: String,
}

impl PlanFailure {
    /// 把已完成步骤中非空的文本结果拼接起来。
    pub fn combined_result(&self) -> Option<String> {
        combine_step_results(&self.completed)
    }

    /// 转换回旧版错误类型，使既有调用方无需改动。
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

// ── 事件 ──

/// 执行日志条目（发送到前端用于调试）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionLogEntry {
    pub timestamp: String,
    /// 取值：`info` | `warn` | `error` | `debug`
    pub level: String,
    /// 取值：`intent` | `planner` | `execution` | `runtime`
    pub step: String,
    pub message: String,
    #[serde(default)]
    pub detail: Option<String>,
}

impl ExecutionLogEntry {
    /// 用当前时间构造一条日志条目，`detail` 默认为空。
    pub fn new(level: &str, step: &str, message: String) -> Self {
        Self {
            timestamp: chrono::Utc::now().to_rfc3339(),
            level: level.to_string(),
            step: step.to_string(),
            message,
            detail: None,
        }
    }

    /// 附加结构化细节（例如错误栈或参数快照）。
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

impl PlanProgressEvent {
    /// 构造一个进度事件骨架。
    ///
    /// 计划与步骤标识、事件类型、进度计数是每个事件都要带的公共部分；
    /// 步骤序号、标签、结果摘要与错误信息用构建方法按需补充。
    pub fn new(
        plan_id: impl Into<String>,
        session_id: impl Into<String>,
        event_type: impl Into<String>,
        total_steps: usize,
        completed_steps: usize,
    ) -> Self {
        Self {
            plan_id: plan_id.into(),
            session_id: session_id.into(),
            event_type: event_type.into(),
            step_index: None,
            step_label: None,
            result_summary: None,
            error: None,
            total_steps,
            completed_steps,
        }
    }

    /// 附加步骤序号（从 0 开始）。
    pub fn with_step_index(mut self, index: usize) -> Self {
        self.step_index = Some(index);
        self
    }

    /// 附加步骤标签。
    pub fn with_step_label(mut self, label: impl Into<String>) -> Self {
        self.step_label = Some(label.into());
        self
    }

    /// 附加结果摘要。
    pub fn with_result_summary(mut self, summary: impl Into<String>) -> Self {
        self.result_summary = Some(summary.into());
        self
    }

    /// 附加错误信息。
    pub fn with_error(mut self, error: impl Into<String>) -> Self {
        self.error = Some(error.into());
        self
    }
}

// ── 数据库记录 ──

pub use crate::core::db::models::{ExecutionPlanRecord, PlanStepRecord};

// ── 执行句柄 ──

/// 运行时执行句柄，用于外部控制暂停/取消
pub struct ExecutionHandle {
    pub cancel_flag: Arc<AtomicBool>,
    pub pause_flag: Arc<AtomicBool>,
    pub session_id: String,
    pub plan_id: String,
}

impl ExecutionHandle {
    /// 为一次计划运行创建句柄，初始状态为「未取消、未暂停」。
    pub fn new(session_id: String, plan_id: String) -> Self {
        Self {
            cancel_flag: Arc::new(AtomicBool::new(false)),
            pause_flag: Arc::new(AtomicBool::new(false)),
            session_id,
            plan_id,
        }
    }

    /// 请求取消：运行循环在下一个检查点退出。
    pub fn cancel(&self) {
        self.cancel_flag.store(true, Ordering::Relaxed);
    }

    /// 请求暂停：运行循环在步骤之间自旋等待，直到 [`Self::resume`]。
    pub fn pause(&self) {
        self.pause_flag.store(true, Ordering::Relaxed);
    }

    /// 解除暂停。
    pub fn resume(&self) {
        self.pause_flag.store(false, Ordering::Relaxed);
    }
}
