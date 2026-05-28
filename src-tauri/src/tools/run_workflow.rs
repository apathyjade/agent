use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;

use crate::orchestrator::pipeline_adapter::PipelineAdapter;

use rig::completion::ToolDefinition;
use rig::tool::{ToolDyn, ToolError};

/// Tool that loads and describes a YAML workflow.
/// Does NOT execute the workflow — returns the plan for the LLM to process.
/// Actual pipeline execution is handled by ExecStep::Pipeline in the ExecutionRuntime.
pub struct RunWorkflowTool;

impl ToolDyn for RunWorkflowTool {
    fn name(&self) -> String {
        "run_workflow".to_string()
    }

    fn definition<'a>(
        &'a self,
        _prompt: String,
    ) -> Pin<Box<dyn Future<Output = ToolDefinition> + Send + 'a>> {
        Box::pin(async move {
            ToolDefinition {
                name: "run_workflow".to_string(),
                description:
                    "Load a predefined workflow by name and return its steps. Available workflows: analyze_project, build_project, test_suite, deploy. The result includes each step's type, tool, and parameters."
                        .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "workflow": {
                            "type": "string",
                            "description": "The workflow name to load"
                        }
                    },
                    "required": ["workflow"]
                }),
            }
        })
    }

    fn call<'a>(
        &'a self,
        args: String,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<String, ToolError>> + Send + 'a>>
    {
        Box::pin(async move {
            let input: Value = serde_json::from_str(&args)
                .map_err(|e| ToolError::JsonError(e))?;

            let workflow_name = input
                .get("workflow")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    ToolError::ToolCallError("Missing 'workflow' parameter".to_string().into())
                })?;

            let workflow_def = PipelineAdapter::load_by_name(workflow_name).map_err(|e| {
                ToolError::ToolCallError(
                    format!("Failed to load workflow '{}': {}", workflow_name, e).into(),
                )
            })?;

            // Return workflow structure as JSON
            let steps: Vec<Value> = workflow_def
                .steps
                .iter()
                .map(|s| match s {
                    crate::pipeline::models::StepDef::ToolCall {
                        id, tool, params, ..
                    } => {
                        json!({
                            "id": id,
                            "type": "tool_call",
                            "tool": tool,
                            "params": params,
                        })
                    }
                    crate::pipeline::models::StepDef::LlmCall {
                        id, prompt, ..
                    } => {
                        json!({
                            "id": id,
                            "type": "llm_call",
                            "prompt_preview": prompt.chars().take(100).collect::<String>(),
                        })
                    }
                    crate::pipeline::models::StepDef::Condition {
                        id, condition, ..
                    } => {
                        json!({
                            "id": id,
                            "type": "condition",
                            "expression": condition,
                        })
                    }
                })
                .collect();

            serde_json::to_string(&json!({
                "workflow": workflow_def.name,
                "description": workflow_def.description,
                "steps": steps,
                "step_count": steps.len(),
            }))
            .map_err(|e| ToolError::JsonError(e))
        })
    }
}
