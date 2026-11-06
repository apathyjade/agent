//! 代码编辑 worker：解析指令中内嵌的 `@@command`，对工作区文件做增删改查。
//!
//! 命令语法：
//! - `@@read_file(path)`
//! - `@@write_file(path)` —— 正文从下一行开始，直到下一个 `@@` 命令
//! - `@@create_file(path)`
//! - `@@delete_file(path)`
//! - `@@rename_file(from, to)`
//!
//! 所有相对路径都以工作区根目录为基准；绝对路径原样使用。

use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::llm::provider::ProviderRegistry;
use crate::core::error::Result;
use crate::agent::workers::{SubTask, WorkerAgent, WorkerKind, WorkerResult};

/// 读文件时展示的最大行数。
const READ_PREVIEW_LINES: usize = 200;

const EDITOR_SYSTEM_PROMPT: &str = r#"You are a code editing agent. Your job is to modify files in a codebase.

You have access to the following tools, which are executed automatically when you include them in your response:

- @@read_file(path) — Read the first 200 lines of a file
- @@write_file(path) — Write content to a file (content follows on subsequent lines)
- @@create_file(path) — Create a new empty file
- @@delete_file(path) — Delete a file or directory
- @@rename_file(from, to) — Rename or move a file

Commands are embedded in your instructions and will be executed automatically. Provide the file changes needed based on the task description."#;

/// 代码编辑 worker。
pub struct CodeEditorWorker {
    providers: Arc<Mutex<ProviderRegistry>>,
    workspace_root: Arc<Mutex<Option<String>>>,
}

impl CodeEditorWorker {
    /// 创建 worker；工作区根目录在调用 [`Self::set_workspace_root`] 之前为空。
    pub fn new(providers: Arc<Mutex<ProviderRegistry>>) -> Self {
        Self {
            providers,
            workspace_root: Arc::new(Mutex::new(None)),
        }
    }

    /// 设置工作区根目录，相对路径命令都以它为基准。
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

    /// 把命令参数解析为目标路径：绝对路径原样使用，相对路径拼到工作区根目录下。
    fn resolve_target(root: &Path, raw: &str) -> PathBuf {
        let trimmed = raw.trim().trim_matches('"');
        if Path::new(trimmed).is_absolute() {
            PathBuf::from(trimmed)
        } else {
            root.join(trimmed)
        }
    }

    /// 收集 `@@write_file` 之后的正文行，直到下一个 `@@` 命令或文本结束。
    ///
    /// 返回正文与「下一行待处理」的下标。
    fn collect_body_lines(lines: &[&str], start: usize) -> (String, usize) {
        let mut i = start;
        let mut content_lines: Vec<&str> = Vec::new();
        while i < lines.len() {
            if lines[i].trim().starts_with("@@") {
                break;
            }
            content_lines.push(lines[i]);
            i += 1;
        }
        (content_lines.join("\n"), i)
    }

    /// 处理 `@@read_file`：输出带行号的前 200 行预览。
    fn handle_read(root: &Path, arg: &str, out: &mut String) {
        let file_path = Self::resolve_target(root, arg);
        match std::fs::read_to_string(&file_path) {
            Ok(content) => {
                let all_lines: Vec<&str> = content.lines().collect();
                let preview: String = all_lines
                    .iter()
                    .take(READ_PREVIEW_LINES)
                    .enumerate()
                    .map(|(i, l)| format!("{:>4}: {}", i + 1, l))
                    .collect::<Vec<_>>()
                    .join("\n");
                if all_lines.len() > READ_PREVIEW_LINES {
                    out.push_str(&format!(
                        "--- {} ({} lines, showing first {}) ---\n{}\n",
                        file_path.display(),
                        all_lines.len(),
                        READ_PREVIEW_LINES,
                        preview
                    ));
                } else {
                    out.push_str(&format!(
                        "--- {} ({} lines) ---\n{}\n",
                        file_path.display(),
                        all_lines.len(),
                        preview
                    ));
                }
            }
            Err(e) => out.push_str(&format!("Read error: {}\n", e)),
        }
    }

    /// 处理 `@@write_file`：按需创建父目录后写入正文。
    fn handle_write(
        root: &Path,
        arg: &str,
        content: &str,
        line_count: usize,
        out: &mut String,
    ) {
        let file_path = Self::resolve_target(root, arg);

        if let Some(parent) = file_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        match std::fs::write(&file_path, content) {
            Ok(_) => {
                if content.is_empty() {
                    out.push_str(&format!("Wrote {} (empty file)\n", file_path.display()));
                } else {
                    out.push_str(&format!(
                        "Wrote {} ({} bytes, {} lines)\n",
                        file_path.display(),
                        content.len(),
                        line_count
                    ));
                }
            }
            Err(e) => out.push_str(&format!("Write error: {}\n", e)),
        }
    }

