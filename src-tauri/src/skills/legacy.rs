//! Detection and import of skills left behind by the legacy in-app manager.
//!
//! The legacy manager stored skills as first-level directories under
//! `<data_dir>/agent/skills` and `~/.agent/skills`, using either `skill.yaml`
//! (the old format) or `SKILL.md` (the Agent Skills format the CLI expects).
//! Only `SKILL.md` directories can be handed to `skills add`; the rest are
//! reported so the UI can explain why they are skipped.
//!
//! Importing never deletes the source directory: the CLI links the skill (or
//! copies it with `--copy`), and the old directory stays untouched.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::is_valid_name;
use crate::error::{AppError, Result};

/// One skill directory found under a legacy root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacySkill {
    /// Directory name, which is what the CLI uses as the skill name.
    pub name: String,
    pub path: String,
    /// True when the directory contains `SKILL.md` and can be imported.
    pub importable: bool,
    /// Why the directory cannot be imported, when `importable` is false.
    pub reason: Option<String>,
}

/// Outcome of one bulk import run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyImportResult {
    pub imported: Vec<String>,
    pub failed: Vec<LegacyImportFailure>,
}

/// One path that could not be imported, with the reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyImportFailure {
    pub path: String,
    pub error: String,
}

/// Reason shown for directories still using the old `skill.yaml` format.
const YAML_REASON: &str = "旧格式（skill.yaml），不符合 Agent Skills 规范";

/// Legacy skill roots, deduplicated. Paths need not exist; [`scan_root`]
/// returns an empty list for a missing directory.
pub fn roots() -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();

    if let Some(data) = dirs::data_dir() {
        found.push(data.join("agent").join("skills"));
    }
    if let Some(home) = dirs::home_dir() {
        found.push(home.join(".agent").join("skills"));
    }

    let mut seen = std::collections::HashSet::new();
    found.retain(|path| seen.insert(path.clone()));
    found
}

/// Scan the first-level subdirectories of `root`.
///
/// A directory with `SKILL.md` is importable. A directory with only
/// `skill.yaml` is reported as incompatible. Directories with neither are
/// skipped entirely, as are plain files and unreadable entries.
pub fn scan_root(root: &Path) -> Vec<LegacySkill> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    let mut skills: Vec<LegacySkill> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = match path.file_name().and_then(|name| name.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };

        if path.join("SKILL.md").is_file() {
            skills.push(LegacySkill {
                name,
                path: path.to_string_lossy().into_owned(),
                importable: true,
                reason: None,
            });
        } else if path.join("skill.yaml").is_file() {
            skills.push(LegacySkill {
                name,
                path: path.to_string_lossy().into_owned(),
                importable: false,
                reason: Some(YAML_REASON.to_string()),
            });
        }
    }

    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

/// Build `add` arguments that import one legacy directory globally.
///
/// Imports are always global (legacy roots are user-wide), so `-g` is part of
/// the fixed argument list rather than a parameter.
pub fn build_import_args(path: &str, agents: &[String], copy: bool) -> Result<Vec<String>> {
    if !is_importable_path(path) {
        return Err(AppError::SkillValidation(format!(
            "invalid skill path '{}'",
            path
        )));
    }
    if agents.is_empty() {
        return Err(AppError::SkillValidation(
            "at least one agent is required".to_string(),
        ));
    }
    for agent in agents {
        if !is_valid_name(agent) {
            return Err(AppError::SkillValidation(format!(
                "invalid agent '{}'",
                agent
            )));
        }
    }

    let mut args = vec![
        "add".to_string(),
        path.trim().to_string(),
        "-a".to_string(),
    ];
    args.extend(agents.iter().map(|agent| agent.trim().to_string()));
    args.push("-g".to_string());
    args.push("-y".to_string());
    if copy {
        args.push("--copy".to_string());
    }

    Ok(args)
}

/// Accept the paths a legacy scan can produce.
///
/// `Path::is_absolute` is false on Windows for POSIX-style paths such as
/// `/tmp/good`, which the code still needs to accept in tests and when a
/// user pastes a path. A leading separator or drive prefix counts as
/// absolute-like, and flag-like or control-character values are rejected.
fn is_importable_path(path: &str) -> bool {
    let trimmed = path.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('-')
        || trimmed.contains('"')
        || trimmed.chars().any(char::is_control)
    {
        return false;
    }

    let candidate = Path::new(trimmed);
    candidate.is_absolute() || trimmed.starts_with('/') || trimmed.starts_with('\\')
}

