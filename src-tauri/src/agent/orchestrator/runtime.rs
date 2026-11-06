//! 计划执行运行时：按顺序驱动 `ExecutionPlan` 的步骤，并同步落库与上报进度。
//!
//! 职责边界：
//! - 只负责「执行 + 状态持久化 + 事件上报」，不负责生成计划（见 `planner`）；
//! - 单个步骤的调度细节在 `executor` 子模块；
//! - 持久化失败只记日志、不改变控制流，避免因为历史记录写失败而中断正在跑的步骤。
//!
//! 约束：进度事件字段与写入 DB 的状态取值是前端与断点续跑的依据，不可随意改动。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use chrono::Utc;
use serde_json::Value;
use tokio::sync::{Mutex, mpsc};

use crate::agent::fast_loop::ToolLoop;
use crate::core::util::truncate_chars;
use crate::llm::provider::ProviderRegistry;
use rig::completion::Message;
use crate::core::db::repository::Database;
use crate::agent::orchestrator::plan_error::ExecutionError;
use crate::agent::orchestrator::plan_types::*;
use crate::agent::tools::registry::ToolRegistry;
use tauri::Emitter;

mod executor;

/// 写入 `sessions.mode` 的自主执行标记。
const SESSION_MODE_AUTONOMOUS: &str = "autonomous";
/// 结果摘要最多保留的字符数（按字符而非字节，避免切断中文）。
const RESULT_SUMMARY_CHARS: usize = 100;
/// 计划来源描述中目标文本最多保留的字符数。
const GOAL_DESC_CHARS: usize = 50;

/// 执行计划的运行时。
///
/// 持有 LLM provider、工具注册表与数据库连接；每次 [`Self::run`] 会按顺序
/// 执行计划中的步骤，并在每个关键节点写 checkpoint、向前端推送进度事件。
pub struct ExecutionRuntime {
    providers: Arc<Mutex<ProviderRegistry>>,
    tools: Arc<Mutex<ToolRegistry>>,
    db: Arc<Mutex<Database>>,
    app_handle: Option<tauri::AppHandle>,
    /// 运行级工具白名单；`None` 表示不限制。
    allowed_tools: Option<Vec<String>>,
    /// 运行级工具总开关。
    tools_enabled: bool,
    /// agent 任务的可选系统提示词（缺省时回落到内置文案）。
    system_prompt: Option<String>,
    /// agent 任务未单独指定时的默认迭代上限。
    max_iterations: Option<usize>,
}

impl ExecutionRuntime {
    /// 用共享的 provider、工具注册表与数据库构造运行时。
    ///
    /// 默认不限制工具、不覆盖系统提示词；需要收紧策略时用 `with_*` 方法。
    pub fn new(
        providers: Arc<Mutex<ProviderRegistry>>,
        tools: Arc<Mutex<ToolRegistry>>,
        db: Arc<Mutex<Database>>,
    ) -> Self {
        Self {
            providers,
            tools,
            db,
            app_handle: None,
            allowed_tools: None,
            tools_enabled: true,
            system_prompt: None,
            max_iterations: None,
        }
    }

    /// 绑定 Tauri `AppHandle`，使执行日志可以推送到前端。
    pub fn with_app_handle(mut self, handle: tauri::AppHandle) -> Self {
        self.app_handle = Some(handle);
        self
    }

    /// 设置运行级工具策略。
    ///
    /// `allowed_tools = None` 表示不设白名单；`Some(vec![])` 表示一个工具都不允许。
    /// 当 `tools_enabled` 为 `false` 时，任何 `ToolCall` 步骤以及任何请求工具的
    /// `AgentTask` 都会在执行前被拒绝。
    pub fn with_tool_policy(
        mut self,
        allowed_tools: Option<Vec<String>>,
        tools_enabled: bool,
    ) -> Self {
        self.allowed_tools = allowed_tools;
        self.tools_enabled = tools_enabled;
        self
    }

    /// 覆盖 `AgentTask` 步骤使用的系统提示词。
    ///
    /// 缺省或为空时使用内置的 agent 任务提示词。
    pub fn with_system_prompt(mut self, system_prompt: Option<String>) -> Self {
        self.system_prompt = system_prompt;
        self
    }

