use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::Mutex;

use crate::api::provider::ProviderRegistry;
use crate::api::types::{ChatRequest, Message, MessageRole};
use crate::error::{AppError, Result};
use crate::pipeline::models::{
    StepDef, StepProgress, TriggerDef, WorkflowDef, WorkflowRunRecord,
};
use crate::tools::registry::ToolRegistry;

pub struct PipelineEngine {
    tools: Arc<Mutex<ToolRegistry>>,
    db: Arc<Mutex<crate::db::repository::Database>>,
    providers: Arc<Mutex<ProviderRegistry>>,
    event_tx: tokio::sync::mpsc::Sender<StepProgress>,
}

impl PipelineEngine {
    pub fn new(
        tools: Arc<Mutex<ToolRegistry>>,
        db: Arc<Mutex<crate::db::repository::Database>>,
        providers: Arc<Mutex<ProviderRegistry>>,
        event_tx: tokio::sync::mpsc::Sender<StepProgress>,
    ) -> Self {
        Self { tools, db, providers, event_tx }
    }

    pub async fn run(&self, workflow: &WorkflowDef) -> Result<HashMap<String, Value>> {
        let run_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();

        let trigger_type = match &workflow.trigger {
            TriggerDef::Manual => "manual".to_string(),
            TriggerDef::Cron { .. } => "cron".to_string(),
            TriggerDef::FileWatch { .. } => "file_watch".to_string(),
        };

        // Create run record
        {
            let db = self.db.lock().await;
            let record = WorkflowRunRecord {
                id: run_id.clone(),
                workflow_name: workflow.name.clone(),
                status: "running".to_string(),
                step_results: None,
                step_progress: None,
                error: None,
                trigger_type,
                started_at: now.clone(),
                finished_at: None,
            };
            if let Err(e) = db.insert_workflow_run(&record) {
                log::warn!("Failed to persist workflow run start: {}", e);
            }
        }

        let mut step_results: HashMap<String, Value> = HashMap::new();
        // Workflow vars and secrets for template rendering (to be injected by caller with config)
        let workflow_vars: HashMap<String, String> = HashMap::new();
        let workflow_secrets: HashMap<String, String> = HashMap::new();
        let mut step_progress_list: Vec<StepProgress> = Vec::new();
        let max_steps = 50;

        for (i, step) in workflow.steps.iter().enumerate() {
            if i >= max_steps {
                return Err(AppError::Tool("Workflow exceeded max steps (50)".to_string()));
            }

            let step_id = match step {
                StepDef::ToolCall { id, .. } => id.clone(),
                StepDef::LlmCall { id, .. } => id.clone(),
                StepDef::Condition { id, .. } => id.clone(),
            };

            // Emit running status
            let _ = self.event_tx
                .send(StepProgress {
                    step_id: step_id.clone(),
                    status: "running".to_string(),
                    duration_ms: None,
                    error: None,
                    result_summary: None,
                })
                .await;

            let start = Instant::now();

            match step {
                StepDef::ToolCall {
                    id,
                    tool,
                    params,
                    retry,
                    on_error,
                    timeout_seconds,
                } => {
                    let rendered_params = self.render_params(params, &step_results, &workflow_vars, &workflow_secrets);

                    let result = self
                        .execute_with_retry(
                            id,
                            retry,
                            on_error,
                            *timeout_seconds,
                            || {
                                let tools = self.tools.clone();
                                let tool = tool.clone();
                                let params = rendered_params.clone();
                                async move {
                                    let tools = tools.lock().await;
                                    tools.execute(&tool, params).await
                                }
                            },
                        )
                        .await;

                    match result {
                        Ok(val) => {
                            let summary =
                                val.as_str().map(|s| s[..s.len().min(100)].to_string());
                            let _ = self.event_tx
                                .send(StepProgress {
                                    step_id: id.clone(),
                                    status: "completed".to_string(),
                                    duration_ms: Some(start.elapsed().as_millis() as u64),
                                    error: None,
                                    result_summary: summary,
                                })
                                .await;
                            step_results.insert(id.clone(), val);
                        }
                        Err(e) => {
                            step_progress_list.push(StepProgress {
                                step_id: id.clone(),
                                status: "failed".to_string(),
                                duration_ms: Some(start.elapsed().as_millis() as u64),
                                error: Some(e.to_string()),
                                result_summary: None,
                            });
                            let db = self.db.lock().await;
                            let _ = db.update_workflow_run_status(
                                &run_id,
                                "failed",
                                Some(&e.to_string()),
                                &step_results,
                                None,
                            );
                            return Err(e);
                        }
                    }
                }

                StepDef::LlmCall {
                    id,
                    prompt,
                    model_id,
                    system_prompt,
                    max_tokens,
                    temperature,
                    retry,
                    on_error,
                    timeout_seconds,
                } => {
                    let rendered = WorkflowDef::render_template(prompt, &step_results, &workflow_vars, &workflow_secrets);

                    let result = self
                        .execute_with_retry(
                            id,
                            retry,
                            on_error,
                            *timeout_seconds,
                            || {
                                let providers = self.providers.clone();
                                let prompt = rendered.clone();
                                let sys = system_prompt.clone();
                                let m_id = model_id.clone();
                                let m_tokens = *max_tokens;
                                let temp = *temperature;
                                async move {
                                    let providers = providers.lock().await;
                                    let mid = m_id.as_deref().unwrap_or_else(|| providers.default_model_id());
                                    if mid.is_empty() {
                                        return Err(AppError::Provider(
                                            "No default model configured".to_string(),
                                        ));
                                    }
                                    let provider = providers.get(mid)?;
                                    let request = ChatRequest {
                                        messages: vec![
                                            Message {
                                                id: None,
                                                role: MessageRole::System,
                                                content: sys.unwrap_or_default(),
                                                tool_calls: None,
                                                tool_call_id: None,
                                            },
                                            Message {
                                                id: None,
                                                role: MessageRole::User,
                                                content: prompt,
                                                tool_calls: None,
                                                tool_call_id: None,
                                            },
                                        ],
                                        model: mid.to_string(),
                                        tools: None,
                                        stream: Some(false),
                                        max_tokens: m_tokens.map(|t| t as usize),
                                        temperature: temp,
                                    };
                                    let response = provider.chat(request).await?;
                                    let content = response
                                        .choices
                                        .first()
                                        .map(|c| c.message.content.clone())
                                        .unwrap_or_default();
                                    Ok(Value::String(content))
                                }
                            },
                        )
                        .await;

                    match result {
                        Ok(val) => {
                            let summary =
                                val.as_str().map(|s| s[..s.len().min(100)].to_string());
                            let _ = self.event_tx
                                .send(StepProgress {
                                    step_id: id.clone(),
                                    status: "completed".to_string(),
                                    duration_ms: Some(start.elapsed().as_millis() as u64),
                                    error: None,
                                    result_summary: summary,
                                })
                                .await;
                            step_results.insert(id.clone(), val);
                        }
                        Err(e) => {
                            step_progress_list.push(StepProgress {
                                step_id: id.clone(),
                                status: "failed".to_string(),
                                duration_ms: Some(start.elapsed().as_millis() as u64),
                                error: Some(e.to_string()),
                                result_summary: None,
                            });
                            let db = self.db.lock().await;
                            let _ = db.update_workflow_run_status(
                                &run_id,
                                "failed",
                                Some(&e.to_string()),
                                &step_results,
                                None,
                            );
                            return Err(e);
                        }
                    }
                }

                StepDef::Condition {
                    id,
                    condition,
                    on_false,
                } => {
                    let rendered = WorkflowDef::render_template(condition, &step_results, &workflow_vars, &workflow_secrets);
                    let trimmed = rendered.trim().to_lowercase();
                    let is_truthy = !rendered.is_empty()
                        && !matches!(
                            trimmed.as_str(),
                            "false" | "no" | "0" | "" | "null"
                        );
                    let _ = self.event_tx
                        .send(StepProgress {
                            step_id: id.clone(),
                            status: "completed".to_string(),
                            duration_ms: Some(start.elapsed().as_millis() as u64),
                            error: None,
                            result_summary: Some(if is_truthy {
                                "true".to_string()
                            } else {
                                "false".to_string()
                            }),
                        })
                        .await;
                    step_results.insert(id.clone(), Value::Bool(is_truthy));
                    if !is_truthy && on_false == "end" {
                        break;
                    }
                }
            }
        }

        // Persist completion with full step progress
        let step_progress_json = serde_json::to_string(&step_progress_list).ok();
        {
            let db = self.db.lock().await;
            let _ = db.update_workflow_run_status(
                &run_id,
                "completed",
                None,
                &step_results,
                step_progress_json.as_deref(),
            );
        }

        Ok(step_results)
    }

