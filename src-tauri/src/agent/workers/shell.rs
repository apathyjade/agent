//! shell worker：执行指令中内嵌的 `@@run` / git 命令，并把输出交给模型分析。
//!
//! 命令语法：
//! - `@@run(command)` —— 通过系统 shell（Windows `cmd /C`、其他 `sh -c`）执行
//! - `@@git_status` / `@@git_diff(path)` / `@@git_log(n)`
//!
//! 安全边界：只执行指令里显式写出的命令，不做任何插值或拼接。

use async_trait::async_trait;
use std::path::Path;
use std::process::Output;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::llm::provider::ProviderRegistry;
use crate::core::error::Result;
use crate::agent::workers::{SubTask, WorkerAgent, WorkerKind, WorkerResult};

const SHELL_SYSTEM_PROMPT: &str = r#"You are a shell command executor. Execute commands and provide analysis of the output."#;

/// shell 命令 worker。
pub struct ShellWorker {
    providers: Arc<Mutex<ProviderRegistry>>,
    workspace_root: Arc<Mutex<Option<String>>>,
}

impl ShellWorker {
    /// 创建 worker；工作区根目录在调用 [`Self::set_workspace_root`] 之前为空。
    pub fn new(providers: Arc<Mutex<ProviderRegistry>>) -> Self {
        Self {
            providers,
            workspace_root: Arc::new(Mutex::new(None)),
        }
    }

    /// 设置工作区根目录，git 命令都在该目录下执行。
    pub async fn set_workspace_root(&self, root: String) {
        *self.workspace_root.lock().await = Some(root);
    }

    /// 把可选的工作区根目录解析成路径，未设置时回落到当前目录。
    fn resolve_root(workspace_root: &Option<String>) -> &Path {
        workspace_root
            .as_ref()
            .map(|s| Path::new(s.as_str()))
            .unwrap_or_else(|| Path::new("."))
    }

    /// 通过系统 shell 执行命令，把 stdout、stderr 与退出码追加到结果。
    ///
    /// 注意：命令在进程当前目录执行，不切换到工作区根目录。
    fn run_shell(cmd: &str, out: &mut String) {
        let mut shell_cmd = if cfg!(target_os = "windows") {
            let mut c = std::process::Command::new("cmd");
            c.args(["/C", cmd]);
            c
        } else {
            let mut c = std::process::Command::new("sh");
            c.args(["-c", cmd]);
            c
        };

        match shell_cmd.output() {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                let exit_code = output.status.code().unwrap_or(-1);

                if !stdout.is_empty() {
                    out.push_str(&format!("stdout:\n{}\n", stdout));
                }
                if !stderr.is_empty() {
                    out.push_str(&format!("stderr:\n{}\n", stderr));
                }
                out.push_str(&format!("exit code: {}", exit_code));
            }
            Err(e) => {
                out.push_str(&format!("Failed to execute command: {}\n", e));
            }
        }
    }

    /// 把 git 输出的非空 stdout / stderr 追加到结果。
    fn append_git_output(output: &Output, out: &mut String) {
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if !stdout.is_empty() {
            out.push_str(&stdout);
            out.push('\n');
        }
        if !stderr.is_empty() {
            out.push_str(&stderr);
            out.push('\n');
        }
    }

    /// 在工作区根目录下执行 git 子命令。
    ///
    /// `error_label` 用于拼出失败提示，保持与原有输出格式一致。
    fn run_git(root: &Path, args: &[String], error_label: &str, out: &mut String) {
        let mut cmd = std::process::Command::new("git");
        cmd.args(args);
        cmd.current_dir(root);

        match cmd.output() {
            Ok(output) => Self::append_git_output(&output, out),
            Err(e) => {
                out.push_str(&format!("{}: {}\n", error_label, e));
            }
        }
    }

    /// 解析并执行指令中内嵌的 `@@run` / git 命令，返回可读的输出文本。
    async fn execute_tools(instruction: &str, root: &Path) -> String {
        let mut result = String::new();

        for line in instruction.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with("@@run(") && trimmed.ends_with(')') {
                let cmd = trimmed[6..trimmed.len() - 1].trim().trim_matches('"');
                Self::run_shell(cmd, &mut result);
            } else if trimmed == "@@git_status" {
                Self::run_git(
                    root,
                    &["status".to_string()],
                    "Git status error",
                    &mut result,
                );
            } else if trimmed.starts_with("@@git_diff(") && trimmed.ends_with(')') {
                let path_arg = trimmed[11..trimmed.len() - 1].trim().trim_matches('"');
                let mut args = vec!["diff".to_string()];
                if !path_arg.is_empty() {
                    args.push(path_arg.to_string());
                }
                Self::run_git(root, &args, "Git diff error", &mut result);
            } else if trimmed.starts_with("@@git_log(") && trimmed.ends_with(')') {
                let n_str = &trimmed[10..trimmed.len() - 1];
                let n: usize = n_str.trim().parse().unwrap_or(10);
                Self::run_git(
                    root,
                    &["log".to_string(), format!("-n{}", n)],
                    "Git log error",
                    &mut result,
                );
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

        // 先执行指令里内嵌的 @@ 命令，再把输出作为上下文交给模型。
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
                return Err(crate::core::error::AppError::Worker(
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
            crate::llm::provider::ProviderRegistry::new(&[]),
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

        // 初始化 git 仓库，让 git status 可用。
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

        // 修改文件以产生 diff。
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
