//! 工作流执行引擎。
//!
//! 按顺序执行 YAML 定义的步骤，把每步状态通过 `event_tx` 推给前端，并把
//! 运行记录写进数据库。步骤失败会立即终止整次运行；`on_error: skip` 的
//! 步骤在重试耗尽后返回 `Null` 而不是中断。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::Mutex;

use crate::llm::provider::ProviderRegistry;
use crate::core::error::{AppError, Result};
use crate::features::workflow::models::{
    StepDef, StepProgress, TriggerDef, WorkflowDef, WorkflowRunRecord,
};
use crate::agent::tools::registry::ToolRegistry;

/// 单个工作流允许的最大步骤数，防止配置错误导致无限执行。
const MAX_STEPS: usize = 50;
/// 步骤结果摘要写入事件时保留的字符数（按字符而非字节，避免切断中文）。
const SUMMARY_CHARS: usize = 100;

/// 执行工作流定义，持有工具注册表、数据库与 provider 注册表的共享句柄。
pub struct PipelineEngine {
    tools: Arc<Mutex<ToolRegistry>>,
    db: Arc<Mutex<crate::core::db::repository::Database>>,
    providers: Arc<Mutex<ProviderRegistry>>,
    event_tx: tokio::sync::mpsc::Sender<StepProgress>,
}

impl PipelineEngine {
    /// 用共享句柄构造引擎；`event_tx` 用于向调用方推送步骤进度。
    pub fn new(
        tools: Arc<Mutex<ToolRegistry>>,
        db: Arc<Mutex<crate::core::db::repository::Database>>,
        providers: Arc<Mutex<ProviderRegistry>>,
        event_tx: tokio::sync::mpsc::Sender<StepProgress>,
    ) -> Self {
        Self { tools, db, providers, event_tx }
    }

