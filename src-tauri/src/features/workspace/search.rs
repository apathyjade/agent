use std::path::{Path, PathBuf};
use std::process::Command;

use crate::core::error::{AppError, Result};

/// 全文搜索（ripgrep）返回的一条命中。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GrepMatch {
    pub file: String,
    pub line: usize,
    pub content: String,
}

/// 文件树列表中的一项。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TreeEntry {
    pub path: PathBuf,
    pub is_dir: bool,
    pub depth: usize,
}

/// 递归遍历时需要跳过的目录。
const EXCLUDED_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".next",
    "dist",
    "build",
    ".cache",
];

/// 提供工作区文件搜索能力：
///
/// - **grep**：通过 ripgrep（`rg`）做全文搜索
/// - **glob**：按模式匹配文件名
/// - **tree**：带深度控制的递归目录列表
/// - **is_rg_available**：检查是否安装了 ripgrep
pub struct SearchEngine;

impl SearchEngine {
    /// 使用 ripgrep 做全文搜索，返回命中的行与文件路径。
    ///
    /// # 参数
    ///
    /// * `pattern` — 要搜索的文本（忽略大小写）
    /// * `root` — 搜索根目录
    /// * `file_pattern` — 可选的 glob 过滤（如 `"*.rs"`），通过 `-g` 传入
    ///
    /// # 错误
    ///
    /// ripgrep 未安装或异常失败时返回 `AppError::Workspace`。
    /// 没有命中时返回空 `Vec`，不算错误。
    pub fn grep(pattern: &str, root: &Path, file_pattern: Option<&str>) -> Result<Vec<GrepMatch>> {
        let mut cmd = Command::new("rg");
        cmd.arg("--line-number")
            .arg("--with-filename")
            .arg("--color")
            .arg("never")
            .arg("-i")
            .arg(pattern)
            .arg(root.to_string_lossy().as_ref());

        if let Some(fp) = file_pattern {
            cmd.arg("-g").arg(fp);
        }

        let output = cmd.output().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                AppError::Workspace(
                    "ripgrep (rg) is not installed. Please install it to use full-text search."
                        .to_string(),
                )
            } else {
                AppError::Workspace(format!("Failed to run ripgrep: {}", e))
            }
        })?;

        // rg 无命中时退出码为 1 —— 视为空结果而非错误。
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.trim().is_empty()
                || stderr.to_lowercase().contains("no results found")
                || stderr.to_lowercase().contains("error searching")
            {
                return Ok(Vec::new());
            }
            return Err(AppError::Workspace(format!(
                "ripgrep failed: {}",
                stderr.trim()
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut matches = Vec::new();

        // 解析输出行：path:line:content
        // 用正则从左侧锚定，路径或内容里的冒号不会干扰分组。
        let re = regex::Regex::new(r"^(.+?):(\d+):(.*)$")
            .map_err(|e| AppError::Workspace(format!("Regex compilation error: {}", e)))?;

        for line in stdout.lines() {
            if line.is_empty() {
                continue;
            }
            if let Some(caps) = re.captures(line) {
                let file = caps.get(1).unwrap().as_str().to_string();
                let line_num = caps.get(2).unwrap().as_str().parse::<usize>().unwrap_or(0);
                let content = caps.get(3).unwrap().as_str().to_string();
                matches.push(GrepMatch {
                    file,
                    line: line_num,
                    content,
                });
            }
        }

        Ok(matches)
    }

    /// glob 文件搜索：递归遍历目录，找出匹配模式的文件。
    ///
    /// 跳过常见产物目录（`.git`、`node_modules`、`target` 等），返回排序后
    /// 的路径。
    ///
    /// # 参数
    ///
    /// * `pattern` — glob 模式（如 `"*.txt"`、`"**/*.rs"`、`"src/**/*.ts"`）
    /// * `root` — 搜索根目录
    pub fn glob(pattern: &str, root: &Path) -> Result<Vec<PathBuf>> {
        let glob_pattern = glob::Pattern::new(pattern).map_err(|e| {
            AppError::InvalidInput(format!("Invalid glob pattern '{}': {}", pattern, e))
        })?;

        let mut results = Vec::new();
        Self::walk_glob(root, root, &glob_pattern, &mut results)?;
        results.sort();
        Ok(results)
    }

    /// 带深度控制的快速文件树列表。
    ///
    /// 排序规则：目录在前、文件在后，组内按字母序；跳过常见产物目录。
    ///
    /// # 参数
    ///
    /// * `root` — 要列出的根目录
    /// * `max_depth` — 最大递归深度（0 表示只列直接子项）
    pub fn tree(root: &Path, max_depth: usize) -> Result<Vec<TreeEntry>> {
        let mut entries = Vec::new();
        Self::walk_tree(root, root, 0, max_depth, &mut entries)?;
        // 排序：目录在前、文件在后，组内按字母序
        entries.sort_by(|a, b| {
            if a.is_dir != b.is_dir {
                // 目录（is_dir=true）排在文件之前
                if a.is_dir {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                }
            } else {
                a.path.cmp(&b.path)
            }
        });
        Ok(entries)
    }

    /// 检查系统上是否可用 ripgrep。
    pub fn is_rg_available() -> bool {
        Command::new("rg")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    // ---------------------------------------------------------------------------
    // 内部辅助函数
    // ---------------------------------------------------------------------------

    /// 递归遍历 `dir`，收集文件名或相对路径匹配 `pattern` 的路径。
    fn walk_glob(
        root: &Path,
        dir: &Path,
        pattern: &glob::Pattern,
        results: &mut Vec<PathBuf>,
    ) -> Result<()> {
        if !dir.is_dir() {
            return Ok(());
        }

        let read_dir = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(_) => return Ok(()), // 跳过无权限的目录
        };

        for entry in read_dir {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let path = entry.path();

            if path.is_dir() {
                let dir_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                if EXCLUDED_DIRS.contains(&dir_name) {
                    continue;
                }
                Self::walk_glob(root, &path, pattern, results)?;
            } else if path.is_file() {
                // 同时检查完整相对路径与纯文件名
                if let Ok(relative) = path.strip_prefix(root) {
                    if pattern.matches_path(relative) {
                        results.push(path);
                    } else if let Some(fname) = path.file_name().and_then(|n| n.to_str()) {
                        if pattern.matches(fname) {
                            results.push(path);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// 带最大深度递归遍历 `dir`，收集 [`TreeEntry`]。
    fn walk_tree(
        root: &Path,
        dir: &Path,
        depth: usize,
        max_depth: usize,
        entries: &mut Vec<TreeEntry>,
    ) -> Result<()> {
        if depth > max_depth || !dir.is_dir() {
            return Ok(());
        }

        let read_dir = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(_) => return Ok(()), // 跳过无权限的目录
        };

        // 先收集原始条目，便于在当前目录内排序
        let mut local_entries: Vec<(PathBuf, bool)> = Vec::new();

        for entry in read_dir {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let path = entry.path();
            let is_dir = path.is_dir();

            if is_dir {
                let dir_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                if EXCLUDED_DIRS.contains(&dir_name) {
                    continue;
                }
            }

            local_entries.push((path, is_dir));
        }

        // 排序：目录在前、文件在后，组内按字母序
        local_entries.sort_by(|(path_a, is_dir_a), (path_b, is_dir_b)| {
            if is_dir_a != is_dir_b {
                if *is_dir_a {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                }
            } else {
                path_a.cmp(path_b)
            }
        });

        for (path, is_dir) in &local_entries {
            entries.push(TreeEntry {
                path: path.clone(),
                is_dir: *is_dir,
                depth,
            });

            if *is_dir {
                Self::walk_tree(root, path, depth + 1, max_depth, entries)?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn create_test_dir() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        fs::write(dir.path().join("hello.txt"), "hello world\nfoo bar").unwrap();
        fs::write(dir.path().join("test.rs"), "fn test() {}\n// hello").unwrap();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/nested.txt"), "nested content").unwrap();
        dir
    }

    #[test]
    fn test_glob_finds_files() {
        let dir = create_test_dir();
        let results = SearchEngine::glob("*.txt", dir.path()).unwrap();
        assert!(!results.is_empty());
        assert!(results.iter().any(|p| p.ends_with("hello.txt")));
    }

    #[test]
    fn test_tree_entries() {
        let dir = create_test_dir();
        let entries = SearchEngine::tree(dir.path(), 2).unwrap();
        assert!(!entries.is_empty());
        assert!(entries.iter().any(|e| e.path.ends_with("hello.txt")));
        assert!(entries.iter().any(|e| e.path.ends_with("sub")));
    }

    #[test]
    fn test_glob_nonexistent_pattern() {
        let dir = create_test_dir();
        let results = SearchEngine::glob("*.nonexistent", dir.path()).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_grep_if_rg_available() {
        if SearchEngine::is_rg_available() {
            let dir = create_test_dir();
            let results = SearchEngine::grep("hello", dir.path(), None).unwrap();
            assert!(!results.is_empty());
            assert!(results.iter().any(|m| m.content.contains("hello")));
        }
        // 未安装 rg 时跳过该用例
    }
}
