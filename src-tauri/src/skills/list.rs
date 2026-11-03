//! Parsing of `skills list --json` output and enrichment from `SKILL.md`.
//!
//! The CLI prints human-readable banner lines before the JSON payload, so the
//! raw stdout is sliced from the first `[` to the last `]` before deserializing.
//! Entries carry only CLI-known fields; the rest (description, version, author,
//! tags) is read from each skill's `SKILL.md` frontmatter.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};

use super::frontmatter::{self, SkillMd};

/// Upper bound for a `SKILL.md` read: 256 KiB. Anything larger is skipped
/// rather than pulled into memory.
const MAX_SKILL_MD_BYTES: u64 = 256 * 1024;

const STATUS_TRACKED: &str = "tracked";
const STATUS_UNTRACKED: &str = "untracked";
const STATUS_MISSING: &str = "missing";

/// A skill entry as reported by `skills list --json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawListEntry {
    pub name: String,
    pub path: Option<String>,
    pub scope: String,
    #[serde(default)]
    pub agents: Vec<String>,
    pub source: Option<String>,
    #[serde(rename = "sourceUrl")]
    pub source_url: Option<String>,
    #[serde(rename = "sourceType")]
    pub source_type: Option<String>,
    #[serde(default = "default_status")]
    pub status: String,
}

/// A skill entry with metadata merged in from `SKILL.md`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledSkill {
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub author: Option<String>,
    pub tags: Option<Vec<String>>,
    pub path: Option<String>,
    pub scope: String,
    pub agents: Vec<String>,
    pub source: Option<String>,
    pub source_url: Option<String>,
    pub source_type: Option<String>,
    pub status: String,
    pub has_skill_md: bool,
}

fn default_status() -> String {
    STATUS_UNTRACKED.to_string()
}

/// Extract the JSON array from raw CLI stdout and parse it.
///
/// Banner lines before and after the payload are ignored. Entries with a
/// status this build does not know about are reported as `untracked`, so an
/// upstream addition never hides a skill.
pub fn parse_entries(stdout: &str) -> Result<Vec<RawListEntry>> {
    let start = stdout
        .find('[')
        .ok_or_else(|| AppError::Skill("`skills list --json` printed no JSON array".to_string()))?;
    let end = stdout
        .rfind(']')
        .ok_or_else(|| AppError::Skill("`skills list --json` printed an unterminated JSON array".to_string()))?;

    let slice = stdout.get(start..end + 1).ok_or_else(|| {
        AppError::Skill("`skills list --json` printed a malformed JSON array".to_string())
    })?;

    let mut entries: Vec<RawListEntry> = serde_json::from_str(slice).map_err(|e| {
        AppError::Skill(format!("failed to parse `skills list --json` output: {e}"))
    })?;

    for entry in &mut entries {
        entry.status = normalize_status(&entry.status);
    }

    Ok(entries)
}

/// Merge `SKILL.md` frontmatter into a CLI entry.
///
/// Missing skills (status `missing`, `path: null`) keep an empty description
/// and report `has_skill_md = false`. The `name` from the CLI always wins.
pub fn enrich(entry: RawListEntry) -> InstalledSkill {
    let RawListEntry {
        name,
        path,
        scope,
        agents,
        source,
        source_url,
        source_type,
        status,
    } = entry;

    let md = if status == STATUS_MISSING {
        None
    } else {
        path.as_deref().and_then(|p| read_skill_md(Path::new(p)))
    };

    let (description, version, author, tags) = match &md {
        Some(md) => (
            md.description.clone().unwrap_or_default(),
            md.version.clone(),
            md.author.clone(),
            md.tags.clone(),
        ),
        None => (String::new(), None, None, None),
    };

    InstalledSkill {
        name,
        description,
        version,
        author,
        tags,
        path,
        scope,
        agents,
        source,
        source_url,
        source_type,
        status,
        has_skill_md: md.is_some(),
    }
}