    /// 为未单独指定迭代上限的 `AgentTask` 步骤提供默认值。
    pub fn with_max_iterations(mut self, max_iterations: Option<usize>) -> Self {
        self.max_iterations = max_iterations;
        self
    }

    /// 向前端推送执行日志（仅在绑定了 `app_handle` 时生效）。
    fn emit_log(&self, level: &str, step: &str, message: String) {
        if let Some(ref handle) = self.app_handle {
            let entry = ExecutionLogEntry::new(level, step, message);
            let _ = handle.emit("execution_log", entry);
        }
    }

    /// 执行一个 Plan，通过 event_tx 发射进度事件。
    ///
    /// 旧版入口：以 [`PlanRunMode::Autonomous`] 运行，只报告成功或
    /// [`ExecutionError`]。需要在失败时拿到部分结果的调用方应改用 [`Self::run`]。
    pub async fn execute(
        &self,
        plan: ExecutionPlan,
        event_tx: mpsc::Sender<PlanProgressEvent>,
        cancel_flag: Arc<AtomicBool>,
        pause_flag: Arc<AtomicBool>,
    ) -> Result<(), ExecutionError> {
        self.run(plan, event_tx, cancel_flag, pause_flag, PlanRunMode::Autonomous)
            .await
            .map(|_| ())
            .map_err(|failure| failure.to_execution_error())
    }

