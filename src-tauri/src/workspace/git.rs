use std::path::Path;
use serde::{Deserialize, Serialize};
use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitFileStatus {
    pub path: String,
    pub status: String,  // "modified", "added", "deleted", "renamed", "untracked", "typechange"
    pub staged: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitInfo {
    pub hash: String,
    pub author: String,
    pub timestamp: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlameLine {
    pub line: usize,
    pub content: String,
    pub commit_hash: String,
    pub author: String,
    pub timestamp: String,
}

pub struct GitIntegration {
    repo: Option<git2::Repository>,
}

impl GitIntegration {
    pub fn open(path: &Path) -> Result<Self> {
        match git2::Repository::discover(path) {
            Ok(repo) => Ok(Self { repo: Some(repo) }),
            Err(_) => Ok(Self { repo: None }),
        }
    }

    pub fn is_repo(&self) -> bool {
        self.repo.is_some()
    }

    pub fn status(&self) -> Result<Vec<GitFileStatus>> {
        let repo = match &self.repo {
            Some(r) => r,
            None => return Ok(Vec::new()),
        };

        let statuses = repo.statuses(None)
            .map_err(|e| crate::error::AppError::Workspace(format!("Git status error: {}", e)))?;

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
            .map_err(|e| crate::error::AppError::Workspace(format!("Git diff error: {}", e)))?;

        let mut result = String::new();
        diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
            let prefix = match line.origin() {
                '+' => "+",
                '-' => "-",
                ' ' => " ",
                _ => " ",
            };
            if let Ok(content) = std::str::from_utf8(line.content()) {
                // Skip the initial +/-/space prefix since we add our own
                let content = content.trim_end_matches('\n');
                result.push_str(&format!("{}{}\n", prefix, content));
            }
            true
        }).map_err(|e| crate::error::AppError::Workspace(format!("Git diff print error: {}", e)))?;

        Ok(result)
    }

    pub fn log(&self, n: usize) -> Result<Vec<CommitInfo>> {
        let repo = match &self.repo {
            Some(r) => r,
            None => return Ok(Vec::new()),
        };

        let mut revwalk = repo.revwalk()
            .map_err(|e| crate::error::AppError::Workspace(format!("Git revwalk error: {}", e)))?;
        revwalk.push_head()
            .map_err(|e| crate::error::AppError::Workspace(format!("Git push head error: {}", e)))?;
        revwalk.set_sorting(git2::Sort::TIME)
            .map_err(|e| crate::error::AppError::Workspace(format!("Git sort error: {}", e)))?;

        let mut commits = Vec::new();
        for (i, oid) in revwalk.enumerate() {
            if i >= n {
                break;
            }
            let oid = oid.map_err(|e| crate::error::AppError::Workspace(format!("Git oid error: {}", e)))?;
            let commit = repo.find_commit(oid)
                .map_err(|e| crate::error::AppError::Workspace(format!("Git find commit error: {}", e)))?;

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

    pub fn blame(&self, path: &str) -> Result<Vec<BlameLine>> {
        let repo = match &self.repo {
            Some(r) => r,
            None => return Ok(Vec::new()),
        };

        let repo_path = repo.path().parent().unwrap_or(Path::new("."));
        let full_path = repo_path.join(path);

        let blame = repo.blame_file(&full_path, None)
            .map_err(|e| crate::error::AppError::Workspace(format!("Git blame error: {}", e)))?;

        let content = std::fs::read_to_string(&full_path)
            .map_err(|e| crate::error::AppError::Workspace(format!("Read file error: {}", e)))?;

        let mut results = Vec::new();
        for (i, line_content) in content.lines().enumerate() {
            let line_num = i + 1;
            let hunk_count = blame.len();
            let mut found_hunk = None;

            // Find which hunk this line belongs to
            for hunk_idx in 0..hunk_count {
                if let Some(hunk) = blame.get_index(hunk_idx) {
                    let start = hunk.final_start_line() as usize;
                    let end = start + hunk.lines_in_hunk() as usize;
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

        // Configure test user
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "test").unwrap();
        config.set_str("user.email", "test@test.com").unwrap();

        // Create initial commit
        fs::write(dir.path().join("README.md"), "# Test\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("README.md")).unwrap();
        index.write().unwrap(); // persist index to .git/index
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let signature = git2::Signature::now("test", "test@test.com").unwrap();
        repo.commit(Some("HEAD"), &signature, &signature, "Initial commit", &tree, &[]).unwrap();

        // Modify a file
        fs::write(dir.path().join("README.md"), "# Test\nModified content\n").unwrap();

        // Add an untracked file
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

        // Check that README.md is modified
        let readme = statuses.iter().find(|s| s.path == "README.md");
        assert!(readme.is_some(), "README.md should be in status");
        assert_eq!(readme.unwrap().status, "modified");

        // Check that new.rs is untracked
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
