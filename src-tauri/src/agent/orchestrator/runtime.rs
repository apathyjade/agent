use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use chrono::Utc;
use serde_json::Value;
use tokio::sync::{Mutex, mpsc};

use crate::agent::fast_loop::ToolLoop;
use crate::llm::provider::ProviderRegistry;
use rig::completion::Message;
use crate::core::db::repository::Database;
use crate::agent::orchestrator::plan_error::ExecutionError;
use crate::agent::orchestrator::plan_types::*;
use crate::agent::tools::registry::ToolRegistry;
use tauri::Emitter;

mod executor;

pub struct ExecutionRuntime {
    providers: Arc<Mutex<ProviderRegistry>>,
    tools: Arc<Mutex<ToolRegistry>>,
    db: Arc<Mutex<Database>>,
    app_handle: Option<tauri::AppHandle>,
    /// Run-level tool whitelist. `None` means unrestricted.
    allowed_tools: Option<Vec<String>>,
    /// Run-level master switch for tool execution.
    tools_enabled: bool,
    /// Optional system prompt for agent tasks (falls back to the built-in text).
    system_prompt: Option<String>,
    /// Optional default iteration cap applied to agent tasks without their own.
    max_iterations: Option<usize>,
}

impl ExecutionRuntime {
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

    pub fn with_app_handle(mut self, handle: tauri::AppHandle) -> Self {
        self.app_handle = Some(handle);
        self
    }

    /// Apply the run-level tool policy.
    ///
    /// `allowed_tools = None` means no whitelist; `Some(vec![])` means no tool
    /// is allowed. When `tools_enabled` is `false`, any `ToolCall` step and any
    /// `AgentTask` that requests tools are rejected before execution.
    pub fn with_tool_policy(
        mut self,
        allowed_tools: Option<Vec<String>>,
        tools_enabled: bool,
    ) -> Self {
        self.allowed_tools = allowed_tools;
        self.tools_enabled = tools_enabled;
        self
    }

    /// Override the system prompt used by `AgentTask` steps.
    ///
    /// When absent (or empty), the built-in agent-task prompt is used.
    pub fn with_system_prompt(mut self, system_prompt: Option<String>) -> Self {
        self.system_prompt = system_prompt;
        self
    }

    /// Default iteration cap for `AgentTask` steps that do not set one.
    pub fn with_max_iterations(mut self, max_iterations: Option<usize>) -> Self {
        self.max_iterations = max_iterations;
        self
    }

    /// Emit execution log event to frontend (if app_handle is set)
    fn emit_log(&self, level: &str, step: &str, message: String) {
        if let Some(ref handle) = self.app_handle {
            let entry = ExecutionLogEntry::new(level, step, message);
            let _ = handle.emit("execution_log", entry);
        }
    }

    /// 执行一个 Plan，通过 event_tx 发射进度事件
    ///
    /// Legacy entry point: runs in [`PlanRunMode::Autonomous`] and only reports
    /// success or an [`ExecutionError`]. New callers that need partial results
    /// after an in-flight failure should use [`Self::run`].
    pub async fn execute(
        &self,
        plan: ExecutionPlan,
        event_tx: mpsc::Sender<PlanProgressEvent>,
        cancel_flag: Arc<AtomicBool>,
        pause_flag: Arc<AtomicBool>,
    ) -> Result<(), ExecutionError> {
        self.execute_with_mode(
            plan,
            event_tx,
            cancel_flag,
            pause_flag,
            PlanRunMode::Autonomous,
        )
        .await
        .map(|_| ())
    }

    /// Execute a plan under the given run mode.
    ///
    /// Failures are mapped back to [`ExecutionError`] so existing callers keep
    /// compiling; use [`Self::run`] to receive completed step results alongside
    /// the failure.
    pub async fn execute_with_mode(
        &self,
        plan: ExecutionPlan,
        event_tx: mpsc::Sender<PlanProgressEvent>,
        cancel_flag: Arc<AtomicBool>,
        pause_flag: Arc<AtomicBool>,
        mode: PlanRunMode,
    ) -> Result<PlanOutcome, ExecutionError> {
        self.run(plan, event_tx, cancel_flag, pause_flag, mode)
            .await
            .map_err(|failure| failure.to_execution_error())
    }