    /// 处理 `@@create_file`：创建一个空文件。
    fn handle_create(root: &Path, arg: &str, out: &mut String) {
        let file_path = Self::resolve_target(root, arg);

        if let Some(parent) = file_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::File::create(&file_path) {
            Ok(_) => {
                out.push_str(&format!("Created file {}\n", file_path.display()));
            }
            Err(e) => out.push_str(&format!("Create error: {}\n", e)),
        }
    }

    /// 处理 `@@delete_file`：文件直接删除，目录递归删除。
    fn handle_delete(root: &Path, arg: &str, out: &mut String) {
        let file_path = Self::resolve_target(root, arg);
        let path = file_path.as_path();

        if path.is_dir() {
            match std::fs::remove_dir_all(path) {
                Ok(_) => {
                    out.push_str(&format!("Deleted directory {}\n", path.display()));
                }
                Err(e) => out.push_str(&format!("Delete error: {}\n", e)),
            }
        } else {
            match std::fs::remove_file(path) {
                Ok(_) => {
                    out.push_str(&format!("Deleted file {}\n", path.display()));
                }
                Err(e) => out.push_str(&format!("Delete error: {}\n", e)),
            }
        }
    }

    /// 处理 `@@rename_file`：移动或重命名文件，按需创建目标父目录。
    fn handle_rename(root: &Path, args_str: &str, out: &mut String) {
        let (from_str, to_str) = if let Some(comma) = args_str.find(',') {
            (
                args_str[..comma].trim().trim_matches('"'),
                args_str[comma + 1..].trim().trim_matches('"'),
            )
        } else {
            out.push_str("Rename error: expected two arguments (from, to)\n");
            return;
        };

        let from_path = Self::resolve_target(root, from_str);
        let to_path = Self::resolve_target(root, to_str);

        if let Some(parent) = to_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        match std::fs::rename(&from_path, &to_path) {
            Ok(_) => {
                out.push_str(&format!(
                    "Renamed {} to {}\n",
                    from_path.display(),
                    to_path.display()
                ));
            }
            Err(e) => out.push_str(&format!("Rename error: {}\n", e)),
        }
    }

    /// 解析并执行指令中内嵌的 `@@command`，返回可读的执行结果文本。
    async fn execute_tools(instruction: &str, root: &Path) -> String {
        let mut result = String::new();
        let lines: Vec<&str> = instruction.lines().collect();
        let mut i = 0;

        while i < lines.len() {
            let trimmed = lines[i].trim();

            if trimmed.starts_with("@@read_file(") && trimmed.ends_with(')') {
                Self::handle_read(root, &trimmed[12..trimmed.len() - 1], &mut result);
            } else if trimmed.starts_with("@@write_file(") && trimmed.ends_with(')') {
                let path_arg = &trimmed[13..trimmed.len() - 1];
                let body_start = i + 1;
                let (content, next_index) = Self::collect_body_lines(&lines, body_start);
                Self::handle_write(
                    root,
                    path_arg,
                    &content,
                    next_index - body_start,
                    &mut result,
                );
                // 正文已被消费，从下一个命令继续。
                i = next_index;
                continue;
            } else if trimmed.starts_with("@@create_file(") && trimmed.ends_with(')') {
                Self::handle_create(root, &trimmed[14..trimmed.len() - 1], &mut result);
            } else if trimmed.starts_with("@@delete_file(") && trimmed.ends_with(')') {
                Self::handle_delete(root, &trimmed[14..trimmed.len() - 1], &mut result);
            } else if trimmed.starts_with("@@rename_file(") && trimmed.ends_with(')') {
                Self::handle_rename(root, &trimmed[14..trimmed.len() - 1], &mut result);
            }

            i += 1;
        }

        result
    }
}

#[async_trait]
impl WorkerAgent for CodeEditorWorker {
    fn kind(&self) -> WorkerKind {
        WorkerKind::CodeEditor
    }

    fn description(&self) -> &str {
        "Edits files using read, write, create, delete, and rename operations."
    }

