use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::api::provider::{ProviderRegistry, chat_text};
use crate::error::Result;
use crate::workers::{SubTask, WorkerAgent, WorkerKind, WorkerResult};
use crate::workspace::search::SearchEngine;

const EXPLORER_SYSTEM_PROMPT: &str = r#"You are a code exploration agent. Your job is to understand codebases.

Using the search results below, analyze the code and provide:
1. Summary of relevant files and their roles
2. Key patterns, types, and functions found
3. Dependencies and relationships between components
4. Any issues or areas of concern

Base your analysis on the actual code search results, not assumptions."#;

pub struct CodeExplorerWorker {
    providers: Arc<Mutex<ProviderRegistry>>,
    workspace_root: Arc<Mutex<Option<String>>>,
}

impl CodeExplorerWorker {
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

    /// Parse and execute tool commands embedded in the instruction using @@tool_name(args) syntax.
    async fn execute_tools(instruction: &str, root: &Path) -> String {
        let mut result = String::new();

        for line in instruction.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with("@@search_code(") && trimmed.ends_with(')') {
                let args = &trimmed[14..trimmed.len() - 1];
                let (pattern, search_path) = if let Some(comma) = args.find(',') {
                    (args[..comma].trim().trim_matches('"'),
                     args[comma + 1..].trim().trim_matches('"'))
                } else {
                    (args.trim().trim_matches('"'), "")
                };
                let search_root = if search_path.is_empty() { root } else { Path::new(search_path) };
                match SearchEngine::grep(pattern, search_root, None) {
                    Ok(matches) => {
                        for m in matches.iter().take(50) {
                            result.push_str(&format!("{}:{}: {}\n", m.file, m.line, m.content));
                        }
                        if matches.is_empty() {
                            result.push_str(&format!("(no matches for '{}')\n", pattern));
                        }
                    }
                    Err(e) => result.push_str(&format!("Search error: {}\n", e)),
                }
            } else if trimmed.starts_with("@@search_files(") && trimmed.ends_with(')') {
                let pattern = &trimmed[15..trimmed.len() - 1].trim().trim_matches('"');
                match SearchEngine::glob(pattern, root) {
                    Ok(files) => {
                        for f in &files {
                            result.push_str(&format!("{}\n", f.display()));
                        }
                        if files.is_empty() {
                            result.push_str(&format!("(no files matching '{}')\n", pattern));
                        }
                    }
                    Err(e) => result.push_str(&format!("Glob error: {}\n", e)),
                }
            } else if trimmed.starts_with("@@read_file(") && trimmed.ends_with(')') {
                let path_str = &trimmed[12..trimmed.len() - 1].trim().trim_matches('"');
                let file_path = if Path::new(path_str).is_absolute() {
                    std::path::PathBuf::from(path_str)
                } else {
                    root.join(path_str)
                };
                match std::fs::read_to_string(&file_path) {
                    Ok(content) => {
                        let lines: Vec<&str> = content.lines().collect();
                        let preview: String = lines.iter()
                            .take(100)
                            .enumerate()
                            .map(|(i, l)| format!("{:>4}: {}", i + 1, l))
                            .collect::<Vec<_>>()
                            .join("\n");
                        if lines.len() > 100 {
                            result.push_str(&format!(
                                "--- {} ({} lines, showing first 100) ---\n{}\n",
                                file_path.display(), lines.len(), preview
                            ));
                        } else {
                            result.push_str(&format!(
                                "--- {} ({} lines) ---\n{}\n",
                                file_path.display(), lines.len(), preview
                            ));
                        }
                    }
                    Err(e) => result.push_str(&format!("Read error: {}\n", e)),
                }
            } else if trimmed.starts_with("@@tree_view(") && trimmed.ends_with(')') {
                let depth_str = &trimmed[12..trimmed.len() - 1].trim();
                let depth: usize = depth_str.parse().unwrap_or(2);
                match SearchEngine::tree(root, depth) {
                    Ok(entries) => {
                        for e in &entries {
                            let indent = "  ".repeat(e.depth);
                            let name = e.path.file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("?");
                            let suffix = if e.is_dir { "/" } else { "" };
                            result.push_str(&format!("{}{}{}\n", indent, name, suffix));
                        }
                    }
                    Err(e) => result.push_str(&format!("Tree error: {}\n", e)),
                }
            }
        }