    /// Execute a plan and preserve partial progress on failure.
    ///
    /// This is the preferred entry point for callers that need to surface the
    /// results of steps that completed before a later step failed. Cancellation
    /// still reports through [`PlanFailure`] with the legacy "cancelled"
    /// message, which [`Self::execute_with_mode`] maps back to
    /// [`ExecutionError::Cancelled`].
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

        let source_desc = match &plan.source {
            PlanSource::Dynamic { goal, .. } => format!("dynamic: {}", goal.chars().take(50).collect::<String>()),
            PlanSource::Static { workflow_name } => format!("static: {}", workflow_name),
        };
        self.emit_log("info", "execution", format!("执行计划开始: {} 步, {}", total_steps, source_desc));

        // 持久化 Plan 记录（两种模式都保留，作为历史）
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

        // 更新 session 状态（仅自主模式；聊天轮次不得改动会话状态）
        if writes_session_state {
            let db = self.db.lock().await;
            let status_json = serde_json::to_string(&ExecStatus::Running {
                step_index: 0,
                started_at: Utc::now().to_rfc3339(),
            })
            .unwrap_or_default();
            let _ = db.update_session_execution(
                &session_id,
                "autonomous",
                &status_json,
                Some(&plan_id),
            );
        }

        let mut completed: Vec<CompletedStep> = Vec::new();

