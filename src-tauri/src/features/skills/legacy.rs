//! 旧版内置管理器遗留技能的检测与导入。
//!
//! 旧管理器把技能存成 `<data_dir>/agent/skills` 与 `~/.agent/skills` 下的
//! 一级目录，格式是 `skill.yaml`（旧格式）或 `SKILL.md`（CLI 期望的 Agent
//! Skills 格式）。只有含 `SKILL.md` 的目录能交给 `skills add`；其余会被
//! 报告出来，让界面解释跳过原因。
//!
//! 导入不会删除源目录：CLI 会链接技能（或配合 `--copy` 复制），旧目录保持
//! 原样。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::is_valid_name;
use crate::core::error::{AppError, Result};

/// 在旧根目录下发现的一个技能目录。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacySkill {
    /// 目录名，CLI 会把它当作技能名。
    pub name: String,
    pub path: String,
    /// 目录含 `SKILL.md`、可导入时为 true。
    pub importable: bool,
    /// `importable` 为 false 时说明不可导入的原因。
    pub reason: Option<String>,
}

/// 一次批量导入的结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyImportResult {
    pub imported: Vec<String>,
    pub failed: Vec<LegacyImportFailure>,
}

/// 一个导入失败的路径及其原因。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyImportFailure {
    pub path: String,
    pub error: String,
}

/// 仍在使用旧 `skill.yaml` 格式的目录所显示的原因。
const YAML_REASON: &str = "旧格式（skill.yaml），不符合 Agent Skills 规范";

/// 旧技能根目录，已去重。路径不要求存在；[`scan_root`] 对不存在的目录
/// 返回空列表。
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

/// 扫描 `root` 的一级子目录。
///
/// 含 `SKILL.md` 的目录可导入；只有 `skill.yaml` 的目录报告为不兼容；
/// 两者都没有的目录、普通文件与不可读条目全部跳过。
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

/// 构造把单个旧目录导入到全局的 `add` 参数。
///
/// 导入始终是全局的（旧根目录属于用户级），因此 `-g` 是固定参数而不是
/// 入参。
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

/// 接受旧版扫描可能产生的路径。
///
/// 在 Windows 上 `Path::is_absolute` 对 `/tmp/good` 这类 POSIX 风格路径
/// 返回 false，但测试和用户粘贴路径时仍需接受。以分隔符或盘符开头即视为
/// 类绝对路径；以 `-` 开头或含控制字符的值会被拒绝。
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
    /// 检测所有 [`roots`] 下的旧版技能目录。
    ///
    /// 合并各根目录的结果并按路径去重。
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

    /// 通过 CLI 导入旧版技能目录。
    ///
    /// 每个路径独立处理：目录不存在、缺少 `SKILL.md` 或 CLI 运行失败都会
    /// 记入 [`LegacyImportResult::failed`]，其余路径继续执行。源目录永不
    /// 删除。
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
