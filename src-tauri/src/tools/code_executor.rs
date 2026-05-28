use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

use rig::completion::ToolDefinition;
use rig::tool::{ToolDyn, ToolError};

pub struct CodeExecutorTool;

impl CodeExecutorTool {
    pub fn new() -> Self {
        Self
    }
}

impl ToolDyn for CodeExecutorTool {
    fn name(&self) -> String {
        "code_executor".to_string()
    }

    fn definition<'a>(
        &'a self,
        _prompt: String,
    ) -> Pin<Box<dyn Future<Output = ToolDefinition> + Send + 'a>> {
        Box::pin(async move {
            ToolDefinition {
                name: "code_executor".to_string(),
                description:
                    "Execute code snippets in Python or JavaScript. Returns stdout, stderr, and exit code. Timeout enforced for safety."
                        .to_string(),
                parameters: json!({
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

            let language = input["language"]
                .as_str()
                .ok_or_else(|| {
                    ToolError::ToolCallError("Missing 'language' parameter".to_string().into())
                })?;

            let code = input["code"]
                .as_str()
                .ok_or_else(|| {
                    ToolError::ToolCallError("Missing 'code' parameter".to_string().into())
                })?;

            let timeout_secs = input["timeout"].as_u64().unwrap_or(30).min(60);

            if code.len() > 100_000 {
                return Err(ToolError::ToolCallError(
                    "Code exceeds maximum length of 100,000 characters"
                        .to_string()
                        .into(),
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
                    return Err(ToolError::ToolCallError(
                        format!(
                            "Unsupported language: {}. Supported: python, javascript",
                            language
                        )
                        .into(),
                    ));
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
                            ToolError::ToolCallError(
                                format!(
                                    "'{}' interpreter not found. Is it installed and on PATH?",
                                    program
                                )
                                .into(),
                            )
                        } else {
                            ToolError::ToolCallError(
                                format!("Failed to execute code: {}", e).into(),
                            )
                        }
                    })?;

                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let exit_code = output.status.code().unwrap_or(-1);

                // Truncate output to prevent huge responses
                let truncated = |s: &str, limit: usize| -> String {
                    if s.len() > limit {
                        format!(
                            "{}...\n[Output truncated at {} characters]",
                            &s[..limit],
                            limit
                        )
                    } else {
                        s.to_string()
                    }
                };

                Ok::<Value, ToolError>(json!({
                    "language": language,
                    "stdout": truncated(&stdout, 50_000),
                    "stderr": truncated(&stderr, 50_000),
                    "exit_code": exit_code,
                    "success": exit_code == 0
                }))
            })
            .await;

            match result {
                Ok(Ok(value)) => serde_json::to_string(&value).map_err(|e| ToolError::JsonError(e)),
                Ok(Err(e)) => Err(e),
                Err(_) => Err(ToolError::ToolCallError(
                    format!("Code execution timed out after {} seconds", timeout_secs).into(),
                )),
            }
        })
    }
}