        result
    }
}

#[async_trait]
impl WorkerAgent for CodeExplorerWorker {
    fn kind(&self) -> WorkerKind {
        WorkerKind::CodeExplorer
    }

    fn description(&self) -> &str {
        "Explores codebases using grep, glob, file reads, and directory tree views."
    }

    async fn execute(&self, task: SubTask) -> Result<WorkerResult> {
        let start = std::time::Instant::now();
        let root = self.workspace_root.lock().await;
        let root_path = Self::resolve_root(&root);

        // Execute any @@tool commands embedded in the instruction
        let tool_results = Self::execute_tools(&task.instruction, root_path).await;

        let system_prompt = format!(
            "{}\n\nCurrent workspace root: {}\n\n## Code Search Results\n{}",
            EXPLORER_SYSTEM_PROMPT,
            root_path.display(),
            if tool_results.is_empty() {
                "No search results — no @@tool commands found in instruction.".to_string()
            } else {
                tool_results
            }
        );

        let content = chat_text(
            &self.providers,
            task.model_id.as_deref(),
            &system_prompt,
            &task.instruction,
            task.max_tokens.map(|t| t as usize),
            task.temperature,
        ).await?;

        drop(root);

        Ok(WorkerResult {
            worker: WorkerKind::CodeExplorer,
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
    fn test_explorer_kind() {
        let providers = Arc::new(Mutex::new(
            crate::api::provider::ProviderRegistry::new(
                &crate::config::AppConfig::default(),
            ),
        ));
        let worker = CodeExplorerWorker::new(providers);
        assert_eq!(worker.kind(), WorkerKind::CodeExplorer);
    }

    #[tokio::test]
    async fn test_execute_tools_search_files() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("test.rs"), "fn main() {}").unwrap();
        fs::write(dir.path().join("lib.rs"), "pub fn helper() {}").unwrap();

        let instruction = "@@search_files(\"*.rs\")\n";
        let result = CodeExplorerWorker::execute_tools(instruction, dir.path()).await;
        assert!(result.contains("test.rs"), "result should contain test.rs");
        assert!(result.contains("lib.rs"), "result should contain lib.rs");
    }

    #[tokio::test]
    async fn test_execute_tools_read_file() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("hello.txt"), "hello world").unwrap();

        let hello_path = dir.path().join("hello.txt");
        let instruction = format!("@@read_file(\"{}\")", hello_path.display());
        let result = CodeExplorerWorker::execute_tools(&instruction, dir.path()).await;
        assert!(result.contains("hello world"), "result should contain file content");
    }

    #[tokio::test]
    async fn test_execute_tools_tree() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.txt"), "").unwrap();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/b.txt"), "").unwrap();

        let result = CodeExplorerWorker::execute_tools("@@tree_view(3)", dir.path()).await;
        assert!(result.contains("a.txt"), "result should contain a.txt");
        assert!(result.contains("sub"), "result should contain sub directory");
    }

    #[tokio::test]
    async fn test_execute_tools_no_commands() {
        let instruction = "Just a plain instruction without tool commands.";
        let dir = TempDir::new().unwrap();
        let result = CodeExplorerWorker::execute_tools(instruction, dir.path()).await;
        assert!(result.is_empty(), "should return empty when no @@ commands");
    }

    #[tokio::test]
    async fn test_execute_tools_search_code() {
        if !SearchEngine::is_rg_available() {
            return;
        }
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("test.rs"), "fn hello() {}\nfn world() {}").unwrap();

        let instruction = "@@search_code(\"hello\")\n";
        let result = CodeExplorerWorker::execute_tools(instruction, dir.path()).await;
        assert!(result.contains("hello"), "result should contain matched pattern");
    }

    #[tokio::test]
    async fn test_execute_tools_search_code_no_match() {
        if !SearchEngine::is_rg_available() {
            return;
        }
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("test.rs"), "fn foo() {}").unwrap();

        let instruction = "@@search_code(\"nonexistent\")\n";
        let result = CodeExplorerWorker::execute_tools(instruction, dir.path()).await;
        assert!(result.contains("no matches"), "should report no matches");
    }
}