    /// 执行一个 Plan，并在失败时保留已经完成的部分进度。
    ///
    /// 需要展示「失败前已完成的步骤」的调用方应使用本方法。取消同样通过
    /// [`PlanFailure`] 返回，`message` 为旧版的 cancelled 文案，便于
    /// [`Self::execute`] 映射回 [`ExecutionError::Cancelled`]。
    pub async fn run(
        &self,
        plan: ExecutionPlan,
        event_tx: mpsc::Sender<PlanProgressEvent>,
        cancel_flag: Arc<AtomicBool>,
        pause_flag: Arc<AtomicBool>,
        mode: PlanRunMode,
    ) -> Result<PlanOutcome, PlanFailure> {
        let plan_id = plan.id.clone();
        let session_id = plan.session_id.clone();
        let total_steps = plan.steps.len();
        let writes_session_state = mode == PlanRunMode::Autonomous;

        self.log_run_start(&plan, total_steps);
        // 两种模式都持久化计划记录，作为历史留存。
        self.persist_plan(&plan).await;

        // 仅自主模式更新会话状态；聊天轮次不得改动会话状态。
        if writes_session_state {
            self.mark_session_running(&session_id, &plan_id).await;
        }

        let mut completed: Vec<CompletedStep> = Vec::new();

        for (i, step) in plan.steps.iter().enumerate() {
            let completed_steps = completed.len();

            if cancel_flag.load(Ordering::Relaxed) {
                return Err(
                    self.abort_cancelled(
                        &plan_id,
                        &session_id,
                        i,
                        &step.label,
                        total_steps,
                        &completed,
                        writes_session_state,
                        &event_tx,
                    )
                    .await,
                );
            }

            // 暂停期间自旋等待，每 200ms 检查一次取消标记。
            while pause_flag.load(Ordering::Relaxed) {
                if writes_session_state {
                    self.set_session_status(
                        &session_id,
                        ExecStatus::Paused {
                            step_index: i,
                            reason: "user_paused".to_string(),
                        },
                    )
                    .await;
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                if cancel_flag.load(Ordering::Relaxed) {
                    return Err(
                        self.abort_cancelled(
                            &plan_id,
                            &session_id,
                            i,
                            &step.label,
                            total_steps,
                            &completed,
                            writes_session_state,
                            &event_tx,
                        )
                        .await,
                    );
                }
            }

            Self::send_progress(
                &event_tx,
                PlanProgressEvent::new(
                    plan_id.as_str(),
                    session_id.as_str(),
                    "step_started",
                    total_steps,
                    completed_steps,
                )
                .with_step_index(i)
                .with_step_label(step.label.clone()),
            )
            .await;

            self.emit_log(
                "info",
                "execution",
                format!("步骤 {}/{} 开始: {}", i + 1, total_steps, step.label),
            );

            let step_start = Instant::now();
            let result = self
                .execute_step(&step.execution, &step.id, cancel_flag.clone(), event_tx.clone())
                .await;

            match result {
                Ok(val) => {
                    let duration = step_start.elapsed().as_millis() as u64;
                    self.emit_log(
                        "info",
                        "execution",
                        format!(
                            "步骤 {}/{} 完成 ({}ms): {}",
                            i + 1,
                            total_steps,
                            duration,
                            step.label
                        ),
                    );
                    self.checkpoint_step_completed(&plan_id, step, i, duration, &val)
                        .await;

                    completed.push(CompletedStep {
                        index: i,
                        id: step.id.clone(),
                        label: step.label.clone(),
                        result: val.clone(),
                    });

                    let mut event = PlanProgressEvent::new(
                        plan_id.as_str(),
                        session_id.as_str(),
                        "step_completed",
                        total_steps,
                        completed.len(),
                    )
                    .with_step_index(i)
                    .with_step_label(step.label.clone());
                    if let Some(summary) = result_summary(&val) {
                        event = event.with_result_summary(summary);
                    }
                    Self::send_progress(&event_tx, event).await;
                }
                Err(e) => {
                    let duration = step_start.elapsed().as_millis() as u64;
                    self.emit_log(
                        "error",
                        "execution",
                        format!(
                            "步骤 {}/{} 失败: {} — {}",
                            i + 1,
                            total_steps,
                            step.label,
                            e
                        ),
                    );
                    self.checkpoint_step_failed(&plan_id, step, i, duration, &e)
                        .await;

                    let completed_steps = completed.len();

                    Self::send_progress(
                        &event_tx,
                        PlanProgressEvent::new(
                            plan_id.as_str(),
                            session_id.as_str(),
                            "step_failed",
                            total_steps,
                            completed_steps,
                        )
                        .with_step_index(i)
                        .with_step_label(step.label.clone())
                        .with_error(e.to_string()),
                    )
                    .await;

                    self.set_plan_status(&plan_id, PlanStatus::Failed, Some(&Utc::now().to_rfc3339()))
                        .await;
                    if writes_session_state {
                        self.set_session_status(
                            &session_id,
                            ExecStatus::Failed {
                                step_index: i,
                                error: e.to_string(),
                            },
                        )
                        .await;
                    }

                    Self::send_progress(
                        &event_tx,
                        PlanProgressEvent::new(
                            plan_id.as_str(),
                            session_id.as_str(),
                            "plan_failed",
                            total_steps,
                            completed_steps,
                        )
                        .with_step_index(i)
                        .with_error(e.to_string()),
                    )
                    .await;

                    return Err(PlanFailure {
                        completed,
                        failed_step: step.label.clone(),
                        message: e.to_string(),
                    });
                }
            }
        }

        let completed_steps = completed.len();

        self.emit_log(
            "info",
            "execution",
            format!("执行计划完成: {}/{} 步成功", completed_steps, total_steps),
        );
        let finished_at = Utc::now().to_rfc3339();
        self.set_plan_status(&plan_id, PlanStatus::Completed, Some(&finished_at))
            .await;
        if writes_session_state {
            self.set_session_status(
                &session_id,
                ExecStatus::Completed {
                    finished_at: finished_at.clone(),
                },
            )
            .await;
        }

        Self::send_progress(
            &event_tx,
            PlanProgressEvent::new(
                plan_id.as_str(),
                session_id.as_str(),
                "plan_completed",
                total_steps,
                completed_steps,
            ),
        )
        .await;

        Ok(PlanOutcome {
            plan_id,
            session_id,
            completed,
        })
    }

    /// 记录运行开始日志，来源描述中的目标文本按字符截断。
    fn log_run_start(&self, plan: &ExecutionPlan, total_steps: usize) {
        let source_desc = match &plan.source {
            PlanSource::Dynamic { goal, .. } => {
                format!("dynamic: {}", truncate_chars(goal, GOAL_DESC_CHARS))
            }
            PlanSource::Static { workflow_name } => format!("static: {}", workflow_name),
        };
        self.emit_log(
            "info",
            "execution",
            format!("执行计划开始: {} 步, {}", total_steps, source_desc),
        );
    }

    /// 落库计划记录；失败只记日志，历史丢失不应阻断本次执行。
    async fn persist_plan(&self, plan: &ExecutionPlan) {
        let source = match serde_json::to_string(&plan.source) {
            Ok(json) => json,
            Err(e) => {
                log::error!(
                    "序列化计划来源失败，将以空字符串落库: plan_id={}, error={}",
                    plan.id,
                    e
                );
                String::new()
            }
        };
        let plan_json = match serde_json::to_string(plan) {
            Ok(json) => json,
            Err(e) => {
                log::error!(
                    "序列化计划失败，将以空字符串落库: plan_id={}, error={}",
                    plan.id,
                    e
                );
                String::new()
            }
        };

        let record = ExecutionPlanRecord {
            id: plan.id.clone(),
            session_id: plan.session_id.clone(),
            source,
            goal: match &plan.source {
                PlanSource::Dynamic { goal, .. } => Some(goal.clone()),
                _ => None,
            },
            plan_json,
            status: PlanStatus::Running.as_str().to_string(),
            created_at: plan.created_at.clone(),
            finished_at: None,
        };

        let db = self.db.lock().await;
        if let Err(e) = db.insert_execution_plan(&record) {
            log::error!(
                "持久化执行计划失败，计划历史将丢失: plan_id={}, error={}",
                plan.id,
                e
            );
        }
    }

    /// 把会话标记为「自主执行中」，仅自主模式调用。
    async fn mark_session_running(&self, session_id: &str, plan_id: &str) {
        let status_json = serialize_session_status(&ExecStatus::Running {
            step_index: 0,
            started_at: Utc::now().to_rfc3339(),
        });

        let db = self.db.lock().await;
        if let Err(e) = db.update_session_execution(
            session_id,
            SESSION_MODE_AUTONOMOUS,
            &status_json,
            Some(plan_id),
        ) {
            log::error!(
                "更新会话执行状态失败，会话状态可能与实际执行不同步: session_id={}, plan_id={}, error={}",
                session_id,
                plan_id,
                e
            );
        }
    }

    /// 处理取消：写状态、上报事件，并返回携带已完成步骤的失败结果。
    #[allow(clippy::too_many_arguments)]
    async fn abort_cancelled(
        &self,
        plan_id: &str,
        session_id: &str,
        step_index: usize,
        step_label: &str,
        total_steps: usize,
        completed: &[CompletedStep],
        writes_session_state: bool,
        event_tx: &mpsc::Sender<PlanProgressEvent>,
    ) -> PlanFailure {
        self.set_plan_status(plan_id, PlanStatus::Cancelled, None)
            .await;
        if writes_session_state {
            self.set_session_status(session_id, ExecStatus::Cancelled)
                .await;
        }

        Self::send_progress(
            event_tx,
            PlanProgressEvent::new(
                plan_id,
                session_id,
                "cancelled",
                total_steps,
                completed.len(),
            )
            .with_step_index(step_index),
        )
        .await;

        cancelled_failure(completed, step_label)
    }

    /// 写入成功步骤的 checkpoint；失败只记日志，避免影响执行结果。
    async fn checkpoint_step_completed(
        &self,
        plan_id: &str,
        step: &PlanStep,
        step_index: usize,
        duration_ms: u64,
        value: &Value,
    ) {
        let record = PlanStepRecord {
            id: step.id.clone(),
            plan_id: plan_id.to_string(),
            step_index: step_index as i32,
            label: step.label.clone(),
            step_type: exec_step_type_name(&step.execution).to_string(),
            status: StepStatus::Completed.as_str().to_string(),
            result_json: Some(value.to_string()),
            error: None,
            started_at: Some(Utc::now().to_rfc3339()),
            duration_ms: Some(duration_ms as i64),
        };

        let db = self.db.lock().await;
        if let Err(e) = db.upsert_plan_step(&record) {
            log::error!(
                "写入步骤 checkpoint 失败，断点续跑将丢失该步骤: plan_id={}, step_id={}, error={}",
                plan_id,
                step.id,
                e
            );
        }
    }

    /// 写入失败步骤的 checkpoint；失败只记日志，避免掩盖真正的执行错误。
    async fn checkpoint_step_failed(
        &self,
        plan_id: &str,
        step: &PlanStep,
        step_index: usize,
        duration_ms: u64,
        error: &ExecutionError,
    ) {
        let record = PlanStepRecord {
            id: step.id.clone(),
            plan_id: plan_id.to_string(),
            step_index: step_index as i32,
            label: step.label.clone(),
            step_type: exec_step_type_name(&step.execution).to_string(),
            status: StepStatus::Failed.as_str().to_string(),
            result_json: None,
            error: Some(error.to_string()),
            started_at: Some(Utc::now().to_rfc3339()),
            duration_ms: Some(duration_ms as i64),
        };

        let db = self.db.lock().await;
        if let Err(e) = db.upsert_plan_step(&record) {
            log::error!(
                "写入失败步骤 checkpoint 失败，断点续跑将丢失该步骤: plan_id={}, step_id={}, error={}",
                plan_id,
                step.id,
                e
            );
        }
    }

    /// 发送进度事件；接收端已关闭时静默忽略。
    async fn send_progress(
        event_tx: &mpsc::Sender<PlanProgressEvent>,
        event: PlanProgressEvent,
    ) {
        let _ = event_tx.send(event).await;
    }

    /// 拒绝当前运行策略不允许的 `ToolCall` 步骤。
    pub(super) fn validate_tool_call(&self, tool: &str) -> Result<(), ExecutionError> {
        if !self.tools_enabled {
            return Err(ExecutionError::StepFailed {
                step: 0,
                message: format!(
                    "Tools are disabled for this run; refusing to execute tool '{}'",
                    tool
                ),
            });
        }

        if let Some(allowed) = &self.allowed_tools {
            if !allowed.iter().any(|name| name == tool) {
                return Err(ExecutionError::StepFailed {
                    step: 0,
                    message: format!(
                        "Tool '{}' is not in the allowed tool list for this run",
                        tool
                    ),
                });
            }
        }

        Ok(())
    }

    /// 当工具被全局禁用时，拒绝请求工具的 `AgentTask`。
    pub(super) fn validate_agent_task(
        &self,
        allowed_tools: &Option<Vec<String>>,
    ) -> Result<(), ExecutionError> {
        if self.tools_enabled {
            return Ok(());
        }

        let requests_tools = allowed_tools
            .as_ref()
            .map(|tools| !tools.is_empty())
            .unwrap_or(false);

        if requests_tools {
            return Err(ExecutionError::StepFailed {
                step: 0,
                message: "Tools are disabled for this run; refusing to execute an agent task that requests tools"
                    .to_string(),
            });
        }

        Ok(())
    }

    /// 计算 agent 任务实际生效的工具白名单：步骤级配置优先于运行级默认值。
    pub(super) fn effective_allowed_tools(
        &self,
        step_allowed: &Option<Vec<String>>,
    ) -> Option<Vec<String>> {
        step_allowed.clone().or_else(|| self.allowed_tools.clone())
    }

    /// 计算 agent 任务实际生效的迭代上限：步骤级配置优先于运行级默认值。
    pub(super) fn effective_max_iterations(&self, step_max: Option<usize>) -> Option<usize> {
        step_max.or(self.max_iterations)
    }

    /// 更新计划状态；失败只记日志。
    async fn set_plan_status(&self, plan_id: &str, status: PlanStatus, finished_at: Option<&str>) {
        let db = self.db.lock().await;
        if let Err(e) = db.update_execution_plan_status(plan_id, status.as_str(), finished_at) {
            log::error!(
                "更新计划状态失败，计划状态可能与实际执行不同步: plan_id={}, status={}, error={}",
                plan_id,
                status.as_str(),
                e
            );
        }
    }

    /// 更新会话执行状态；失败只记日志。
    async fn set_session_status(&self, session_id: &str, status: ExecStatus) {
        let status_json = serialize_session_status(&status);
        let db = self.db.lock().await;
        if let Err(e) = db.update_session_execution_status(session_id, &status_json) {
            log::error!(
                "更新会话执行状态失败，会话状态可能与实际执行不同步: session_id={}, error={}",
                session_id,
                e
            );
        }
    }
}

/// 序列化会话执行状态；失败时返回空字符串并记录日志（保持原有落库行为）。
fn serialize_session_status(status: &ExecStatus) -> String {
    match serde_json::to_string(status) {
        Ok(json) => json,
        Err(e) => {
            log::error!("序列化会话执行状态失败，将以空字符串落库: error={}", e);
            String::new()
        }
    }
}

/// 从步骤结果中提取给前端展示的摘要，按字符截断以免切断中文。
fn result_summary(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(|s| truncate_chars(s, RESULT_SUMMARY_CHARS).to_string())
}

/// 构造「已取消」的失败结果，`message` 沿用旧版 cancelled 文案。
fn cancelled_failure(completed: &[CompletedStep], failed_step: &str) -> PlanFailure {
    PlanFailure {
        completed: completed.to_vec(),
        failed_step: failed_step.to_string(),
        message: ExecutionError::Cancelled.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::db::models::Session;
    use std::collections::HashMap;
    use uuid::Uuid;

    fn test_runtime() -> (ExecutionRuntime, Arc<Mutex<Database>>) {
        let providers = Arc::new(Mutex::new(ProviderRegistry::new(&[])));
        let tools = Arc::new(Mutex::new(ToolRegistry::new()));
        let db = Arc::new(Mutex::new(Database::new_test().expect("in-memory db")));
        let runtime = ExecutionRuntime::new(providers, tools, db.clone());
        (runtime, db)
    }

    async fn insert_session(db: &Arc<Mutex<Database>>, id: &str) {
        let now = Utc::now().to_rfc3339();
        let db = db.lock().await;
        db.create_session(&Session {
            id: id.to_string(),
            title: "test session".to_string(),
            model_id: "test-model".to_string(),
            system_prompt: None,
            persona_id: None,
            config: None,
            title_source: "manual".to_string(),
            archived: false,
            created_at: now.clone(),
            updated_at: now,
            mode: "chat".to_string(),
            execution_status: String::new(),
            active_plan_id: None,
            project_id: None,
            intent_domain_id: None,
            intent_domain_mode: "auto".to_string(),
        })
        .expect("create session");
    }

    fn build_plan(session_id: &str, steps: Vec<ExecStep>) -> ExecutionPlan {
        ExecutionPlan {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            source: PlanSource::Dynamic {
                goal: "test goal".to_string(),
                generated_by: "test".to_string(),
            },
            steps: steps
                .into_iter()
                .enumerate()
                .map(|(i, execution)| PlanStep {
                    id: format!("step-{}", i + 1),
                    label: format!("Step {}", i + 1),
                    execution,
                    status: StepStatus::Pending,
                    result: None,
                    error: None,
                    started_at: None,
                    duration_ms: None,
                })
                .collect(),
            status: PlanStatus::Pending,
            created_at: Utc::now().to_rfc3339(),
            finished_at: None,
        }
    }

    fn condition_step() -> ExecStep {
        ExecStep::Condition {
            expression: "true".to_string(),
            on_true: BranchTarget::Continue,
            on_false: BranchTarget::End,
        }
    }

    fn tool_step(tool: &str) -> ExecStep {
        ExecStep::ToolCall {
            tool: tool.to_string(),
            params: HashMap::new(),
            retry: None,
            timeout_seconds: None,
        }
    }

    fn flags() -> (Arc<AtomicBool>, Arc<AtomicBool>) {
        (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)))
    }

