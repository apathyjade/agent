use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};

use crate::error::{AppError, Result};

use super::r#trait::Tool;

pub struct ScriptTool {
    #[allow(dead_code)]
    skill_id: String,
    name: String,
    description: String,
    parameters: Value,
    interpreter: String,
    script_path: String,
    timeout_secs: u64,
    config: Arc<Mutex<Option<Value>>>,
}

impl ScriptTool {
    pub fn new(
        skill_id: &str,
        name: &str,
        description: &str,
        parameters: Option<Value>,
        interpreter: &str,
        script_path: &str,
        timeout_secs: u64,
    ) -> Self {
        Self {
            skill_id: skill_id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            parameters: parameters.unwrap_or_else(|| {
                json!({
                    "type": "object",
                    "properties": {
                        "input": {
                            "type": "string",
                            "description": "Input for the skill"
                        }
                    }
                })
            }),
            interpreter: interpreter.to_string(),
            script_path: script_path.to_string(),
            timeout_secs,
            config: Arc::new(Mutex::new(None)),
        }
    }

    /// Update the config at runtime
    pub async fn update_config(&self, config: Value) {
        let mut c = self.config.lock().await;
        *c = Some(config);
    }

    /// Resolve interpreter path using `where` (Windows) or `which` (Unix)
    #[allow(dead_code)]
    fn resolve_interpreter(interpreter: &str) -> String {
        // For common interpreters, just return as-is (let OS resolve via PATH)
        // Users can also provide full paths
        if Path::new(interpreter).is_absolute() {
            return interpreter.to_string();
        }
        // Return as-is; Command::new will search PATH
        interpreter.to_string()
    }
}

#[async_trait]
impl Tool for ScriptTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> Value {
        self.parameters.clone()
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let config = self.config.lock().await;

        // Build the stdin payload
        let payload = json!({
            "params": input,
            "config": *config,
        });

        drop(config); // release lock before spawning process

        let script = self.script_path.clone();

        // Run the subprocess
        let result =
            run_script(&self.interpreter, &script, &payload.to_string(), self.timeout_secs).await?;

        Ok(result)
    }
}

async fn run_script(
    interpreter: &str,
    script: &str,
    stdin_data: &str,
    timeout_secs: u64,
) -> Result<Value> {
    let mut cmd = Command::new(interpreter);
    cmd.arg(script)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| {
        AppError::Skill(format!("Failed to spawn script process: {}", e))
    })?;

    // Write to stdin
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        stdin
            .write_all(stdin_data.as_bytes())
            .await
            .map_err(|e| {
                AppError::Skill(format!("Failed to write to script stdin: {}", e))
            })?;
        stdin.flush().await.ok();
    }
    drop(child.stdin.take());

    // Wait with timeout
    let output = match timeout(Duration::from_secs(timeout_secs), child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            return Err(AppError::Skill(format!("Script process error: {}", e)));
        }
        Err(_) => {
            return Err(AppError::Skill(format!(
                "Script execution timed out after {}s",
                timeout_secs
            )));
        }
    };

    // Parse stdout as JSON
    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let stderr_str = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        // Try to parse stdout as error JSON first
        if let Ok(val) = serde_json::from_str::<Value>(&stdout_str) {
            if let Some(err_msg) = val.get("error").and_then(|v| v.as_str()) {
                return Err(AppError::Skill(err_msg.to_string()));
            }
        }
        // Fall back to stderr
        let err_msg = if !stderr_str.is_empty() {
            stderr_str.trim().to_string()
        } else {
            format!(
                "Script exited with code {}",
                output.status.code().unwrap_or(-1)
            )
        };
        return Err(AppError::Skill(err_msg));
    }

    // Parse stdout JSON result
    if stdout_str.trim().is_empty() {
        return Ok(json!(null));
    }

    match serde_json::from_str::<Value>(stdout_str.trim()) {
        Ok(val) => {
            // Check for error response
            if let Some(err_msg) = val.get("error").and_then(|v| v.as_str()) {
                return Err(AppError::Skill(err_msg.to_string()));
            }
            // Return result field if present, otherwise whole value
            Ok(val.get("result").cloned().unwrap_or(val))
        }
        Err(_) => {
            // If not JSON, return as string
            Ok(json!(stdout_str.trim()))
        }
    }
}