/// Read and parse the `SKILL.md` of one skill.
///
/// `path` may be either the skill directory or the `SKILL.md` file itself.
/// Returns `None` when the file is absent, larger than 256 KiB, not UTF-8, or
/// otherwise unreadable.
pub fn read_skill_md(path: &Path) -> Option<SkillMd> {
    let file = if path.is_dir() {
        path.join("SKILL.md")
    } else {
        path.to_path_buf()
    };

    let metadata = std::fs::metadata(&file).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_SKILL_MD_BYTES {
        return None;
    }

    let content = std::fs::read_to_string(&file).ok()?;
    Some(frontmatter::parse(&content))
}

/// Map a CLI status onto the three known values; anything else is `untracked`.
fn normalize_status(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        STATUS_TRACKED => STATUS_TRACKED.to_string(),
        STATUS_UNTRACKED => STATUS_UNTRACKED.to_string(),
        STATUS_MISSING => STATUS_MISSING.to_string(),
        _ => STATUS_UNTRACKED.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_after_banner_noise() {
        let raw = r#"hello
[
  {"name":"find-skills","path":"C:\\x\\find-skills","scope":"global","agents":["OpenCode"],"source":"vercel-labs/skills","sourceUrl":null,"sourceType":"github","status":"tracked"}
]
"#;
        let entries = parse_entries(raw).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "find-skills");
        assert_eq!(entries[0].scope, "global");
        assert_eq!(entries[0].status, "tracked");
    }

    #[test]
    fn keeps_missing_entries_and_defaults_unknown_status() {
        let raw = r#"[
  {"name":"gone","path":null,"scope":"project","agents":[],"source":null,"sourceUrl":null,"sourceType":null,"status":"missing"},
  {"name":"odd","path":"/tmp/odd","scope":"global","agents":[],"source":null,"sourceUrl":null,"sourceType":null,"status":"future"}
]"#;
        let entries = parse_entries(raw).unwrap();
        assert!(entries[0].path.is_none());
        assert_eq!(entries[0].status, "missing");
        assert_eq!(entries[1].status, "untracked");
    }

    #[test]
    fn parses_empty_array() {
        assert!(parse_entries("[]").unwrap().is_empty());
    }

    #[test]
    fn missing_entry_enriches_without_frontmatter() {
        let entry = RawListEntry {
            name: "gone".into(),
            path: None,
            scope: "global".into(),
            agents: vec![],
            source: None,
            source_url: None,
            source_type: None,
            status: STATUS_MISSING.into(),
        };
        let skill = enrich(entry);
        assert_eq!(skill.name, "gone");
        assert!(skill.description.is_empty());
        assert!(!skill.has_skill_md);
    }

    #[test]
    fn enrich_reads_skill_md_frontmatter() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("SKILL.md"),
            "---\nname: on-disk\ndescription: From frontmatter\nversion: \"2.0.0\"\nauthor: vercel\ntags: [a, b]\n---\n\nBody\n",
        )
        .unwrap();

        let entry = RawListEntry {
            name: "from-cli".into(),
            path: Some(dir.path().to_string_lossy().into_owned()),
            scope: "project".into(),
            agents: vec!["OpenCode".into()],
            source: Some("vercel-labs/skills".into()),
            source_url: None,
            source_type: Some("github".into()),
            status: STATUS_TRACKED.into(),
        };
        let skill = enrich(entry);
        assert_eq!(skill.name, "from-cli", "CLI name must win over frontmatter");
        assert_eq!(skill.description, "From frontmatter");
        assert_eq!(skill.version.as_deref(), Some("2.0.0"));
        assert_eq!(skill.author.as_deref(), Some("vercel"));
        assert_eq!(skill.tags.as_deref(), Some(&["a".to_string(), "b".to_string()][..]));
        assert!(skill.has_skill_md);
    }

    #[test]
    fn skips_oversized_and_missing_skill_md() {
        assert!(read_skill_md(Path::new("this/path/does/not/exist")).is_none());

        let dir = tempfile::tempdir().unwrap();
        let oversized = vec![b'a'; (MAX_SKILL_MD_BYTES + 1) as usize];
        std::fs::write(dir.path().join("SKILL.md"), oversized).unwrap();
        assert!(read_skill_md(dir.path()).is_none());
    }
}
