use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::Result;
use crate::skills::loader;

/// Result of a reconcile operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconcileResult {
    /// Skill IDs that were auto-added (found on disk, missing from DB)
    pub added: Vec<String>,
    /// Skill IDs that were auto-removed (DB record, but files missing)
    pub removed: Vec<String>,
}

/// A skill directory found on disk during scanning.
pub struct ScannedSkill {
    pub id: String,
    pub path: PathBuf,
    /// "yaml" if skill.yaml present, "sk.md" if only SKILL.md found
    pub format: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: Option<String>,
    pub icon: Option<String>,
    pub tags: Option<Vec<String>>,
}

/// Directories to scan for skills, relative to user home.
pub const HOME_SKILL_DIRS: &[&str] = &[
    ".agent/skills",
    ".agents/skills",
];

/// Parse YAML frontmatter from a Markdown file (between --- markers).
pub fn parse_yaml_frontmatter(content: &str) -> Option<(Value, usize)> {
    let trimmed = content.trim();
    if !trimmed.starts_with("---") {
        return None;
    }
    let after_opening = &trimmed[3..];
    let end = after_opening.find("\n---")?;
    let yaml_str = &after_opening[..end];
    let frontmatter: Value = serde_yaml::from_str(yaml_str).ok()?;
    let body_start = 3 + end + 5;
    Some((frontmatter, body_start))
}

/// Metadata extracted from SKILL.md YAML frontmatter
#[derive(Debug, Clone, Deserialize)]
pub struct SkillMdFrontmatter {
    pub name: Option<String>,
    pub description: Option<String>,
    pub version: Option<String>,
    pub author: Option<String>,
    pub icon: Option<String>,
    pub tags: Option<Vec<String>>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Scan a single directory for skill subdirectories.
/// Accepts subdirs with skill.yaml (full format) or SKILL.md (YAML frontmatter).
pub fn scan_data_dir(data_dir: &Path) -> Result<Vec<ScannedSkill>> {
    let mut results = Vec::new();

    if !data_dir.exists() || !data_dir.is_dir() {
        return Ok(results);
    }

    let read_dir = match std::fs::read_dir(data_dir) {
        Ok(rd) => rd,
        Err(_) => return Ok(results),
    };

    for entry in read_dir {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let id = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Check for skill.yaml
        let skill_yaml = path.join("skill.yaml");
        if skill_yaml.exists() && skill_yaml.is_file() {
            if let Ok(meta) = loader::parse_skill_yaml(&skill_yaml) {
                results.push(ScannedSkill {
                    id,
                    path,
                    format: "yaml".to_string(),
                    name: meta.name,
                    description: meta.description,
                    version: meta.version,
                    author: meta.author,
                    icon: meta.icon,
                    tags: meta.tags,
                });
                continue;
            }
        }

        // Check for SKILL.md
        let skill_md = path.join("SKILL.md");
        if skill_md.exists() && skill_md.is_file() {
            if let Ok(content) = std::fs::read_to_string(&skill_md) {
                if let Some((fm, _)) = parse_yaml_frontmatter(&content) {
                    if let Ok(parsed) = serde_json::from_value::<SkillMdFrontmatter>(fm) {
                        results.push(ScannedSkill {
                            name: parsed.name.clone().unwrap_or_else(|| id.clone()),
                            description: parsed.description.unwrap_or_default(),
                            version: parsed.version.clone().unwrap_or_else(|| "0.1.0".to_string()),
                            author: parsed.author,
                            icon: parsed.icon,
                            tags: parsed.tags,
                            id,
                            path,
                            format: "sk.md".to_string(),
                        });
                    }
                }
            }
        }
    }

    Ok(results)
}

/// Scan multiple directories and merge results (deduplicates by id, first wins).
pub fn scan_dirs(dirs: &[&Path]) -> Result<Vec<ScannedSkill>> {
    let mut seen: HashMap<String, ScannedSkill> = HashMap::new();
    for dir in dirs {
        let skills = scan_data_dir(dir)?;
        for skill in skills {
            if !seen.contains_key(&skill.id) {
                seen.insert(skill.id.clone(), skill);
            }
        }
    }
    Ok(seen.into_values().collect())
}
