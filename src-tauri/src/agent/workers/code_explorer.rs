//! 代码探索 worker：用 grep / glob / 读文件 / 目录树理解代码库。
//!
//! 命令语法：
//! - `@@search_code(pattern[, path])`
//! - `@@search_files(glob)`
//! - `@@read_file(path)`
//! - `@@tree_view(depth)`
//!
//! 搜索优先使用 ripgrep；不可用时由 `SearchEngine` 自行降级。

use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::llm::provider::ProviderRegistry;
use crate::core::error::Result;
use crate::agent::workers::{SubTask, WorkerAgent, WorkerKind, WorkerResult};
use crate::features::workspace::search::SearchEngine;

/// 单次搜索最多展示的匹配条数。
const SEARCH_RESULT_LIMIT: usize = 50;
/// 读文件时展示的最大行数。
const READ_PREVIEW_LINES: usize = 100;
/// `@@tree_view` 未指定深度时的默认值。
const DEFAULT_TREE_DEPTH: usize = 2;

const EXPLORER_SYSTEM_PROMPT: &str = r#"You are a code exploration agent. Your job is to understand codebases.

Using the search results below, analyze the code and provide:
1. Summary of relevant files and their roles
2. Key patterns, types, and functions found
3. Dependencies and relationships between components
4. Any issues or areas of concern

Base your analysis on the actual code search results, not assumptions."#;

/// 代码探索 worker。
pub struct CodeExplorerWorker {
    providers: Arc<Mutex<ProviderRegistry>>,
    workspace_root: Arc<Mutex<Option<String>>>,
}

impl CodeExplorerWorker {
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

    /// 处理 `@@search_code(pattern[, path])`：按内容搜索并逐条输出。
    fn handle_search_code(root: &Path, args: &str, out: &mut String) {
        let (pattern, search_path) = if let Some(comma) = args.find(',') {
            (
                args[..comma].trim().trim_matches('"'),
                args[comma + 1..].trim().trim_matches('"'),
            )
        } else {
            (args.trim().trim_matches('"'), "")
        };
        let search_root = if search_path.is_empty() {
            root
        } else {
            Path::new(search_path)
        };

        match SearchEngine::grep(pattern, search_root, None) {
            Ok(matches) => {
                for m in matches.iter().take(SEARCH_RESULT_LIMIT) {
                    out.push_str(&format!("{}:{}: {}\n", m.file, m.line, m.content));
                }
                if matches.is_empty() {
                    out.push_str(&format!("(no matches for '{}')\n", pattern));
                }
            }
            Err(e) => out.push_str(&format!("Search error: {}\n", e)),
        }
    }

    /// 处理 `@@search_files(glob)`：按文件名模式匹配。
    fn handle_search_files(root: &Path, arg: &str, out: &mut String) {
        let pattern = arg.trim().trim_matches('"');
        match SearchEngine::glob(pattern, root) {
            Ok(files) => {
                for f in &files {
                    out.push_str(&format!("{}\n", f.display()));
                }
                if files.is_empty() {
                    out.push_str(&format!("(no files matching '{}')\n", pattern));
                }
            }
            Err(e) => out.push_str(&format!("Glob error: {}\n", e)),
        }
    }

    /// 处理 `@@read_file(path)`：输出带行号的前 100 行预览。
    fn handle_read_file(root: &Path, arg: &str, out: &mut String) {
        let path_str = arg.trim().trim_matches('"');
        let file_path = if Path::new(path_str).is_absolute() {
            std::path::PathBuf::from(path_str)
        } else {
            root.join(path_str)
        };

        match std::fs::read_to_string(&file_path) {
            Ok(content) => {
                let lines: Vec<&str> = content.lines().collect();
                let preview: String = lines
                    .iter()
                    .take(READ_PREVIEW_LINES)
                    .enumerate()
                    .map(|(i, l)| format!("{:>4}: {}", i + 1, l))
                    .collect::<Vec<_>>()
                    .join("\n");
                if lines.len() > READ_PREVIEW_LINES {
                    out.push_str(&format!(
                        "--- {} ({} lines, showing first {}) ---\n{}\n",
                        file_path.display(),
                        lines.len(),
                        READ_PREVIEW_LINES,
                        preview
                    ));
                } else {
                    out.push_str(&format!(
                        "--- {} ({} lines) ---\n{}\n",
                        file_path.display(),
                        lines.len(),
                        preview
                    ));
                }
            }
            Err(e) => out.push_str(&format!("Read error: {}\n", e)),
        }
    }

    /// 处理 `@@tree_view(depth)`：按层级输出目录树。
    fn handle_tree_view(root: &Path, arg: &str, out: &mut String) {
        let depth: usize = arg
            .trim()
            .parse()
            .unwrap_or(DEFAULT_TREE_DEPTH);

        match SearchEngine::tree(root, depth) {
            Ok(entries) => {
                for e in &entries {
                    let indent = "  ".repeat(e.depth);
                    let name = e.path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
                    let suffix = if e.is_dir { "/" } else { "" };
                    out.push_str(&format!("{}{}{}\n", indent, name, suffix));
                }
            }
            Err(e) => out.push_str(&format!("Tree error: {}\n", e)),
        }
    }

    /// 解析并执行指令中内嵌的 `@@command`，返回可读的搜索结果文本。
    async fn execute_tools(instruction: &str, root: &Path) -> String {
        let mut result = String::new();

        for line in instruction.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with("@@search_code(") && trimmed.ends_with(')') {
                Self::handle_search_code(root, &trimmed[14..trimmed.len() - 1], &mut result);
            } else if trimmed.starts_with("@@search_files(") && trimmed.ends_with(')') {
                Self::handle_search_files(root, &trimmed[15..trimmed.len() - 1], &mut result);
            } else if trimmed.starts_with("@@read_file(") && trimmed.ends_with(')') {
                Self::handle_read_file(root, &trimmed[12..trimmed.len() - 1], &mut result);
            } else if trimmed.starts_with("@@tree_view(") && trimmed.ends_with(')') {
                Self::handle_tree_view(root, &trimmed[12..trimmed.len() - 1], &mut result);
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

        // 先执行指令里内嵌的 @@ 命令，再把搜索结果作为上下文交给模型。
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

        let content = {
            let registry = self.providers.lock().await;
            let provider = registry.resolve(task.model_id.as_deref())?;
            provider.prompt(&system_prompt, &task.instruction).await?
        };

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
            crate::llm::provider::ProviderRegistry::new(&[]),
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
