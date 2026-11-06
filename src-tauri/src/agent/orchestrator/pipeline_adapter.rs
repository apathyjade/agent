//! YAML 工作流的适配层：把 `WorkflowDef` 转成可执行的 `ExecutionPlan`。
//!
//! 这里只做「结构转换 + 模板渲染」，不执行任何步骤；真正的执行由
//! `runtime` 的 `ExecStep::Pipeline` 分支负责。

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::Utc;

use crate::core::util::truncate_chars;
use crate::agent::orchestrator::plan_error::ExecutionError;
use crate::agent::orchestrator::plan_types::*;
use crate::features::workflow::models::{StepDef, WorkflowDef};

/// 步骤标签中模板预览的最大字符数。
const LABEL_PREVIEW_CHARS: usize = 40;

/// 将 YAML 工作流定义转为 ExecutionPlan
pub struct PipelineAdapter;

impl PipelineAdapter {
    /// 转换 WorkflowDef → ExecutionPlan
    ///
    /// `vars` 是调用方传入的模板变量，会渲染进每个步骤的参数与提示词。
    pub fn convert(
        workflow: &WorkflowDef,
        session_id: &str,
        vars: &HashMap<String, String>,
    ) -> ExecutionPlan {
        let steps: Vec<PlanStep> = workflow
            .steps
            .iter()
            .map(|step| Self::convert_step(step, vars))
            .collect();

        ExecutionPlan {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            source: PlanSource::Static {
                workflow_name: workflow.name.clone(),
            },
            steps,
            status: PlanStatus::Pending,
            created_at: Utc::now().to_rfc3339(),
            finished_at: None,
        }
    }

    /// 把单个工作流步骤转换为 `PlanStep`。
    fn convert_step(step: &StepDef, vars: &HashMap<String, String>) -> PlanStep {
        let (id, label, execution) = match step {
            StepDef::ToolCall {
                id,
                tool,
                params,
                retry,
                timeout_seconds,
                ..
            } => {
                let rendered_params = Self::render_params(params, vars, &HashMap::new());
                (
                    id.clone(),
                    format!("工具调用: {}", tool),
                    ExecStep::ToolCall {
                        tool: tool.clone(),
                        params: rendered_params,
                        retry: retry.as_ref().map(|r| RetryConfig {
                            max: r.max,
                            delay_seconds: r.delay_seconds,
                        }),
                        timeout_seconds: *timeout_seconds,
                    },
                )
            }
            StepDef::LlmCall {
                id,
                prompt,
                model_id,
                system_prompt,
                max_tokens,
                temperature,
                ..
            } => {
                let rendered =
                    WorkflowDef::render_template(prompt, &HashMap::new(), vars, &HashMap::new());
                (
                    id.clone(),
                    format!("LLM: {}..", truncate_chars(&rendered, LABEL_PREVIEW_CHARS)),
                    ExecStep::LlmCall {
                        prompt: rendered,
                        system_prompt: system_prompt.clone(),
                        model_id: model_id.clone(),
                        temperature: *temperature,
                        max_tokens: *max_tokens,
                    },
                )
            }
            StepDef::Condition {
                id,
                condition,
                on_false,
            } => {
                let rendered = WorkflowDef::render_template(
                    condition,
                    &HashMap::new(),
                    vars,
                    &HashMap::new(),
                );
                (
                    id.clone(),
                    format!("条件: {}", truncate_chars(&rendered, LABEL_PREVIEW_CHARS)),
                    ExecStep::Condition {
                        expression: rendered,
                        on_true: BranchTarget::Continue,
                        on_false: if on_false == "end" {
                            BranchTarget::End
                        } else {
                            BranchTarget::Continue
                        },
                    },
                )
            }
        };

        PlanStep {
            id,
            label,
            execution,
            status: StepStatus::Pending,
            result: None,
            error: None,
            started_at: None,
            duration_ms: None,
        }
    }

    /// 按名称扫描并加载工作流
    pub fn load_by_name(name: &str) -> Result<WorkflowDef, ExecutionError> {
        let pipelines = Self::scan_dirs()?;
        for (_, wf) in pipelines {
            if wf.name == name {
                return Ok(wf);
            }
        }
        Err(ExecutionError::PlanNotFound(format!(
            "Workflow '{}' not found in any workflow directory",
            name
        )))
    }

    /// 列出所有可用工作流名称
    pub fn list_workflow_names() -> Result<Vec<String>, ExecutionError> {
        let pipelines = Self::scan_dirs()?;
        Ok(pipelines.into_iter().map(|(_, wf)| wf.name).collect())
    }

    /// 扫描所有工作流目录并解析出工作流定义。
    fn scan_dirs() -> Result<Vec<(PathBuf, WorkflowDef)>, ExecutionError> {
        let dirs = crate::features::workflow::scanner::scan_workflow_files()
            .map_err(|e| ExecutionError::Internal(format!("Failed to scan workflows: {}", e)))?;
        Ok(dirs)
    }

    /// 渲染工具参数中的字符串模板，非字符串值原样保留。
    fn render_params(
        params: &HashMap<String, serde_json::Value>,
        vars: &HashMap<String, String>,
        secrets: &HashMap<String, String>,
    ) -> HashMap<String, serde_json::Value> {
        let mut result = HashMap::new();
        for (key, val) in params {
            let rendered = match val {
                serde_json::Value::String(s) => serde_json::Value::String(
                    WorkflowDef::render_template(s, &HashMap::new(), vars, secrets),
                ),
                other => other.clone(),
            };
            result.insert(key.clone(), rendered);
        }
        result
    }
}
