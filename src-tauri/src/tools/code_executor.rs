use async_trait::async_trait;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

use crate::error::{AppError, Result};

use super::r#trait::Tool;

pub struct CodeExecutorTool;

impl CodeExecutorTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for CodeExecutorTool {
    fn name(&self) -> &str {
        "code_executor"
    }

    fn description(&self) -> &str {
        "Execute code snippets in Python or JavaScript. Returns stdout, stderr, and exit code. Timeout enforced for safety."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "language": {
                    "type": "string",
                    "enum": ["python", "javascript"],
                    "description": "The programming language of the code"
                },
                "code": {
                    "type": "string",
                    "description": "The code to execute"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Execution timeout in seconds (default: 30, max: 60)",
                    "default": 30
                }
            },
            "required": ["language", "code"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let language = input["language"]
            .as_str()
            .ok_or_else(|| AppError::InvalidInput("Missing 'language' parameter".to_string()))?;

        let code = input["code"]
            .as_str()
            .ok_or_else(|| AppError::InvalidInput("Missing 'code' parameter".to_string()))?;

        let timeout_secs = input["timeout"]
            .as_u64()
            .unwrap_or(30)
            .min(60);

        if code.len() > 100_000 {
            return Err(AppError::InvalidInput(
                "Code exceeds maximum length of 100,000 characters".to_string(),
            ));
        }

        let (program, arg) = match language {
            "python" => {
                if cfg!(target_os = "windows") {
                    ("python", "-c")
                } else {
                    ("python3", "-c")
                }
            }
            "javascript" => ("node", "-e"),
            _ => {
                return Err(AppError::InvalidInput(format!(
                    "Unsupported language: {}. Supported: python, javascript",
                    language
                )));
            }
        };

        let result = timeout(Duration::from_secs(timeout_secs), async {
            let output = Command::new(program)
                .arg(arg)
                .arg(code)
                .kill_on_drop(true)
                .output()
                .await
                .map_err(|e| {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        AppError::Tool(format!(
                            "'{}' interpreter not found. Is it installed and on PATH?",
                            program
                        ))
                    } else {
                        AppError::Tool(format!("Failed to execute code: {}", e))
                    }
                })?;

            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let exit_code = output.status.code().unwrap_or(-1);

            // Truncate output to prevent huge responses
            let truncated = |s: &str, limit: usize| -> String {
                if s.len() > limit {
                    format!("{}...\n[Output truncated at {} characters]", &s[..limit], limit)
                } else {
                    s.to_string()
                }
            };

            Ok::<Value, AppError>(json!({
                "language": language,
                "stdout": truncated(&stdout, 50_000),
                "stderr": truncated(&stderr, 50_000),
                "exit_code": exit_code,
                "success": exit_code == 0
            }))
        }).await;

        match result {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(AppError::Tool(format!(
                "Code execution timed out after {} seconds",
                timeout_secs
            ))),
        }
    }
}
