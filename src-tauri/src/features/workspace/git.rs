use std::path::Path;
use serde::{Deserialize, Serialize};
use crate::core::error::Result;

/// 工作区中单个文件的 git 状态。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitFileStatus {
    pub path: String,
    /// 取值："modified"、"added"、"deleted"、"renamed"、"untracked"、"typechange"
    pub status: String,
    /// 是否存在已暂存的改动
    pub staged: bool,
}

/// 一条提交记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitInfo {
    pub hash: String,
    pub author: String,
    pub timestamp: String,
    pub message: String,
}

/// blame 结果中的一行。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlameLine {
    pub line: usize,
    pub content: String,
    pub commit_hash: String,
    pub author: String,
    pub timestamp: String,
}

/// 基于 git2 的工作区 git 集成。
///
/// 打开路径不在仓库内时不报错，而是退化为「非仓库」状态，所有查询返回空
/// 结果，使调用方无需区分两种情况。
pub struct GitIntegration {
    repo: Option<git2::Repository>,
}

impl GitIntegration {
    /// 打开路径所属的仓库；找不到仓库时返回一个空的集成实例。
    pub fn open(path: &Path) -> Result<Self> {
        match git2::Repository::discover(path) {
            Ok(repo) => Ok(Self { repo: Some(repo) }),
            Err(_) => Ok(Self { repo: None }),
        }
    }

    /// 打开的路径是否位于 git 仓库内。
    pub fn is_repo(&self) -> bool {
        self.repo.is_some()
    }

    /// 列出工作区中所有有状态变化的文件。
    ///
    /// 同时包含暂存区与工作区改动；状态优先级为新增、修改、删除、重命名、
    /// 类型变化，其他状态（如忽略）跳过。
    pub fn status(&self) -> Result<Vec<GitFileStatus>> {
        let repo = match &self.repo {
            Some(r) => r,
            None => return Ok(Vec::new()),
        };

        let statuses = repo.statuses(None)
            .map_err(|e| crate::core::error::AppError::Workspace(format!("Git status error: {}", e)))?;

        let mut results = Vec::new();
        for entry in statuses.iter() {
            let flags = entry.status();
            let path = entry.path().unwrap_or("").to_string();

            let staged = flags.intersects(
                git2::Status::INDEX_NEW | git2::Status::INDEX_MODIFIED |
                git2::Status::INDEX_DELETED | git2::Status::INDEX_RENAMED |
                git2::Status::INDEX_TYPECHANGE
            );

            let status = if flags.contains(git2::Status::WT_NEW) || flags.contains(git2::Status::INDEX_NEW) {
                "added"
            } else if flags.contains(git2::Status::WT_MODIFIED) || flags.contains(git2::Status::INDEX_MODIFIED) {
                "modified"
            } else if flags.contains(git2::Status::WT_DELETED) || flags.contains(git2::Status::INDEX_DELETED) {
                "deleted"
            } else if flags.contains(git2::Status::WT_RENAMED) || flags.contains(git2::Status::INDEX_RENAMED) {
                "renamed"
            } else if flags.contains(git2::Status::WT_TYPECHANGE) || flags.contains(git2::Status::INDEX_TYPECHANGE) {
                "typechange"
            } else {
                continue;
            };

            results.push(GitFileStatus { path, status: status.to_string(), staged });
        }

        Ok(results)
    }