impl super::SkillsService {
    /// Detect legacy skill directories under every [`roots`] entry.
    ///
    /// Results from all roots are merged and deduplicated by path.
    pub async fn detect_legacy(&self) -> Result<Vec<LegacySkill>> {
        tokio::task::spawn_blocking(|| {
            let mut found: Vec<LegacySkill> = Vec::new();
            for root in roots() {
                found.extend(scan_root(&root));
            }

            let mut seen = std::collections::HashSet::new();
            found.retain(|skill| seen.insert(skill.path.clone()));
            found
        })
        .await
        .map_err(|err| AppError::Skill(format!("legacy skill scan task failed: {err}")))
    }

    /// Import legacy skill directories through the CLI.
    ///
    /// Every path is handled independently: a missing directory, a directory
    /// without `SKILL.md` or a failed CLI run is recorded in
    /// [`LegacyImportResult::failed`] and the remaining paths still run.
    /// Source directories are never removed.
    pub async fn import_legacy(
        &self,
        paths: &[String],
        agents: &[String],
        copy: bool,
    ) -> Result<LegacyImportResult> {
        if agents.is_empty() {
            return Err(AppError::SkillValidation(
                "at least one agent is required".to_string(),
            ));
        }
        for agent in agents {
            if !is_valid_name(agent) {
                return Err(AppError::SkillValidation(format!(
                    "invalid agent '{}'",
                    agent
                )));
            }
        }

        let mut result = LegacyImportResult {
            imported: Vec::new(),
            failed: Vec::new(),
        };

        for path in paths {
            let dir = Path::new(path);
            if !dir.is_dir() {
                result.failed.push(LegacyImportFailure {
                    path: path.clone(),
                    error: "skill directory does not exist".to_string(),
                });
                continue;
            }
            if !dir.join("SKILL.md").is_file() {
                result.failed.push(LegacyImportFailure {
                    path: path.clone(),
                    error: "skill directory has no SKILL.md".to_string(),
                });
                continue;
            }

            let args = match build_import_args(path, agents, copy) {
                Ok(args) => args,
                Err(err) => {
                    result.failed.push(LegacyImportFailure {
                        path: path.clone(),
                        error: err.to_string(),
                    });
                    continue;
                }
            };

            match super::cli::run(&self.cli, &args, None, super::CLI_MUTATION_TIMEOUT).await {
                Ok(output) if output.success => result.imported.push(path.clone()),
                Ok(output) => result.failed.push(LegacyImportFailure {
                    path: path.clone(),
                    error: super::mutation_failure_detail(&output),
                }),
                Err(err) => result.failed.push(LegacyImportFailure {
                    path: path.clone(),
                    error: err.to_string(),
                }),
            }
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn scans_importable_and_incompatible_skills() {
        let dir = tempfile::tempdir().unwrap();
        let md = dir.path().join("good");
        fs::create_dir_all(&md).unwrap();
        fs::write(md.join("SKILL.md"), "---\nname: good\n---\nBody").unwrap();

        let yaml = dir.path().join("legacy");
        fs::create_dir_all(&yaml).unwrap();
        fs::write(yaml.join("skill.yaml"), "id: legacy").unwrap();

        let found = scan_root(dir.path());
        assert_eq!(found.len(), 2);
        let good = found.iter().find(|s| s.name == "good").unwrap();
        assert!(good.importable);
        let legacy = found.iter().find(|s| s.name == "legacy").unwrap();
        assert!(!legacy.importable);
        assert!(legacy.reason.is_some());
    }

    #[test]
    fn missing_root_is_empty() {
        assert!(scan_root(Path::new("Z:\\does-not-exist\\skills")).is_empty());
    }

    #[test]
    fn builds_global_import_args() {
        let args = build_import_args("/tmp/good", &["opencode".into()], true).unwrap();
        assert_eq!(args, vec!["add", "/tmp/good", "-a", "opencode", "-g", "-y", "--copy"]);
    }

    #[test]
    fn rejects_import_without_agents() {
        assert!(build_import_args("/tmp/good", &[], false).is_err());
    }
}