    async fn execute_with_retry<F, Fut>(
        &self,
        step_id: &str,
        retry: &Option<crate::pipeline::models::RetryConfig>,
        on_error: &str,
        timeout_seconds: Option<u64>,
        f: F,
    ) -> Result<Value>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<Value>>,
    {
        let max = retry.as_ref().map(|r| r.max).unwrap_or(1);
        let delay = retry.as_ref().map(|r| r.delay_seconds).unwrap_or(0);
        let mut last_err: Option<AppError> = None;

        for attempt in 0..max {
            let fut = f();
            let result = if let Some(timeout) = timeout_seconds {
                match tokio::time::timeout(Duration::from_secs(timeout), fut).await {
                    Ok(r) => r,
                    Err(_) => Err(AppError::Tool(format!(
                        "Step '{}' timed out after {}s",
                        step_id, timeout
                    ))),
                }
            } else {
                fut.await
            };

            match result {
                Ok(val) => return Ok(val),
                Err(e) => {
                    last_err = Some(e);
                    if attempt < max - 1 && delay > 0 {
                        tokio::time::sleep(Duration::from_secs(delay)).await;
                    }
                }
            }
        }

        match on_error {
            "skip" => Ok(Value::Null),
            _ => Err(last_err.unwrap_or_else(|| {
                AppError::Tool(format!("Step '{}' failed with unknown error", step_id))
            })),
        }
    }

    fn render_params(
        &self,
        params: &HashMap<String, Value>,
        step_results: &HashMap<String, Value>,
        vars: &HashMap<String, String>,
        secrets: &HashMap<String, String>,
    ) -> Value {
        let mut map = serde_json::Map::new();
        for (key, val) in params {
            let rendered = match val {
                Value::String(s) => {
                    Value::String(WorkflowDef::render_template(s, step_results, vars, secrets))
                }
                other => other.clone(),
            };
            map.insert(key.clone(), rendered);
        }
        Value::Object(map)
    }
}