    async fn execute(&self, task: SubTask) -> Result<WorkerResult> {
        let start = std::time::Instant::now();
        let root = self.workspace_root.lock().await;
        let root_path = Self::resolve_root(&root);

        // 先执行指令里内嵌的 @@ 命令，再把结果作为上下文交给模型。
        let tool_results = Self::execute_tools(&task.instruction, root_path).await;

        let system_prompt = format!(
            "{}\n\nCurrent workspace root: {}\n\n## Tool Results\n{}",
            EDITOR_SYSTEM_PROMPT,
            root_path.display(),
            if tool_results.is_empty() {
                "No tool commands found in instruction.".to_string()
            } else {
                tool_results
            }
        );

        let content = {
            let registry = self.providers.lock().await;
            let provider = registry.resolve(task.model_id.as_deref())?;
            provider.prompt(&system_prompt, &task.instruction).await?
        };

        drop(root);

        Ok(WorkerResult {
            worker: WorkerKind::CodeEditor,
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
    fn test_code_editor_kind() {
        let providers = Arc::new(Mutex::new(
            crate::llm::provider::ProviderRegistry::new(&[]),
        ));
        let worker = CodeEditorWorker::new(providers);
        assert_eq!(worker.kind(), WorkerKind::CodeEditor);
    }

    #[tokio::test]
    async fn test_execute_tools_read_file() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("test.txt"), "hello world").unwrap();
        let instruction = format!(
            "@@read_file(\"{}\")",
            dir.path().join("test.txt").display()
        );
        let result = CodeEditorWorker::execute_tools(&instruction, dir.path()).await;
        assert!(result.contains("hello world"));
    }

    #[tokio::test]
    async fn test_execute_tools_write_file() {
        let dir = TempDir::new().unwrap();
        let instruction = format!(
            "@@write_file(\"{}\")",
            dir.path().join("output.txt").display()
        );
        let result = CodeEditorWorker::execute_tools(&instruction, dir.path()).await;
        assert!(result.contains("wrote") || result.contains("Wrote"));
    }

    #[tokio::test]
    async fn test_execute_tools_write_file_with_content() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("content.txt");
        let instruction = format!(
            "@@write_file(\"{}\")\nline1\nline2\nline3",
            file_path.display()
        );
        let result = CodeEditorWorker::execute_tools(&instruction, dir.path()).await;
        assert!(result.contains("Wrote"));
        assert!(result.contains("3 lines"));
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "line1\nline2\nline3");
    }

    #[tokio::test]
    async fn test_create_file() {
        let dir = TempDir::new().unwrap();
        let instruction = format!(
            "@@create_file(\"{}\")",
            dir.path().join("new.txt").display()
        );
        let result = CodeEditorWorker::execute_tools(&instruction, dir.path()).await;
        assert!(result.contains("Created file"));
        assert!(dir.path().join("new.txt").exists());
    }

    #[tokio::test]
    async fn test_delete_file() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("delete_me.txt");
        fs::write(&file_path, "bye").unwrap();
        assert!(file_path.exists());

        let instruction =
            format!("@@delete_file(\"{}\")", file_path.display());
        let result = CodeEditorWorker::execute_tools(&instruction, dir.path()).await;
        assert!(result.contains("Deleted file"));
        assert!(!file_path.exists());
    }

    #[tokio::test]
    async fn test_rename_file() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("old.txt"), "content").unwrap();

        let instruction = format!(
            "@@rename_file(\"{}\", \"{}\")",
            dir.path().join("old.txt").display(),
            dir.path().join("new.txt").display()
        );
        let result = CodeEditorWorker::execute_tools(&instruction, dir.path()).await;
        assert!(result.contains("Renamed"));
        assert!(!dir.path().join("old.txt").exists());
        assert!(dir.path().join("new.txt").exists());
    }

    #[tokio::test]
    async fn test_execute_tools_no_commands() {
        let instruction = "Just a plain instruction without tool commands.";
        let dir = TempDir::new().unwrap();
        let result = CodeEditorWorker::execute_tools(instruction, dir.path()).await;
        assert!(result.is_empty(), "should return empty when no @@ commands");
    }

    #[tokio::test]
    async fn test_read_file_not_found() {
        let dir = TempDir::new().unwrap();
        let instruction = format!(
            "@@read_file(\"{}\")",
            dir.path().join("nonexistent.txt").display()
        );
        let result = CodeEditorWorker::execute_tools(&instruction, dir.path()).await;
        assert!(result.contains("Read error"));
    }

    #[tokio::test]
    async fn test_write_file_creates_parent_dirs() {
        let dir = TempDir::new().unwrap();
        let nested_path = dir.path().join("a").join("b").join("nested.txt");
        let instruction = format!(
            "@@write_file(\"{}\")\nhello nested",
            nested_path.display()
        );
        let result = CodeEditorWorker::execute_tools(&instruction, dir.path()).await;
        assert!(result.contains("Wrote"));
        assert!(nested_path.exists());
        assert_eq!(fs::read_to_string(&nested_path).unwrap(), "hello nested");
    }
}