    #[tokio::test]
    async fn interactive_leaves_session_state_untouched_autonomous_writes_it() {
        let (runtime, db) = test_runtime();
        insert_session(&db, "s1").await;

        let interactive_plan = build_plan("s1", vec![condition_step()]);
        let interactive_plan_id = interactive_plan.id.clone();
        let (tx, _rx) = mpsc::channel(32);
        let (cf, pf) = flags();
        runtime
            .run(
                interactive_plan,
                tx,
                cf,
                pf,
                PlanRunMode::Interactive,
            )
            .await
            .expect("interactive run should succeed");

        {
            let db = db.lock().await;
            let session = db.get_session("s1").unwrap().expect("session");
            assert_eq!(session.mode, "chat");
            assert_eq!(session.execution_status, "");
            assert_eq!(session.active_plan_id, None);
            // 交互模式下计划记录仍会被持久化。
            assert!(db
                .get_execution_plan(&interactive_plan_id)
                .unwrap()
                .is_some());
        }

        let autonomous_plan = build_plan("s1", vec![condition_step()]);
        let autonomous_plan_id = autonomous_plan.id.clone();
        let (tx, _rx) = mpsc::channel(32);
        let (cf, pf) = flags();
        runtime
            .run(
                autonomous_plan,
                tx,
                cf,
                pf,
                PlanRunMode::Autonomous,
            )
            .await
            .expect("autonomous run should succeed");

        let db = db.lock().await;
        let session = db.get_session("s1").unwrap().expect("session");
        assert_eq!(session.mode, "autonomous");
        assert_eq!(
            session.active_plan_id.as_deref(),
            Some(autonomous_plan_id.as_str())
        );
        assert!(session.execution_status.contains("completed"));
    }

