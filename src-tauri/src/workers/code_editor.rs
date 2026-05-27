use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::api::provider::ProviderRegistry;
use crate::api::types::{ChatRequest, Message, MessageRole};
use crate::error::Result;
use crate::workers::{SubTask, WorkerAgent, WorkerKind, WorkerResult};

const EDITOR_SYSTEM_PROMPT: &str = r#"You are a code editing agent. Your job is to modify files in a codebase.

You have access to the following tools, which are executed automatically when you include them in your response:

- @@read_file(path) — Read the first 200 lines of a file
- @@write_file(path) — Write content to a file (content follows on subsequent lines)
- @@create_file(path) — Create a new empty file
- @@delete_file(path) — Delete a file or directory
- @@rename_file(from, to) — Rename or move a file

Commands are embedded in your instructions and will be executed automatically. Provide the file changes needed based on the task description."#;

pub struct CodeEditorWorker {
    providers: Arc<Mutex<ProviderRegistry>>,
    workspace_root: Arc<Mutex<Option<String>>>,
}

impl CodeEditorWorker {
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

    /// Parse and execute @@ tool commands embedded in the instruction for file editing.
    async fn execute_tools(instruction: &str, root: &Path) -> String {
        let mut result = String::new();
        let lines: Vec<&str> = instruction.lines().collect();
        let mut i = 0;

        while i < lines.len() {
            let trimmed = lines[i].trim();

            if trimmed.starts_with("@@read_file(") && trimmed.ends_with(')') {
                let path_str = &trimmed[12..trimmed.len() - 1].trim().trim_matches('"');
                let file_path = if Path::new(path_str).is_absolute() {
                    std::path::PathBuf::from(path_str)
                } else {
                    root.join(path_str)
                };
                match std::fs::read_to_string(&file_path) {
                    Ok(content) => {
                        let all_lines: Vec<&str> = content.lines().collect();
                        let preview: String = all_lines
                            .iter()
                            .take(200)
                            .enumerate()
                            .map(|(i, l)| format!("{:>4}: {}", i + 1, l))
                            .collect::<Vec<_>>()
                            .join("\n");
                        if all_lines.len() > 200 {
                            result.push_str(&format!(
                                "--- {} ({} lines, showing first 200) ---\n{}\n",
                                file_path.display(),
                                all_lines.len(),
                                preview
                            ));
                        } else {
                            result.push_str(&format!(
                                "--- {} ({} lines) ---\n{}\n",
                                file_path.display(),
                                all_lines.len(),
                                preview
                            ));
                        }
                    }
                    Err(e) => result.push_str(&format!("Read error: {}\n", e)),
                }
            } else if trimmed.starts_with("@@write_file(") && trimmed.ends_with(')') {
                let path_str = &trimmed[13..trimmed.len() - 1].trim().trim_matches('"');
                let file_path = if Path::new(path_str).is_absolute() {
                    std::path::PathBuf::from(path_str)
                } else {
                    root.join(path_str)
                };

                // Collect content from subsequent lines until next @@ command or end
                i += 1;
                let mut content_lines: Vec<&str> = Vec::new();
                while i < lines.len() {
                    let cl = lines[i];
                    if cl.trim().starts_with("@@") {
                        break;
                    }
                    content_lines.push(cl);
                    i += 1;
                }

                let content = content_lines.join("\n");

                // Create parent directories if needed
                if let Some(parent) = file_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }

                match std::fs::write(&file_path, &content) {
                    Ok(_) => {
                        if content.is_empty() {
                            result.push_str(&format!(
                                "Wrote {} (empty file)\n",
                                file_path.display()
                            ));
                        } else {
                            result.push_str(&format!(
                                "Wrote {} ({} bytes, {} lines)\n",
                                file_path.display(),
                                content.len(),
                                content_lines.len()
                            ));
                        }
                    }
                    Err(e) => result.push_str(&format!("Write error: {}\n", e)),
                }
                continue;
            } else if trimmed.starts_with("@@create_file(") && trimmed.ends_with(')') {
                let path_str = &trimmed[14..trimmed.len() - 1].trim().trim_matches('"');
                let file_path = if Path::new(path_str).is_absolute() {
                    std::path::PathBuf::from(path_str)
                } else {
                    root.join(path_str)
                };

                // Create parent directories if needed
                if let Some(parent) = file_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                match std::fs::File::create(&file_path) {
                    Ok(_) => {
                        result.push_str(&format!("Created file {}\n", file_path.display()));
                    }
                    Err(e) => result.push_str(&format!("Create error: {}\n", e)),
                }
            } else if trimmed.starts_with("@@delete_file(") && trimmed.ends_with(')') {
                let path_str = &trimmed[14..trimmed.len() - 1].trim().trim_matches('"');
                let file_path = if Path::new(path_str).is_absolute() {
                    std::path::PathBuf::from(path_str)
                } else {
                    root.join(path_str)
                };

                let path = file_path.as_path();
                if path.is_dir() {
                    match std::fs::remove_dir_all(path) {
                        Ok(_) => {
                            result.push_str(&format!("Deleted directory {}\n", path.display()));
                        }
                        Err(e) => result.push_str(&format!("Delete error: {}\n", e)),
                    }
                } else {
                    match std::fs::remove_file(path) {
                        Ok(_) => {
                            result.push_str(&format!("Deleted file {}\n", path.display()));
                        }
                        Err(e) => result.push_str(&format!("Delete error: {}\n", e)),
                    }
                }
            } else if trimmed.starts_with("@@rename_file(") && trimmed.ends_with(')') {
                let args_str = &trimmed[14..trimmed.len() - 1];
                let (from_str, to_str) = if let Some(comma) = args_str.find(',') {
                    (
                        args_str[..comma].trim().trim_matches('"'),
                        args_str[comma + 1..].trim().trim_matches('"'),
                    )
                } else {
                    result.push_str("Rename error: expected two arguments (from, to)\n");
                    i += 1;
                    continue;
                };

                let from_path = if Path::new(from_str).is_absolute() {
                    std::path::PathBuf::from(from_str)
                } else {
                    root.join(from_str)
                };
                let to_path = if Path::new(to_str).is_absolute() {
                    std::path::PathBuf::from(to_str)
                } else {
                    root.join(to_str)
                };

                // Create parent directories for destination if needed
                if let Some(parent) = to_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }

                match std::fs::rename(&from_path, &to_path) {
                    Ok(_) => {
                        result.push_str(&format!(
                            "Renamed {} to {}\n",
                            from_path.display(),
                            to_path.display()
                        ));
                    }
                    Err(e) => result.push_str(&format!("Rename error: {}\n", e)),
                }
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

        // Execute any @@tool commands embedded in the instruction
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

        let provider = {
            let registry = self.providers.lock().await;
            let mid = task
                .model_id
                .as_deref()
                .unwrap_or_else(|| registry.default_model_id());
            if mid.is_empty() {
                drop(root);
                return Err(crate::error::AppError::Worker(
                    "No model configured for CodeEditorWorker".into(),
                ));
            }
            registry.get(mid)?
        };

        let request = ChatRequest {
            messages: vec![
                Message {
                    id: None,
                    role: MessageRole::System,
                    content: system_prompt,
                    tool_calls: None,
                    tool_call_id: None,
                },
                Message {
                    id: None,
                    role: MessageRole::User,
                    content: task.instruction.clone(),
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            model: "".to_string(),
            tools: None,
            stream: Some(false),
            max_tokens: task.max_tokens.map(|t| t as usize),
            temperature: task.temperature,
        };

        let response = provider.chat(request).await?;
        let content = response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

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
            crate::api::provider::ProviderRegistry::new(
                &crate::config::AppConfig::default(),
            ),
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
