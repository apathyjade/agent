use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use chrono::Utc;
use serde_json::Value;
use tokio::sync::{Mutex, mpsc};

use crate::api::provider::ProviderRegistry;
use crate::db::repository::Database;
use crate::execution::error::ExecutionError;
use crate::execution::types::*;
use crate::tools::registry::ToolRegistry;

pub struct ExecutionRuntime {
    providers: Arc<Mutex<ProviderRegistry>>,
    tools: Arc<Mutex<ToolRegistry>>,
    db: Arc<Mutex<Database>>,
}

impl ExecutionRuntime {
    pub fn new(
        providers: Arc<Mutex<ProviderRegistry>>,
        tools: Arc<Mutex<ToolRegistry>>,
        db: Arc<Mutex<Database>>,
    ) -> Self {
        Self { providers, tools, db }
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

            // 执行步骤
            let step_start = Instant::now();
            let result = self.execute_step(&step.execution, &step.id, cancel_flag.clone()).await;

            match result {
                Ok(val) => {
                    let duration = step_start.elapsed().as_millis() as u64;
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

    /// 执行单个 ExecStep
    async fn execute_step(
        &self,
        step: &ExecStep,
        _step_id: &str,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<Value, ExecutionError> {
        match step {
            ExecStep::ToolCall {
                tool,
                params,
                retry,
                timeout_seconds,
            } => {
                self.execute_tool(tool, params, retry, *timeout_seconds, cancel_flag)
                    .await
            }
            ExecStep::LlmCall {
                prompt,
                system_prompt,
                model_id,
                temperature,
                max_tokens,
            } => {
                self.execute_llm(prompt, system_prompt, model_id, *temperature, *max_tokens)
                    .await
            }
            ExecStep::AgentTask {
                instruction,
                model_id,
                max_iterations,
                allowed_tools,
                temperature,
            } => {
                // AgentTask 在 Phase 2 中完整实现，Phase 1 返回占位
                self.execute_agent_task(
                    instruction,
                    model_id,
                    *max_iterations,
                    allowed_tools,
                    *temperature,
                    cancel_flag,
                )
                .await
            }
            ExecStep::Condition {
                expression,
                on_true,
                on_false,
            } => self.execute_condition(expression, on_true, on_false).await,
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

        let map: serde_json::Map<String, Value> = params.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let input = Value::Object(map);

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
                        tokio::time::sleep(tokio::time::Duration::from_secs(delay)).await;
                    }
                    continue;
                }
                Err(e) => {
                    return Err(ExecutionError::StepFailed {
                        step: 0,
                        message: format!(
                            "Tool '{}' failed after {} retries: {}",
                            tool,
                            max_retries,
                            e
                        ),
                    });
                }
            }
        }
        Err(ExecutionError::MaxRetries { step: 0 })
    }

    async fn execute_llm(
        &self,
        _prompt: &str,
        _system_prompt: &Option<String>,
        _model_id: &Option<String>,
        _temperature: Option<f32>,
        _max_tokens: Option<u32>,
    ) -> Result<Value, ExecutionError> {
        // Phase 2: 实际 LLM 调用
        Err(ExecutionError::Internal(
            "LLM call not yet implemented in Phase 1".to_string(),
        ))
    }

    async fn execute_agent_task(
        &self,
        _instruction: &str,
        _model_id: &Option<String>,
        _max_iterations: Option<usize>,
        _allowed_tools: &Option<Vec<String>>,
        _temperature: Option<f32>,
        _cancel_flag: Arc<AtomicBool>,
    ) -> Result<Value, ExecutionError> {
        // Phase 2: 完整 Agent 循环
        Err(ExecutionError::Internal(
            "Agent task not yet implemented in Phase 1".to_string(),
        ))
    }

    async fn execute_condition(
        &self,
        expression: &str,
        _on_true: &BranchTarget,
        _on_false: &BranchTarget,
    ) -> Result<Value, ExecutionError> {
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
        let _ = db.update_session_execution_status(&session_id, &status_json);
    }
}