    #[tokio::test]
    async fn tools_disabled_rejects_tool_steps() {
        let (runtime, db) = test_runtime();
        let runtime = runtime.with_tool_policy(None, false);
        insert_session(&db, "s1").await;

        let plan = build_plan("s1", vec![tool_step("calculator")]);
        let (tx, _rx) = mpsc::channel(32);
        let (cf, pf) = flags();
        let failure = runtime
            .run(plan, tx, cf, pf, PlanRunMode::Interactive)
            .await
            .expect_err("tool step should be rejected");

        assert!(failure.completed.is_empty());
        assert!(
            failure.message.contains("disabled"),
            "unexpected message: {}",
            failure.message
        );
    }

    #[tokio::test]
    async fn tool_outside_whitelist_is_rejected() {
        let (runtime, db) = test_runtime();
        let runtime =
            runtime.with_tool_policy(Some(vec!["calculator".to_string()]), true);
        insert_session(&db, "s1").await;

        let plan = build_plan("s1", vec![tool_step("web_search")]);
        let (tx, _rx) = mpsc::channel(32);
        let (cf, pf) = flags();
        let failure = runtime
            .run(plan, tx, cf, pf, PlanRunMode::Interactive)
            .await
            .expect_err("out-of-whitelist tool should be rejected");

        assert!(
            failure.message.contains("not in the allowed tool list"),
            "unexpected message: {}",
            failure.message
        );
    }