    /// 执行整条工作流，返回按步骤 id 索引的结果。
    ///
    /// 开始时会插入一条 `running` 运行记录；成功结束后改写为 `completed`，
    /// 任一步失败则改写为 `failed` 并返回该错误。
    pub async fn run(&self, workflow: &WorkflowDef) -> Result<HashMap<String, Value>> {
        let run_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();

        let trigger_type = match &workflow.trigger {
            TriggerDef::Manual => "manual".to_string(),
            TriggerDef::Cron { .. } => "cron".to_string(),
            TriggerDef::FileWatch { .. } => "file_watch".to_string(),
        };

        self.insert_run_record(workflow, &run_id, trigger_type, &now)
            .await;

        let mut step_results: HashMap<String, Value> = HashMap::new();
        // 工作流变量与密钥用于模板渲染，当前由调用方在外部注入。
        let workflow_vars: HashMap<String, String> = HashMap::new();
        let workflow_secrets: HashMap<String, String> = HashMap::new();
        let mut step_progress_list: Vec<StepProgress> = Vec::new();

        for (i, step) in workflow.steps.iter().enumerate() {
            if i >= MAX_STEPS {
                return Err(AppError::Tool(format!(
                    "Workflow exceeded max steps ({MAX_STEPS})"
                )));
            }

            let step_id = step_id_of(step);

            self.emit_progress(&step_id, "running", None, None, None)
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
                    let rendered_params =
                        self.render_params(params, &step_results, &workflow_vars, &workflow_secrets);

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
                            self.emit_step_completed(id, start, &val).await;
                            step_results.insert(id.clone(), val);
                        }
                        Err(e) => {
                            self.record_step_failure(
                                &run_id,
                                id,
                                start,
                                &e,
                                &mut step_progress_list,
                                &step_results,
                            )
                            .await;
                            return Err(e);
                        }
                    }
                }

                StepDef::LlmCall {
                    id,
                    prompt,
                    model_id,
                    system_prompt,
                    max_tokens: _max_tokens,
                    temperature: _temperature,
                    retry,
                    on_error,
                    timeout_seconds,
                } => {
                    let rendered = WorkflowDef::render_template(
                        prompt,
                        &step_results,
                        &workflow_vars,
                        &workflow_secrets,
                    );

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
                                async move {
                                    let providers = providers.lock().await;
                                    let mid = m_id
                                        .as_deref()
                                        .unwrap_or_else(|| providers.default_model_id());
                                    if mid.is_empty() {
                                        return Err(AppError::Provider(
                                            "No default model configured".to_string(),
                                        ));
                                    }
                                    let provider = providers.get(mid)?;
                                    let system = sys.as_deref().unwrap_or("");
                                    let content = provider.prompt(system, &prompt).await?;
                                    Ok(Value::String(content))
                                }
                            },
                        )
                        .await;

                    match result {
                        Ok(val) => {
                            self.emit_step_completed(id, start, &val).await;
                            step_results.insert(id.clone(), val);
                        }
                        Err(e) => {
                            self.record_step_failure(
                                &run_id,
                                id,
                                start,
                                &e,
                                &mut step_progress_list,
                                &step_results,
                            )
                            .await;
                            return Err(e);
                        }
                    }
                }

                StepDef::Condition {
                    id,
                    condition,
                    on_false,
                } => {
                    let rendered = WorkflowDef::render_template(
                        condition,
                        &step_results,
                        &workflow_vars,
                        &workflow_secrets,
                    );
                    let trimmed = rendered.trim().to_lowercase();
                    let is_truthy = !rendered.is_empty()
                        && !matches!(
                            trimmed.as_str(),
                            "false" | "no" | "0" | "" | "null"
                        );
                    let summary = if is_truthy { "true" } else { "false" }.to_string();
                    self.emit_progress(
                        id,
                        "completed",
                        Some(start.elapsed().as_millis() as u64),
                        None,
                        Some(summary),
                    )
                    .await;
                    step_results.insert(id.clone(), Value::Bool(is_truthy));
                    if !is_truthy && on_false == "end" {
                        break;
                    }
                }
            }
        }

        // 成功结束：写入完整步骤进度。
        let step_progress_json = serde_json::to_string(&step_progress_list).ok();
        let db = self.db.lock().await;
        let _ = db.update_workflow_run_status(
            &run_id,
            "completed",
            None,
            &step_results,
            step_progress_json.as_deref(),
        );

        Ok(step_results)
    }

    /// 插入一条 `running` 状态的运行记录；持久化失败只记日志，不阻断执行。
    async fn insert_run_record(
        &self,
        workflow: &WorkflowDef,
        run_id: &str,
        trigger_type: String,
        now: &str,
    ) {
        let db = self.db.lock().await;
        let record = WorkflowRunRecord {
            id: run_id.to_string(),
            workflow_name: workflow.name.clone(),
            status: "running".to_string(),
            step_results: None,
            step_progress: None,
            error: None,
            trigger_type,
            started_at: now.to_string(),
            finished_at: None,
        };
        if let Err(e) = db.insert_workflow_run(&record) {
            log::warn!("Failed to persist workflow run start: {}", e);
        }
    }

    /// 推送一次步骤状态；接收端已关闭时静默忽略。
    async fn emit_progress(
        &self,
        step_id: &str,
        status: &str,
        duration_ms: Option<u64>,
        error: Option<String>,
        result_summary: Option<String>,
    ) {
        let _ = self
            .event_tx
            .send(StepProgress {
                step_id: step_id.to_string(),
                status: status.to_string(),
                duration_ms,
                error,
                result_summary,
            })
            .await;
    }

    /// 推送步骤成功事件，并把结果值截断成摘要。
    async fn emit_step_completed(&self, step_id: &str, start: Instant, value: &Value) {
        let summary = value
            .as_str()
            .map(|s| crate::core::util::truncate_chars(s, SUMMARY_CHARS).to_string());
        self.emit_progress(
            step_id,
            "completed",
            Some(start.elapsed().as_millis() as u64),
            None,
            summary,
        )
        .await;
    }

    /// 记录失败步骤：推入本地进度列表并把运行记录改写为 `failed`。
    async fn record_step_failure(
        &self,
        run_id: &str,
        step_id: &str,
        start: Instant,
        error: &AppError,
        step_progress_list: &mut Vec<StepProgress>,
        step_results: &HashMap<String, Value>,
    ) {
        step_progress_list.push(StepProgress {
            step_id: step_id.to_string(),
            status: "failed".to_string(),
            duration_ms: Some(start.elapsed().as_millis() as u64),
            error: Some(error.to_string()),
            result_summary: None,
        });
        let db = self.db.lock().await;
        let _ = db.update_workflow_run_status(
            run_id,
            "failed",
            Some(&error.to_string()),
            step_results,
            None,
        );
    }

    /// 按重试配置执行单个步骤，支持超时。
    ///
    /// 重试耗尽后，`on_error: skip` 返回 `Null`，其他值返回最后一次错误。
    async fn execute_with_retry<F, Fut>(
        &self,
        step_id: &str,
        retry: &Option<crate::features::workflow::models::RetryConfig>,
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

    /// 对工具参数里的字符串值做模板渲染，非字符串值原样透传。
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

/// 取出步骤的 id；三种步骤定义都要求 `id`。
fn step_id_of(step: &StepDef) -> String {
    match step {
        StepDef::ToolCall { id, .. } => id.clone(),
        StepDef::LlmCall { id, .. } => id.clone(),
        StepDef::Condition { id, .. } => id.clone(),
    }
}