        for (i, step) in plan.steps.iter().enumerate() {
            let completed_steps = completed.len();

            // 检查是否被取消
            if cancel_flag.load(Ordering::Relaxed) {
                self.set_plan_status(&plan_id, "cancelled", None).await;
                if writes_session_state {
                    self.set_session_status(&session_id, ExecStatus::Cancelled).await;
                }
                let _ = event_tx
                    .send(PlanProgressEvent {
                        plan_id: plan_id.clone(),
                        session_id: session_id.clone(),
                        event_type: "cancelled".to_string(),
                        step_index: Some(i),
                        step_label: None,
                        result_summary: None,
                        error: None,
                        total_steps,
                        completed_steps,
                    })
                    .await;
                return Err(cancelled_failure(&completed, &step.label));
            }

            // 检查是否暂停
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
                    self.set_plan_status(&plan_id, "cancelled", None).await;
                    if writes_session_state {
                        self.set_session_status(&session_id, ExecStatus::Cancelled).await;
                    }
                    return Err(cancelled_failure(&completed, &step.label));
                }
            }

            // 发射 step_started 事件
            let _ = event_tx
                .send(PlanProgressEvent {
                    plan_id: plan_id.clone(),
                    session_id: session_id.clone(),
                    event_type: "step_started".to_string(),
                    step_index: Some(i),
                    step_label: Some(step.label.clone()),
                    result_summary: None,
                    error: None,
                    total_steps,
                    completed_steps,
                })
                .await;

            self.emit_log("info", "execution", format!("步骤 {}/{} 开始: {}", i + 1, total_steps, step.label));

            // 执行步骤
            let step_start = Instant::now();
            let result = self.execute_step(&step.execution, &step.id, cancel_flag.clone(), event_tx.clone()).await;

            match result {
                Ok(val) => {
                    let duration = step_start.elapsed().as_millis() as u64;
                    self.emit_log("info", "execution", format!("步骤 {}/{} 完成 ({}ms): {}", i + 1, total_steps, duration, step.label));
                    // 保存 checkpoint
                    {
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
                    }

                    completed.push(CompletedStep {
                        index: i,
                        id: step.id.clone(),
                        label: step.label.clone(),
                        result: val.clone(),
                    });
                    let completed_steps = completed.len();

                    let _ = event_tx
                        .send(PlanProgressEvent {
                            plan_id: plan_id.clone(),
                            session_id: session_id.clone(),
                            event_type: "step_completed".to_string(),
                            step_index: Some(i),
                            step_label: Some(step.label.clone()),
                            result_summary: val
                                .as_str()
                                .map(|s| s[..s.len().min(100)].to_string()),
                            error: None,
                            total_steps,
                            completed_steps,
                        })
                        .await;
                }
                Err(e) => {
                    let duration = step_start.elapsed().as_millis() as u64;
                    self.emit_log("error", "execution", format!("步骤 {}/{} 失败: {} — {}", i + 1, total_steps, step.label, e));
                    // 保存失败 checkpoint
                    {
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
                    }

                    let completed_steps = completed.len();

                    let _ = event_tx
                        .send(PlanProgressEvent {
                            plan_id: plan_id.clone(),
                            session_id: session_id.clone(),
                            event_type: "step_failed".to_string(),
                            step_index: Some(i),
                            step_label: Some(step.label.clone()),
                            result_summary: None,
                            error: Some(e.to_string()),
                            total_steps,
                            completed_steps,
                        })
                        .await;

                    self.set_plan_status(&plan_id, "failed", Some(&Utc::now().to_rfc3339()))
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

                    let _ = event_tx
                        .send(PlanProgressEvent {
                            plan_id: plan_id.clone(),
                            session_id: session_id.clone(),
                            event_type: "plan_failed".to_string(),
                            step_index: Some(i),
                            step_label: None,
                            result_summary: None,
                            error: Some(e.to_string()),
                            total_steps,
                            completed_steps,
                        })
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

        // Plan 完成
        self.emit_log("info", "execution", format!("执行计划完成: {}/{} 步成功", completed_steps, total_steps));
        let finished_at = Utc::now().to_rfc3339();
        self.set_plan_status(&plan_id, "completed", Some(&finished_at))
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

        let _ = event_tx
            .send(PlanProgressEvent {
                plan_id: plan_id.clone(),
                session_id: session_id.clone(),
                event_type: "plan_completed".to_string(),
                step_index: None,
                step_label: None,
                result_summary: None,
                error: None,
                total_steps,
                completed_steps,
            })
            .await;

        Ok(PlanOutcome {
            plan_id,
            session_id,
            completed,
        })
    }

    /// Reject a `ToolCall` step that the current run policy does not permit.
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

    /// Reject an `AgentTask` that would enable tools while tools are disabled.
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

    /// Resolve the effective tool whitelist for an agent task.
    pub(super) fn effective_allowed_tools(
        &self,
        step_allowed: &Option<Vec<String>>,
    ) -> Option<Vec<String>> {
        step_allowed.clone().or_else(|| self.allowed_tools.clone())
    }

    /// Resolve the effective iteration cap for an agent task.
    pub(super) fn effective_max_iterations(&self, step_max: Option<usize>) -> Option<usize> {
        step_max.or(self.max_iterations)
    }

    async fn set_plan_status(&self, plan_id: &str, status: &str, finished_at: Option<&str>) {
        let db = self.db.lock().await;
        let _ = db.update_execution_plan_status(plan_id, status, finished_at);
    }

    async fn set_session_status(&self, session_id: &str, status: ExecStatus) {
        let db = self.db.lock().await;
        let status_json = serde_json::to_string(&status).unwrap_or_default();
        let _ = db.update_session_execution_status(&session_id, &status_json);
    }
}

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
            // Plan rows are still persisted in interactive mode.
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
        // Step-level values win over run-level defaults.
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
    async fn execute_with_mode_maps_failure_to_execution_error() {
        let (runtime, db) = test_runtime();
        insert_session(&db, "s1").await;

        let plan = build_plan("s1", vec![tool_step("does_not_exist")]);
        let (tx, _rx) = mpsc::channel(32);
        let (cf, pf) = flags();
        let error = runtime
            .execute_with_mode(plan, tx, cf, pf, PlanRunMode::Interactive)
            .await
            .expect_err("step should fail");

        assert!(matches!(error, ExecutionError::StepFailed { .. }));
    }
}