    #[tokio::test]
    async fn system_prompt_override_reaches_agent_task() {
        let (runtime, _db) = test_runtime();
        let custom = runtime.with_system_prompt(Some("CUSTOM AGENT PROMPT".to_string()));
        let messages = custom.agent_task_messages("do the thing");

        let system = messages.iter().find_map(|m| match m {
            Message::System { content } => Some(content.clone()),
            _ => None,
        });
        assert_eq!(system.as_deref(), Some("CUSTOM AGENT PROMPT"));

        let (default_runtime, _db) = test_runtime();
        let messages = default_runtime.agent_task_messages("do the thing");
        let system = messages.iter().find_map(|m| match m {
            Message::System { content } => Some(content.clone()),
            _ => None,
        });
        assert!(system
            .as_deref()
            .unwrap_or_default()
            .starts_with("You are an autonomous agent"));
    }

    #[test]
    fn policy_defaults_apply_when_step_omits_them() {
        let (runtime, _db) = test_runtime();
        let runtime = runtime
            .with_tool_policy(Some(vec!["calculator".to_string()]), true)
            .with_max_iterations(Some(3));

        assert_eq!(
            runtime.effective_allowed_tools(&None),
            Some(vec!["calculator".to_string()])
        );
        assert_eq!(runtime.effective_max_iterations(None), Some(3));
        // 步骤级配置优先于运行级默认值。
        assert_eq!(
            runtime.effective_allowed_tools(&Some(vec!["web_search".to_string()])),
            Some(vec!["web_search".to_string()])
        );
        assert_eq!(runtime.effective_max_iterations(Some(7)), Some(7));
    }

