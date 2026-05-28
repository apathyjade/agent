use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::api::provider::ProviderRegistry;
use crate::error::Result;
use crate::workers::{SubTask, WorkerAgent, WorkerKind, WorkerResult};

const SHELL_SYSTEM_PROMPT: &str = r#"You are a shell command executor. Execute commands and provide analysis of the output."#;

pub struct ShellWorker {
    providers: Arc<Mutex<ProviderRegistry>>,
    workspace_root: Arc<Mutex<Option<String>>>,
}

impl ShellWorker {
    pub fn new(providers: Arc<Mutex<ProviderRegistry>>) -> Self {
        Self {
            providers,
            workspace_root: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn set_workspace_root(&self, root: String) {
        *self.workspace_root.lock().await = Some(root);
    }

    fn resolve_root(workspace_root: &Option<String>) -> &Path {
        workspace_root
            .as_ref()
            .map(|s| Path::new(s.as_str()))
            .unwrap_or_else(|| Path::new("."))
    }

    /// Parse and execute shell/git commands embedded in the instruction using @@tool_name(args) syntax.
    async fn execute_tools(instruction: &str, root: &Path) -> String {
        let mut result = String::new();

        for line in instruction.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with("@@run(") && trimmed.ends_with(')') {
                let cmd = &trimmed[6..trimmed.len() - 1];
                let cmd_trimmed = cmd.trim().trim_matches('"');

                let mut shell_cmd = if cfg!(target_os = "windows") {
                    let mut c = std::process::Command::new("cmd");
                    c.args(["/C", cmd_trimmed]);
                    c
                } else {
                    let mut c = std::process::Command::new("sh");
                    c.args(["-c", cmd_trimmed]);
                    c
                };

                match shell_cmd.output() {
                    Ok(output) => {
                        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                        let exit_code = output.status.code().unwrap_or(-1);

                        if !stdout.is_empty() {
                            result.push_str(&format!("stdout:\n{}\n", stdout));
                        }
                        if !stderr.is_empty() {
                            result.push_str(&format!("stderr:\n{}\n", stderr));
                        }
                        result.push_str(&format!("exit code: {}", exit_code));
                    }
                    Err(e) => {
                        result.push_str(&format!("Failed to execute command: {}\n", e));
                    }
                }
            } else if trimmed == "@@git_status" {
                match std::process::Command::new("git")
                    .arg("status")
                    .current_dir(root)
                    .output()
                {
                    Ok(output) => {
                        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                        if !stdout.is_empty() {
                            result.push_str(&stdout);
                            result.push('\n');
                        }
                        if !stderr.is_empty() {
                            result.push_str(&stderr);
                            result.push('\n');
                        }
                    }
                    Err(e) => {
                        result.push_str(&format!("Git status error: {}\n", e));
                    }
                }
            } else if trimmed.starts_with("@@git_diff(") && trimmed.ends_with(')') {
                let args = &trimmed[11..trimmed.len() - 1];
                let path_arg = args.trim().trim_matches('"');

                let mut cmd = std::process::Command::new("git");
                cmd.arg("diff");
                cmd.current_dir(root);
                if !path_arg.is_empty() {
                    cmd.arg(path_arg);
                }

                match cmd.output() {
                    Ok(output) => {
                        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                        if !stdout.is_empty() {
                            result.push_str(&stdout);
                            result.push('\n');
                        }
                        if !stderr.is_empty() {
                            result.push_str(&stderr);
                            result.push('\n');
                        }
                    }
                    Err(e) => {
                        result.push_str(&format!("Git diff error: {}\n", e));
                    }
                }
            } else if trimmed.starts_with("@@git_log(") && trimmed.ends_with(')') {
                let n_str = &trimmed[10..trimmed.len() - 1];
                let n: usize = n_str.trim().parse().unwrap_or(10);

                match std::process::Command::new("git")
                    .args(["log", &format!("-n{}", n)])
                    .current_dir(root)
                    .output()
                {
                    Ok(output) => {
                        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                        if !stdout.is_empty() {
                            result.push_str(&stdout);
                            result.push('\n');
                        }
                        if !stderr.is_empty() {
                            result.push_str(&stderr);
                            result.push('\n');
                        }
                    }
                    Err(e) => {
                        result.push_str(&format!("Git log error: {}\n", e));
                    }
                }
            }
        }

        result.trim().to_string()
    }
}

#[async_trait]
impl WorkerAgent for ShellWorker {
    fn kind(&self) -> WorkerKind {
        WorkerKind::Shell
    }

    fn description(&self) -> &str {
        "Executes shell commands and git operations (status, diff, log) and provides analysis of the output."
    }

