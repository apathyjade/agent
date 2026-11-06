//! 带 YAML frontmatter 的 `SKILL.md` 解析器。
//!
//! 技能 markdown 文件可以以 `---` 围起来的 YAML 块开头，携带 name、
//! description、version、author、tags 等元数据；闭合围栏之后的内容是
//! markdown 正文。

use serde_yaml::Value;

/// `SKILL.md` 文件的解析结果。
#[derive(Debug, Clone, Default)]
pub struct SkillMd {
    pub name: Option<String>,
    pub description: Option<String>,
    pub version: Option<String>,
    pub author: Option<String>,
    pub tags: Option<Vec<String>>,
    /// markdown 正文，已去掉开头的空行。
    pub body: String,
}

/// 解析一个 `SKILL.md` 文档。
///
/// 永不 panic：frontmatter 缺失或格式错误时所有元数据为 `None`，
/// `body` 回退为原始内容。
pub fn parse(content: &str) -> SkillMd {
    // 去掉一个开头的 UTF-8 BOM，部分编辑器会写入它并遮住起始围栏。
    // 各回退分支仍返回未经修改的原始内容。
    let source = content.strip_prefix('\u{feff}').unwrap_or(content);

    // 归一化换行符，使 LF 与 CRLF 文档的围栏识别行为一致。
    let normalized = source.replace("\r\n", "\n").replace('\r', "\n");

    let after_open = match normalized.strip_prefix("---") {
        // 只有围栏字符、没有正文的文档。
        Some("") => return fallback(content),
        // 起始围栏必须是独立一行，例如不能是 `----`。
        Some(rest) if rest.starts_with('\n') => &rest[1..],
        _ => return fallback(content),
    };

    // 定位闭合的 `---` 行。
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
        // frontmatter 未闭合：把整篇文档当作正文。
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
            // YAML 格式错误：无元数据，保留原始文档作为正文。
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
        // version 必须保持文本形式：经过数字往返时 YAML 会把 `1.10`
        // 重写成 `1.1`。
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

/// 把 YAML 标量转成字符串；结构化值忽略。
fn scalar_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => trimmed_non_empty(s),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// 只接受文本标量。用于数字强制转换会丢失信息的位置。
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

/// 同时接受 YAML 序列与逗号分隔字符串两种 tags 写法。
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