    #[tokio::test]
    async fn second_step_failure_returns_partial_results() {
        let (runtime, db) = test_runtime();
        insert_session(&db, "s1").await;

        let plan = build_plan(
            "s1",
            vec![
                condition_step(),
                tool_step("does_not_exist"),
                condition_step(),
            ],
        );
        let (tx, _rx) = mpsc::channel(32);
        let (cf, pf) = flags();
        let failure = runtime
            .run(plan, tx, cf, pf, PlanRunMode::Interactive)
            .await
            .expect_err("second step should fail");

        assert_eq!(failure.completed.len(), 1);
        assert_eq!(failure.completed[0].label, "Step 1");
        assert_eq!(failure.failed_step, "Step 2");
        assert!(
            failure.message.contains("does_not_exist"),
            "unexpected message: {}",
            failure.message
        );
        assert!(failure.combined_result().is_some());
    }

    #[tokio::test]
    async fn execute_maps_failure_to_execution_error() {
        let (runtime, db) = test_runtime();
        insert_session(&db, "s1").await;

        let plan = build_plan("s1", vec![tool_step("does_not_exist")]);
        let (tx, _rx) = mpsc::channel(32);
        let (cf, pf) = flags();
        let error = runtime
            .execute(plan, tx, cf, pf)
            .await
            .expect_err("step should fail");

        assert!(matches!(error, ExecutionError::StepFailed { .. }));
    }

