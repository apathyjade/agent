use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TriggerDef {
    #[default]
    Manual,
    Cron { schedule: String },
    FileWatch { path: String, pattern: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDef {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub trigger: TriggerDef,
    #[serde(default)]
    pub steps: Vec<StepDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StepDef {
    ToolCall {
        id: String,
        tool: String,
        #[serde(default)]
        params: HashMap<String, Value>,
        #[serde(default)]
        retry: Option<RetryConfig>,
        #[serde(default = "default_on_error")]
        on_error: String,
        #[serde(default)]
        timeout_seconds: Option<u64>,
    },
    LlmCall {
        id: String,
        #[serde(default)]
        prompt: String,
        #[serde(default)]
        model_id: Option<String>,
        #[serde(default)]
        system_prompt: Option<String>,
        #[serde(default)]
        max_tokens: Option<u32>,
        #[serde(default)]
        temperature: Option<f32>,
        #[serde(default)]
        retry: Option<RetryConfig>,
        #[serde(default = "default_on_error")]
        on_error: String,
        #[serde(default)]
        timeout_seconds: Option<u64>,
    },
    Condition {
        id: String,
        condition: String,
        #[serde(default = "default_on_false")]
        on_false: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    pub max: u32,
    #[serde(default = "default_retry_delay")]
    pub delay_seconds: u64,
}

fn default_retry_delay() -> u64 { 3 }
fn default_on_error() -> String { "fail".to_string() }
fn default_on_false() -> String { "end".to_string() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepProgress {
    pub step_id: String,
    pub status: String,
    pub duration_ms: Option<u64>,
    pub error: Option<String>,
    pub result_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRunRecord {
    pub id: String,
    pub workflow_name: String,
    pub status: String, // "running", "completed", "failed", "cancelled"
    pub step_results: Option<String>, // JSON HashMap<String, Value>
    pub step_progress: Option<String>,
    pub error: Option<String>,
    pub trigger_type: String, // "manual" or "scheduled"
    pub started_at: String,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowInfo {
    pub name: String,
    pub description: String,
    pub step_count: usize,
    pub file_path: String,
    pub trigger: String,
    pub next_run_at: Option<String>,
    pub last_run_status: Option<String>,
    pub last_run_at: Option<String>,
}

impl WorkflowDef {
    pub fn render_template(
        template: &str,
        step_results: &HashMap<String, Value>,
        vars: &HashMap<String, String>,
        secrets: &HashMap<String, String>,
    ) -> String {
        let mut result = template.to_string();
        // Render step results
        for (step_id, value) in step_results {
            let placeholder = format!("{{{{ steps.{}.result }}}}", step_id);
            let rendered = match value {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            result = result.replace(&placeholder, &rendered);
        }
        // Render vars
        for (key, val) in vars {
            let placeholder = format!("{{{{ vars.{} }}}}", key);
            result = result.replace(&placeholder, val);
        }
        // Render secrets
        for (key, val) in secrets {
            let placeholder = format!("{{{{ secrets.{} }}}}", key);
            result = result.replace(&placeholder, val);
        }
        result
    }
}
