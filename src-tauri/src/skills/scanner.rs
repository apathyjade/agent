use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::Result;

/// Known agent types and their skill directory paths (relative to home dir)
pub const AGENT_SCAN_PATHS: &[(&str, &[&str])] = &[
    ("generic", &[".agents/skills"]),
    ("claude-code", &[".claude/skills"]),
    ("opencode", &[".config/opencode/skills"]),
    ("codex", &[".codex/skills"]),
    ("cursor", &[".cursor/rules"]),
];

/// A skill found on disk by scanning agent skill directories
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredSkill {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Path to the skill directory (containing SKILL.md or skill.yaml)
    pub path: String,
    pub version: Option<String>,
    pub author: Option<String>,
    pub icon: Option<String>,
    pub tags: Option<Vec<String>>,
    /// Which agent(s) this skill was found in (e.g. ["claude-code", "opencode"])
    pub agent_sources: Vec<String>,
    /// Whether this skill is already imported into the local DB
    pub already_imported: bool,
    /// The format of the skill definition found
    pub format: String, // "sk.md" or "yaml"
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

/// Parse YAML frontmatter from a Markdown file.
/// Returns the parsed YAML value and the byte offset where the body starts.
pub fn parse_yaml_frontmatter(content: &str) -> Option<(Value, usize)> {
    let trimmed = content.trim();
    if !trimmed.starts_with("---") {
        return None;
    }

    // Find closing --- after the opening
    let after_opening = &trimmed[3..];
    let end = after_opening.find("\n---")?;
    let yaml_str = &after_opening[..end];

    let frontmatter: Value = serde_yaml::from_str(yaml_str).ok()?;
    // body starts after opening --- + yaml + closing ---
    let body_start = 3 + end + 5; // 3 for "---", +5 for "\n---" trailing
    Some((frontmatter, body_start))
}

/// Read and parse SKILL.md from a directory
pub fn read_skill_md(dir: &Path) -> Result<Option<SkillMdFrontmatter>> {
    let path = dir.join("SKILL.md");
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)?;
    if let Some((fm, _)) = parse_yaml_frontmatter(&content) {
        let parsed: SkillMdFrontmatter = serde_json::from_value(fm)
            .map_err(|e| crate::error::AppError::SkillValidation(format!(
                "Invalid SKILL.md frontmatter: {}", e
            )))?;
        Ok(Some(parsed))
    } else {
        Ok(None)
    }
}

/// Scan a single agent directory for skills (one level deep).
/// Each child directory containing SKILL.md or skill.yaml is a discovered skill.
fn scan_agent_dir(dir: &Path, agent_source: &str) -> Result<Vec<DiscoveredSkill>> {
    let mut results = Vec::new();
    if !dir.exists() || !dir.is_dir() {
        return Ok(results);
    }

    let read_dir = match std::fs::read_dir(dir) {
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

        let skill_dir_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Check for SKILL.md or skill.yaml
        let skill_md_path = path.join("SKILL.md");
        let skill_yaml_path = path.join("skill.yaml");

        let (format, name, description, version, author, icon, tags) =
            if skill_md_path.exists() && skill_md_path.is_file() {
                // Parse SKILL.md frontmatter
                let content = match std::fs::read_to_string(&skill_md_path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let (fm, _) = match parse_yaml_frontmatter(&content) {
                    Some(f) => f,
                    None => continue,
                };
                let parsed: SkillMdFrontmatter = match serde_json::from_value(fm) {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                (
                    "sk.md".to_string(),
                    parsed.name.unwrap_or_else(|| skill_dir_name.clone()),
                    parsed.description.unwrap_or_default(),
                    parsed.version,
                    parsed.author,
                    parsed.icon,
                    parsed.tags,
                )
            } else if skill_yaml_path.exists() && skill_yaml_path.is_file() {
                // Parse skill.yaml
                match crate::skills::loader::parse_skill_yaml(&skill_yaml_path) {
                    Ok(meta) => (
                        "yaml".to_string(),
                        meta.name,
                        meta.description,
                        Some(meta.version),
                        meta.author,
                        meta.icon,
                        meta.tags,
                    ),
                    Err(_) => continue,
                }
            } else {
                continue;
            };

        results.push(DiscoveredSkill {
            id: skill_dir_name,
            name,
            description,
            path: path.to_string_lossy().to_string(),
            version,
            author,
            icon,
            tags,
            agent_sources: vec![agent_source.to_string()],
            already_imported: false, // filled by caller
            format,
        });
    }

    Ok(results)
}

/// Scan all known agent directories and the workspace for skills.
/// Deduplicates by skill directory name and merges agent_sources.
///
/// * `workspace_skills_dir` - Optional path to the project's skills dir (e.g. `.opencode/skills/`)
pub fn scan_all(workspace_skills_dir: Option<&str>) -> Result<Vec<DiscoveredSkill>> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let mut seen: HashMap<String, DiscoveredSkill> = HashMap::new();

    // Scan known agent directories
    for (agent, rel_paths) in AGENT_SCAN_PATHS {
        for rel in rel_paths.iter() {
            let dir = home.join(rel);
            let skills = scan_agent_dir(&dir, agent)?;
            merge_skills(&mut seen, skills);
        }
    }

    // Scan workspace skills if provided
    if let Some(ws_path) = workspace_skills_dir {
        let ws_dir = Path::new(ws_path);
        if ws_dir.exists() && ws_dir.is_dir() {
            let skills = scan_agent_dir(ws_dir, "workspace")?;
            merge_skills(&mut seen, skills);
        }
    }

    Ok(seen.into_values().collect())
}

/// Merge discovered skills, combining agent_sources for duplicates
fn merge_skills(seen: &mut HashMap<String, DiscoveredSkill>, skills: Vec<DiscoveredSkill>) {
    for skill in skills {
        if let Some(existing) = seen.get_mut(&skill.id) {
            for src in &skill.agent_sources {
                if !existing.agent_sources.contains(src) {
                    existing.agent_sources.push(src.clone());
                }
            }
        } else {
            seen.insert(skill.id.clone(), skill);
        }
    }
}

/// Mark skills that are already imported
pub fn mark_imported(
    skills: &mut [DiscoveredSkill],
    imported_ids: &[String],
) {
    for skill in skills.iter_mut() {
        if imported_ids.contains(&skill.id) {
            skill.already_imported = true;
        }
    }
}