    #[test]
    fn status_as_str_matches_serde_values() {
        // 落库字符串必须与序列化取值一致，否则历史数据无法反序列化。
        for status in [
            PlanStatus::Pending,
            PlanStatus::Running,
            PlanStatus::Completed,
            PlanStatus::Failed,
            PlanStatus::Cancelled,
        ] {
            let serialized = serde_json::to_string(&status).expect("serialize plan status");
            assert_eq!(serialized, format!("\"{}\"", status.as_str()));
        }

        for status in [
            StepStatus::Pending,
            StepStatus::Running,
            StepStatus::Completed,
            StepStatus::Failed,
            StepStatus::Skipped,
        ] {
            let serialized = serde_json::to_string(&status).expect("serialize step status");
            assert_eq!(serialized, format!("\"{}\"", status.as_str()));
        }
    }
    // ── task 4：deep 路径的工具执行 ─────────────────────────────────────

    /// 一个会睡很久的工具，用来验证步骤级超时。
    #[derive(Debug, Clone)]
    struct SlowTool;

    #[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
    struct SlowArgs {}

    impl rig::tool::Tool for SlowTool {
        const NAME: &'static str = "slow";
        type Args = SlowArgs;
        type Output = String;
        type Error = rig::tool::ToolExecutionError;

        fn description(&self) -> String {
            "Sleeps forever".to_string()
        }

        fn parameters(&self) -> serde_json::Value {
            rig::__core::tool::args_schema::<SlowArgs>()
        }

        async fn call(
            &self,
            _context: &mut rig::tool::ToolContext,
            _args: Self::Args,
        ) -> Result<Self::Output, Self::Error> {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            Ok("done".to_string())
        }
    }

    fn runtime_with_slow_tool() -> ExecutionRuntime {
        let providers = Arc::new(Mutex::new(ProviderRegistry::new(&[])));
        let mut registry = ToolRegistry::new();
        registry.register(
            "slow",
            crate::agent::tools::convert::into_dynamic(SlowTool),
            true,
        );
        let tools = Arc::new(Mutex::new(registry));
        let db = Arc::new(Mutex::new(Database::new_test().expect("in-memory db")));
        ExecutionRuntime::new(providers, tools, db)
    }

    #[tokio::test]
    async fn execute_tool_uses_registry_and_returns_output() {
        let (runtime, _db) = test_runtime();
        let dir = tempfile::tempdir().expect("临时目录");
        let path = dir.path().join("probe.txt");
        std::fs::write(&path, "hello from rig-tools").expect("写探针文件");

        let params = HashMap::from([(
            "path".to_string(),
            serde_json::json!(path.to_string_lossy()),
        )]);
        let (cf, _pf) = flags();

        let value = runtime
            .execute_tool("read", &params, &None, None, cf)
            .await
            .expect("read 应经注册表执行成功");

        assert!(
            value.as_str().unwrap_or_default().contains("hello from rig-tools"),
            "read 应返回文件内容: {:?}",
            value
        );
    }

    #[tokio::test]
    async fn execute_tool_fails_when_timeout_expires() {
        let runtime = runtime_with_slow_tool();
        let (cf, _pf) = flags();

        let error = runtime
            .execute_tool("slow", &HashMap::new(), &None, Some(1), cf)
            .await
            .expect_err("超时应让步骤失败");

        assert!(
            error.to_string().contains("timed out"),
            "错误应说明超时: {}",
            error
        );
    }

    #[tokio::test]
    async fn agent_task_without_whitelist_still_gets_tools() {
        let (runtime, _db) = test_runtime();

        // None 是「不限制」，运行级开关才是唯一的总闸。
        assert!(runtime.tools_enabled);
        assert_eq!(runtime.effective_allowed_tools(&None), None);
    }
}

