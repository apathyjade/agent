use async_trait::async_trait;
use serde_json::{json, Value};

use crate::error::Result;
use crate::execution::pipeline_adapter::PipelineAdapter;
use crate::tools::r#trait::Tool;

/// Tool that loads and describes a YAML workflow.
/// Does NOT execute the workflow — returns the plan for the LLM to process.
/// Actual pipeline execution is handled by ExecStep::Pipeline in the ExecutionRuntime.
pub struct RunWorkflowTool;

#[async_trait]
impl Tool for RunWorkflowTool {
    fn name(&self) -> &str {
        "run_workflow"
    }

    fn description(&self) -> &str {
        "Load a predefined workflow by name and return its steps. Available workflows: analyze_project, build_project, test_suite, deploy. The result includes each step's type, tool, and parameters."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "workflow": {
                    "type": "string",
                    "description": "The workflow name to load"
                }
            },
            "required": ["workflow"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let workflow_name = input
            .get("workflow")
            .and_then(|v| v.as_str())
            .ok_or_else(|| crate::error::AppError::InvalidInput("Missing 'workflow' parameter".to_string()))?;

        let workflow_def = PipelineAdapter::load_by_name(workflow_name)
            .map_err(|e| crate::error::AppError::Tool(format!("Failed to load workflow '{}': {}", workflow_name, e)))?;

        // Return workflow structure as JSON
        let steps: Vec<Value> = workflow_def
            .steps
            .iter()
            .map(|s| match s {
                crate::pipeline::models::StepDef::ToolCall { id, tool, params, .. } => {
                    json!({
                        "id": id,
                        "type": "tool_call",
                        "tool": tool,
                        "params": params,
                    })
                }
                crate::pipeline::models::StepDef::LlmCall { id, prompt, .. } => {
                    json!({
                        "id": id,
                        "type": "llm_call",
                        "prompt_preview": prompt.chars().take(100).collect::<String>(),
                    })
                }
                crate::pipeline::models::StepDef::Condition { id, condition, .. } => {
                    json!({
                        "id": id,
                        "type": "condition",
                        "expression": condition,
                    })
                }
            })
            .collect();

        Ok(json!({
            "workflow": workflow_def.name,
            "description": workflow_def.description,
            "steps": steps,
            "step_count": steps.len(),
        }))
    }
}
