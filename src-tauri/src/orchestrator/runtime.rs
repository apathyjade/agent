use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use chrono::Utc;
use serde_json::Value;
use tokio::sync::{Mutex, mpsc};

use crate::agent::r#loop::ToolLoop;
use crate::api::provider::ProviderRegistry;
use rig::completion::Message;
use crate::db::repository::Database;
use crate::orchestrator::plan_error::ExecutionError;
use crate::orchestrator::plan_types::*;
use crate::tools::registry::ToolRegistry;
use tauri::Emitter;

mod executor;

pub struct ExecutionRuntime {
    providers: Arc<Mutex<ProviderRegistry>>,
    tools: Arc<Mutex<ToolRegistry>>,
    db: Arc<Mutex<Database>>,
    app_handle: Option<tauri::AppHandle>,
}

impl ExecutionRuntime {
    pub fn new(
        providers: Arc<Mutex<ProviderRegistry>>,
        tools: Arc<Mutex<ToolRegistry>>,
        db: Arc<Mutex<Database>>,
    ) -> Self {
        Self { providers, tools, db, app_handle: None }
    }

    pub fn with_app_handle(mut self, handle: tauri::AppHandle) -> Self {
        self.app_handle = Some(handle);
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

        let source_desc = match &plan.source {
            PlanSource::Dynamic { goal, .. } => format!("dynamic: {}", goal.chars().take(50).collect::<String>()),
            PlanSource::Static { workflow_name } => format!("static: {}", workflow_name),
        };
        self.emit_log("info", "execution", format!("执行计划开始: {} 步, {}", total_steps, source_desc));

        // 持久化 Plan 记录
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
            let _ = db.update_session_execution(
                &session_id,
                "autonomous",
                &status_json,
                Some(&plan_id),
            );
        }

        let mut completed_steps: usize = 0;

        for (i, step) in plan.steps.iter().enumerate() {
            // 检查是否被取消
            if cancel_flag.load(Ordering::Relaxed) {
                self.set_plan_status(&plan_id, "cancelled", None).await;
                self.set_session_status(&session_id, ExecStatus::Cancelled).await;
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
                return Err(ExecutionError::Cancelled);
            }

            // 检查是否暂停
            while pause_flag.load(Ordering::Relaxed) {
                self.set_session_status(
                    &session_id,
                    ExecStatus::Paused {
                        step_index: i,
                        reason: "user_paused".to_string(),
                    },
                )
                .await;
                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                if cancel_flag.load(Ordering::Relaxed) {
                    self.set_plan_status(&plan_id, "cancelled", None).await;
                    self.set_session_status(&session_id, ExecStatus::Cancelled).await;
                    return Err(ExecutionError::Cancelled);
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

                    completed_steps += 1;

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
                    self.set_session_status(
                        &session_id,
                        ExecStatus::Failed {
                            step_index: i,
                            error: e.to_string(),
                        },
                    )
                    .await;

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

                    return Err(e);
                }
            }
        }

        // Plan 完成
        self.emit_log("info", "execution", format!("执行计划完成: {}/{} 步成功", completed_steps, total_steps));
        let finished_at = Utc::now().to_rfc3339();
        self.set_plan_status(&plan_id, "completed", Some(&finished_at))
            .await;
        self.set_session_status(
            &session_id,
            ExecStatus::Completed {
                finished_at: finished_at.clone(),
            },
        )
        .await;

        let _ = event_tx
            .send(PlanProgressEvent {
                plan_id,
                session_id,
                event_type: "plan_completed".to_string(),
                step_index: None,
                step_label: None,
                result_summary: None,
                error: None,
                total_steps,
                completed_steps,
            })
            .await;

        Ok(())
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