    /// 输出工作区相对暂存区的 diff，可按路径过滤。
    ///
    /// 返回带 `+` / `-` / 空格前缀的补丁文本；非仓库返回占位字符串。
    pub fn diff(&self, path: Option<&str>) -> Result<String> {
        let repo = match &self.repo {
            Some(r) => r,
            None => return Ok("(not a git repository)".to_string()),
        };

        let mut opts = git2::DiffOptions::new();
        if let Some(p) = path {
            opts.pathspec(p);
        }

        let diff = repo.diff_index_to_workdir(None, Some(&mut opts))
            .map_err(|e| crate::core::error::AppError::Workspace(format!("Git diff error: {}", e)))?;

        let mut result = String::new();
        diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
            let prefix = match line.origin() {
                '+' => "+",
                '-' => "-",
                ' ' => " ",
                _ => " ",
            };
            if let Ok(content) = std::str::from_utf8(line.content()) {
                // 丢弃原有的 +/-/空格 前缀，改用我们自己加的前缀
                let content = content.trim_end_matches('\n');
                result.push_str(&format!("{}{}\n", prefix, content));
            }
            true
        }).map_err(|e| crate::core::error::AppError::Workspace(format!("Git diff print error: {}", e)))?;

        Ok(result)
    }

    /// 按时间倒序返回最近 `n` 条提交。
    pub fn log(&self, n: usize) -> Result<Vec<CommitInfo>> {
        let repo = match &self.repo {
            Some(r) => r,
            None => return Ok(Vec::new()),
        };

        let mut revwalk = repo.revwalk()
            .map_err(|e| crate::core::error::AppError::Workspace(format!("Git revwalk error: {}", e)))?;
        revwalk.push_head()
            .map_err(|e| crate::core::error::AppError::Workspace(format!("Git push head error: {}", e)))?;
        revwalk.set_sorting(git2::Sort::TIME)
            .map_err(|e| crate::core::error::AppError::Workspace(format!("Git sort error: {}", e)))?;

        let mut commits = Vec::new();
        for (i, oid) in revwalk.enumerate() {
            if i >= n {
                break;
            }
            let oid = oid.map_err(|e| crate::core::error::AppError::Workspace(format!("Git oid error: {}", e)))?;
            let commit = repo.find_commit(oid)
                .map_err(|e| crate::core::error::AppError::Workspace(format!("Git find commit error: {}", e)))?;

            let hash = oid.to_string();
            let author = commit.author().name().unwrap_or("unknown").to_string();
            let timestamp = chrono::DateTime::from_timestamp(commit.time().seconds(), 0)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_else(|| "unknown".to_string());
            let message = commit.summary().unwrap_or("").to_string();

            commits.push(CommitInfo { hash, author, timestamp, message });
        }

        Ok(commits)
    }

    /// 对指定文件做逐行 blame。
    ///
    /// 未被任何提交覆盖的行（例如未跟踪内容）以全零 hash 与 `unknown`
    /// 作者占位。
    pub fn blame(&self, path: &str) -> Result<Vec<BlameLine>> {
        let repo = match &self.repo {
            Some(r) => r,
            None => return Ok(Vec::new()),
        };

        let repo_path = repo.path().parent().unwrap_or(Path::new("."));
        let full_path = repo_path.join(path);

        let blame = repo.blame_file(&full_path, None)
            .map_err(|e| crate::core::error::AppError::Workspace(format!("Git blame error: {}", e)))?;

        let content = std::fs::read_to_string(&full_path)
            .map_err(|e| crate::core::error::AppError::Workspace(format!("Read file error: {}", e)))?;

        let mut results = Vec::new();
        for (i, line_content) in content.lines().enumerate() {
            let line_num = i + 1;
            let hunk_count = blame.len();
            let mut found_hunk = None;

            // 找到该行所属的 hunk
            for hunk_idx in 0..hunk_count {
                if let Some(hunk) = blame.get_index(hunk_idx) {
                    let start = hunk.final_start_line();
                    let end = start + hunk.lines_in_hunk();
                    if line_num >= start && line_num < end {
                        found_hunk = Some(hunk);
                        break;
                    }
                }
            }

            if let Some(hunk) = found_hunk {
                let oid = hunk.final_commit_id();
                let hash = oid.to_string();
                let author = hunk.final_signature().name().unwrap_or("unknown").to_string();
                let timestamp = chrono::DateTime::from_timestamp(hunk.final_signature().when().seconds(), 0)
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_else(|| "unknown".to_string());

                results.push(BlameLine {
                    line: line_num,
                    content: line_content.to_string(),
                    commit_hash: hash,
                    author,
                    timestamp,
                });
            } else {
                results.push(BlameLine {
                    line: line_num,
                    content: line_content.to_string(),
                    commit_hash: "0000000".to_string(),
                    author: "unknown".to_string(),
                    timestamp: "unknown".to_string(),
                });
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn init_test_repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();

        // 配置测试用户
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "test").unwrap();
        config.set_str("user.email", "test@test.com").unwrap();

        // 创建初始提交
        fs::write(dir.path().join("README.md"), "# Test\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("README.md")).unwrap();
        index.write().unwrap(); // 把索引写回 .git/index
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let signature = git2::Signature::now("test", "test@test.com").unwrap();
        repo.commit(Some("HEAD"), &signature, &signature, "Initial commit", &tree, &[]).unwrap();

        // 修改一个文件
        fs::write(dir.path().join("README.md"), "# Test\nModified content\n").unwrap();

        // 新增一个未跟踪文件
        fs::write(dir.path().join("new.rs"), "fn main() {}\n").unwrap();

        dir
    }

    #[test]
    fn test_open_non_repo() {
        let dir = TempDir::new().unwrap();
        let git = GitIntegration::open(dir.path()).unwrap();
        assert!(!git.is_repo());
    }

    #[test]
    fn test_status() {
        let dir = init_test_repo();
        let git = GitIntegration::open(dir.path()).unwrap();
        assert!(git.is_repo());

        let statuses = git.status().unwrap();
        assert!(!statuses.is_empty(), "should have at least one changed file");

        // README.md 应为 modified
        let readme = statuses.iter().find(|s| s.path == "README.md");
        assert!(readme.is_some(), "README.md should be in status");
        assert_eq!(readme.unwrap().status, "modified");

        // new.rs 应为未跟踪
        let new_file = statuses.iter().find(|s| s.path == "new.rs");
        assert!(new_file.is_some(), "new.rs should be in status");
    }

    #[test]
    fn test_diff() {
        let dir = init_test_repo();
        let git = GitIntegration::open(dir.path()).unwrap();
        let diff_output = git.diff(None).unwrap();
        assert!(diff_output.contains("Modified content"), "diff should show changes");
    }

    #[test]
    fn test_log() {
        let dir = init_test_repo();
        let git = GitIntegration::open(dir.path()).unwrap();
        let commits = git.log(5).unwrap();
        assert_eq!(commits.len(), 1, "should have 1 commit");
        assert_eq!(commits[0].message, "Initial commit");
    }

    #[test]
    fn test_non_repo_returns_empty() {
        let dir = TempDir::new().unwrap();
        let git = GitIntegration::open(dir.path()).unwrap();
        assert!(git.status().unwrap().is_empty());
        assert_eq!(git.log(5).unwrap().len(), 0);
        assert!(git.diff(None).unwrap().contains("not a git repository"));
    }
}