    async fn execute(&self, task: SubTask) -> Result<WorkerResult> {
        let start = std::time::Instant::now();
        let root = self.workspace_root.lock().await;
        let root_path = Self::resolve_root(&root);

        // Execute any @@tool commands embedded in the instruction
        let tool_results = Self::execute_tools(&task.instruction, root_path).await;

        let system_prompt = format!(
            "{}\n\nCurrent workspace root: {}\n\n## Command Output\n{}",
            SHELL_SYSTEM_PROMPT,
            root_path.display(),
            if tool_results.is_empty() {
                "No commands executed — no @@tool commands found in instruction.".to_string()
            } else {
                tool_results
            }
        );

        let provider = {
            let registry = self.providers.lock().await;
            let mid = task.model_id.as_deref().unwrap_or_else(|| registry.default_model_id());
            if mid.is_empty() {
                drop(root);
                return Err(crate::error::AppError::Worker(
                    "No model configured for ShellWorker".into(),
                ));
            }
            registry.get(mid)?
        };

        let content = provider
            .prompt(&system_prompt, &task.instruction)
            .await?;

        drop(root);

        Ok(WorkerResult {
            worker: WorkerKind::Shell,
            task_id: task.id,
            content,
            metadata: None,
            duration_ms: Some(start.elapsed().as_millis() as u64),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_shell_kind() {
        let providers = Arc::new(Mutex::new(
            crate::api::provider::ProviderRegistry::new(&crate::config::AppConfig::default()),
        ));
        let worker = ShellWorker::new(providers);
        assert_eq!(worker.kind(), WorkerKind::Shell);
    }

    #[tokio::test]
    async fn test_execute_run_echo() {
        let dir = TempDir::new().unwrap();
        let instruction = "@@run(\"echo hello\")\n";
        let result = ShellWorker::execute_tools(instruction, dir.path()).await;
        assert!(
            result.contains("hello") || result.contains("exit code: 0"),
            "expected echo output or exit code 0, got: {}",
            result
        );
    }

    #[tokio::test]
    async fn test_execute_run_fail() {
        let dir = TempDir::new().unwrap();
        let instruction = "@@run(\"exit 1\")\n";
        let result = ShellWorker::execute_tools(instruction, dir.path()).await;
        assert!(
            result.contains("exit code: 1"),
            "expected exit code 1, got: {}",
            result
        );
    }

    #[tokio::test]
    async fn test_execute_git_status() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("test.txt"), "hello").unwrap();

        // Init a git repo so git status works
        let _ = std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output();
        let _ = std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir.path())
            .output();
        let _ = std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir.path())
            .output();

        let instruction = "@@git_status\n";
        let result = ShellWorker::execute_tools(instruction, dir.path()).await;
        assert!(
            result.contains("test.txt") || result.contains("Untracked"),
            "expected git status with test.txt, got: {}",
            result
        );
    }

    #[tokio::test]
    async fn test_execute_git_diff() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("file.txt"), "original").unwrap();

        let _ = std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output();
        let _ = std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir.path())
            .output();
        let _ = std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir.path())
            .output();
        let _ = std::process::Command::new("git")
            .args(["add", "file.txt"])
            .current_dir(dir.path())
            .output();
        let _ = std::process::Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(dir.path())
            .output();

        // Modify the file to create a diff
        fs::write(dir.path().join("file.txt"), "modified").unwrap();

        let instruction = "@@git_diff(\"file.txt\")\n";
        let result = ShellWorker::execute_tools(instruction, dir.path()).await;
        assert!(
            result.contains("diff") || result.contains("modified"),
            "expected git diff content, got: {}",
            result
        );
    }

    #[tokio::test]
    async fn test_execute_git_log() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("file.txt"), "content").unwrap();

        let _ = std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output();
        let _ = std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir.path())
            .output();
        let _ = std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir.path())
            .output();
        let _ = std::process::Command::new("git")
            .args(["add", "file.txt"])
            .current_dir(dir.path())
            .output();
        let _ = std::process::Command::new("git")
            .args(["commit", "-m", "initial commit"])
            .current_dir(dir.path())
            .output();

        let instruction = "@@git_log(5)\n";
        let result = ShellWorker::execute_tools(instruction, dir.path()).await;
        assert!(
            result.contains("commit") || result.contains("initial commit"),
            "expected git log with commit info, got: {}",
            result
        );
    }

    #[tokio::test]
    async fn test_execute_no_commands() {
        let dir = TempDir::new().unwrap();
        let instruction = "Just a plain instruction without tool commands.";
        let result = ShellWorker::execute_tools(instruction, dir.path()).await;
        assert!(result.is_empty(), "should return empty when no @@ commands");
    }
}
