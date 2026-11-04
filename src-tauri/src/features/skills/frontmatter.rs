//! Parser for `SKILL.md` files with YAML frontmatter.
//!
//! A skill markdown file may start with a `---` fenced YAML block that carries
//! metadata such as name, description, version, author and tags. Everything
//! after the closing fence is the markdown body.

use serde_yaml::Value;

/// Parsed representation of a `SKILL.md` file.
#[derive(Debug, Clone, Default)]
pub struct SkillMd {
    pub name: Option<String>,
    pub description: Option<String>,
    pub version: Option<String>,
    pub author: Option<String>,
    pub tags: Option<Vec<String>>,
    /// Markdown body, with leading blank lines stripped.
    pub body: String,
}

/// Parse a `SKILL.md` document.
///
/// Never panics: when the frontmatter is absent or malformed, all metadata is
/// `None` and `body` falls back to the original content.
pub fn parse(content: &str) -> SkillMd {
    // Strip one leading UTF-8 BOM, which some editors prepend and which would
    // otherwise hide the opening fence. The fallback paths still return the
    // original content unchanged.
    let source = content.strip_prefix('\u{feff}').unwrap_or(content);

    // Normalize line endings so the fence detection behaves identically for
    // LF and CRLF documents.
    let normalized = source.replace("\r\n", "\n").replace('\r', "\n");

    let after_open = match normalized.strip_prefix("---") {
        // A document that is only the fence character sequence has no body.
        Some("") => return fallback(content),
        // The opening fence must be a standalone line, not e.g. `----`.
        Some(rest) if rest.starts_with('\n') => &rest[1..],
        _ => return fallback(content),
    };

    // Locate the closing `---` line.
    let mut offset = 0usize;
    let mut yaml_end: Option<usize> = None;
    let mut body_start: Option<usize> = None;
    for line in after_open.split_inclusive('\n') {
        if line.trim() == "---" {
            yaml_end = Some(offset);
            body_start = Some(offset + line.len());
            break;
        }
        offset += line.len();
    }

    let (yaml_end, body_start) = match (yaml_end, body_start) {
        (Some(y), Some(b)) => (y, b),
        // Unterminated frontmatter: treat the whole document as body.
        _ => return fallback(content),
    };

    let yaml = &after_open[..yaml_end];
    let body = after_open[body_start..]
        .trim_start_matches('\n')
        .to_string();

    let parsed: Value = if yaml.trim().is_empty() {
        Value::Null
    } else {
        match serde_yaml::from_str(yaml) {
            Ok(value) => value,
            // Malformed YAML: no metadata, keep the raw document as body.
            Err(_) => return fallback(content),
        }
    };

    let mut md = SkillMd {
        body,
        ..SkillMd::default()
    };

    if let Value::Mapping(map) = &parsed {
        md.name = lookup(map, "name").and_then(scalar_to_string);
        md.description = lookup(map, "description").and_then(scalar_to_string);
        // Version must stay textual: YAML would reformat `1.10` to `1.1`
        // when round-tripped through a number.
        md.version = lookup(map, "version").and_then(string_only);
        md.author = lookup(map, "author").and_then(scalar_to_string);
        md.tags = lookup(map, "tags").and_then(tags_to_vec);
    }

    md
}

fn fallback(content: &str) -> SkillMd {
    SkillMd {
        body: content.to_string(),
        ..SkillMd::default()
    }
}

fn lookup<'a>(map: &'a serde_yaml::Mapping, key: &str) -> Option<&'a Value> {
    map.get(Value::String(key.to_string()))
}

/// Convert a scalar YAML value to a string. Structured values are ignored.
fn scalar_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => trimmed_non_empty(s),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Accept only textual scalars. Used where numeric coercion would be lossy.
fn string_only(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => trimmed_non_empty(s),
        _ => None,
    }
}

fn trimmed_non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Accept either a YAML sequence of tags or a comma-separated string.
fn tags_to_vec(value: &Value) -> Option<Vec<String>> {
    match value {
        Value::Sequence(items) => {
            let tags: Vec<String> = items.iter().filter_map(scalar_to_string).collect();
            if tags.is_empty() {
                None
            } else {
                Some(tags)
            }
        }
        Value::String(s) => {
            let tags: Vec<String> = s
                .split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect();
            if tags.is_empty() {
                None
            } else {
                Some(tags)
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_frontmatter() {
        let raw = "---\nname: find-skills\ndescription: Finds skills\nversion: 1.2.0\nauthor: vercel\ntags: [a, b]\n---\n\n# Find Skills\n\nBody\n";
        let md = parse(raw);
        assert_eq!(md.name.as_deref(), Some("find-skills"));
        assert_eq!(md.description.as_deref(), Some("Finds skills"));
        assert_eq!(md.version.as_deref(), Some("1.2.0"));
        assert_eq!(md.author.as_deref(), Some("vercel"));
        assert_eq!(md.tags.as_deref(), Some(&["a".to_string(), "b".to_string()][..]));
        assert!(md.body.contains("# Find Skills"));
    }

    #[test]
    fn handles_missing_frontmatter() {
        let md = parse("# Title\n\nBody");
        assert!(md.name.is_none());
        assert_eq!(md.body, "# Title\n\nBody");
    }

    #[test]
    fn handles_malformed_yaml_without_panicking() {
        let md = parse("---\nname: [unclosed\n---\nBody");
        assert!(md.name.is_none());
        assert!(md.body.contains("Body"));
    }

    #[test]
    fn handles_crlf() {
        let md = parse("---\r\nname: x\r\n---\r\nBody");
        assert_eq!(md.name.as_deref(), Some("x"));
        assert!(md.body.contains("Body"));
    }

    #[test]
    fn parses_lone_fence_without_panicking() {
        let md = parse("---");
        assert!(md.name.is_none());
        assert!(md.description.is_none());
        assert!(md.version.is_none());
        assert!(md.author.is_none());
        assert!(md.tags.is_none());
        assert_eq!(md.body, "---");
    }

    #[test]
    fn strips_leading_bom_before_frontmatter() {
        let md = parse("\u{feff}---\nname: bom-skill\ndescription: Has a BOM\n---\nBody");
        assert_eq!(md.name.as_deref(), Some("bom-skill"));
        assert_eq!(md.description.as_deref(), Some("Has a BOM"));
        assert_eq!(md.body, "Body");
    }

    #[test]
    fn numeric_version_is_not_coerced() {
        let md = parse("---\nname: numeric-version\nversion: 1.10\n---\nBody");
        assert_eq!(md.name.as_deref(), Some("numeric-version"));
        assert!(md.version.is_none());
    }

    #[test]
    fn parses_comma_separated_tags() {
        let md = parse("---\nname: tagsy\ntags: \"a, b\"\n---\nBody");
        assert_eq!(md.tags.as_deref(), Some(&["a".to_string(), "b".to_string()][..]));
    }
}
