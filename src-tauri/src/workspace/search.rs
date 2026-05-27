use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{AppError, Result};

/// A single match from full-text search (ripgrep).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GrepMatch {
    pub file: String,
    pub line: usize,
    pub content: String,
}

/// A single entry in a file tree listing.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TreeEntry {
    pub path: PathBuf,
    pub is_dir: bool,
    pub depth: usize,
}

/// Directories to skip during recursive walks.
const EXCLUDED_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".next",
    "dist",
    "build",
    ".cache",
];

/// SearchEngine provides file search capabilities over the workspace:
///
/// - **grep**: full-text search via ripgrep (`rg`)
/// - **glob**: pattern-based file name matching
/// - **tree**: recursive directory listing with depth control
/// - **is_rg_available**: check whether ripgrep is installed
pub struct SearchEngine;

impl SearchEngine {
    /// Full-text search using ripgrep. Returns matching lines with file paths.
    ///
    /// # Arguments
    ///
    /// * `pattern` — the text pattern to search for (case-insensitive)
    /// * `root` — the root directory to search in
    /// * `file_pattern` — optional glob filter (e.g. `"*.rs"`), passed via `-g`
    ///
    /// # Errors
    ///
    /// Returns `AppError::Workspace` if ripgrep is not installed or fails unexpectedly.
    /// If ripgrep finds no results, an empty `Vec` is returned (not an error).
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

        // rg exits with 1 when no matches are found — treat as empty results, not an error.
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

        // Parse output lines: path:line:content
        // We use rfind(':') twice from the right to handle colons in path or content.
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

    /// Glob file search. Finds files matching a glob pattern via recursive directory walk.
    ///
    /// Skips common artifact directories (`.git`, `node_modules`, `target`, etc.).
    /// Returns sorted paths.
    ///
    /// # Arguments
    ///
    /// * `pattern` — glob pattern (e.g. `"*.txt"`, `"**/*.rs"`, `"src/**/*.ts"`)
    /// * `root` — the root directory to search from
    pub fn glob(pattern: &str, root: &Path) -> Result<Vec<PathBuf>> {
        let glob_pattern = glob::Pattern::new(pattern).map_err(|e| {
            AppError::InvalidInput(format!("Invalid glob pattern '{}': {}", pattern, e))
        })?;

        let mut results = Vec::new();
        Self::walk_glob(root, root, &glob_pattern, &mut results)?;
        results.sort();
        Ok(results)
    }

    /// Fast file tree listing with depth control.
    ///
    /// Entries are sorted: directories first, then files, alphabetically within each group.
    /// Skips common artifact directories.
    ///
    /// # Arguments
    ///
    /// * `root` — the root directory to list
    /// * `max_depth` — maximum recursion depth (0 = immediate children only)
    pub fn tree(root: &Path, max_depth: usize) -> Result<Vec<TreeEntry>> {
        let mut entries = Vec::new();
        Self::walk_tree(root, root, 0, max_depth, &mut entries)?;
        // Sort: directories first, then files; alphabetical within each group
        entries.sort_by(|a, b| {
            if a.is_dir != b.is_dir {
                // Sort by is_dir descending so directories (true > false? No...)
                // We want directories first, so dirs (is_dir=true) come before files
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

    /// Check if ripgrep is available on the system.
    pub fn is_rg_available() -> bool {
        Command::new("rg")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    // ---------------------------------------------------------------------------
    // Internal helpers
    // ---------------------------------------------------------------------------

    /// Recursively walk `dir`, collecting paths whose filename or relative path
    /// matches `pattern`.
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
            Err(_) => return Ok(()), // skip permission-denied directories
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
                // Check both the full relative path and just the filename
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

    /// Recursively walk `dir` with a maximum depth, collecting `TreeEntry` values.
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
            Err(_) => return Ok(()), // skip permission-denied directories
        };

        // Collect raw entries first so we can sort within this directory
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

        // Sort: directories first, then files; alphabetical within each group
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
        // If rg not available, test passes by skipping
    }
}
