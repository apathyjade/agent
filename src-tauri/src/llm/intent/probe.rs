//! Bounded, read-only project probe.
//!
//! Used only when the LLM rerank is low-confidence. The probe never reuses the
//! agent `ToolRegistry`: it lists the project root and does one keyword search
//! inside it. Any error, timeout, or path escape yields `None`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::core::db::models::IntentRecord;

const MAX_DIR_ENTRIES: usize = 60;
const MAX_FILES: usize = 120;
const MAX_DEPTH: usize = 4;
const MAX_FILE_BYTES: u64 = 128 * 1024;
const MAX_EVIDENCE_CHARS: usize = 1200;
const MAX_MATCHES: usize = 8;
const MAX_KEYWORDS: usize = 5;

const SKIPPED_DIRS: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".venv",
    "venv",
    "__pycache__",
];

/// Collect a short evidence string for `intent` inside `project_root`.
pub async fn probe(
    intent: &IntentRecord,
    project_root: &Path,
    timeout: Duration,
) -> Option<String> {
    let root = project_root.to_path_buf();
    let keywords = keywords(intent);
    if keywords.is_empty() {
        return None;
    }

    let work = tokio::task::spawn_blocking(move || collect_evidence(&root, &keywords));
    match tokio::time::timeout(timeout, work).await {
        Ok(Ok(evidence)) => evidence,
        Ok(Err(error)) => {
            log::warn!("intent probe task failed: {error}");
            None
        }
        Err(_) => {
            log::warn!("intent probe timed out");
            None
        }
    }
}

fn keywords(intent: &IntentRecord) -> Vec<String> {
    let mut result = Vec::new();
    let mut push = |value: &str| {
        let trimmed = value.trim();
        if trimmed.chars().count() >= 2 && result.len() < MAX_KEYWORDS {
            let lowered = trimmed.to_lowercase();
            if !result.iter().any(|item: &String| item == &lowered) {
                result.push(lowered);
            }
        }
    };

    push(&intent.name);
    for example in &intent.examples {
        push(example);
    }
    for token in intent
        .description
        .split(|ch: char| ch.is_whitespace() || ch.is_ascii_punctuation())
    {
        push(token);
    }

    result
}

fn collect_evidence(root: &Path, keywords: &[String]) -> Option<String> {
    let root = root.canonicalize().ok()?;
    if !root.is_dir() {
        return None;
    }

    let mut parts = Vec::new();
    if let Ok(entries) = fs::read_dir(&root) {
        let mut names: Vec<String> = entries
            .flatten()
            .take(MAX_DIR_ENTRIES)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect();
        names.sort();
        if !names.is_empty() {
            parts.push(format!("project root: {}", names.join(", ")));
        }
    }

    let mut matches = Vec::new();
    let mut files_seen = 0usize;
    walk(&root, &root, keywords, 0, &mut files_seen, &mut matches);

    if !matches.is_empty() {
        parts.push(format!("keyword matches:\n{}", matches.join("\n")));
    }

    let mut evidence = parts.join("\n");
    if evidence.is_empty() {
        return None;
    }
    if evidence.chars().count() > MAX_EVIDENCE_CHARS {
        evidence = evidence.chars().take(MAX_EVIDENCE_CHARS).collect();
    }
    Some(evidence)
}

fn walk(
    root: &Path,
    dir: &Path,
    keywords: &[String],
    depth: usize,
    files_seen: &mut usize,
    matches: &mut Vec<String>,
) {
    if depth > MAX_DEPTH || *files_seen >= MAX_FILES || matches.len() >= MAX_MATCHES {
        return;
    }

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        if *files_seen >= MAX_FILES || matches.len() >= MAX_MATCHES {
            return;
        }

        let path = entry.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if metadata.file_type().is_symlink() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        if metadata.is_dir() {
            if name.starts_with('.') || SKIPPED_DIRS.contains(&name.as_str()) {
                continue;
            }
            walk(root, &path, keywords, depth + 1, files_seen, matches);
            continue;
        }

        if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
            continue;
        }

        let Some(safe) = safe_path(root, &path) else {
            continue;
        };
        if !is_probably_text(&safe) {
            continue;
        }
        let Ok(content) = fs::read_to_string(&safe) else {
            continue;
        };
        *files_seen += 1;

        for line in content.lines() {
            let lowered = line.to_lowercase();
            if keywords.iter().any(|keyword| lowered.contains(keyword)) {
                let relative = safe
                    .strip_prefix(root)
                    .unwrap_or(&safe)
                    .to_string_lossy()
                    .to_string();
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                matches.push(format!(
                    "{}: {}",
                    relative,
                    trimmed.chars().take(160).collect::<String>()
                ));
                if matches.len() >= MAX_MATCHES {
                    return;
                }
            }
        }
    }
}

fn is_probably_text(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_lowercase();
    if matches!(
        extension.as_str(),
        "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "ico"
            | "pdf"
            | "zip"
            | "gz"
            | "tar"
            | "exe"
            | "dll"
            | "so"
            | "dylib"
            | "db"
            | "sqlite"
            | "woff"
            | "woff2"
            | "ttf"
    ) {
        return false;
    }

    let Ok(bytes) = fs::read(path).map(|bytes| bytes.into_iter().take(512).collect::<Vec<_>>())
    else {
        return false;
    };
    !bytes.contains(&0)
}

/// Canonicalize `candidate` and require it to stay inside `root`.
fn safe_path(root: &Path, candidate: &Path) -> Option<PathBuf> {
    let canonical = candidate.canonicalize().ok()?;
    if canonical.starts_with(root) {
        Some(canonical)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn intent() -> IntentRecord {
        IntentRecord {
            id: "test.intent".to_string(),
            domain_id: "test.domain".to_string(),
            name: "实现功能".to_string(),
            description: "写代码，加开关".to_string(),
            examples: vec!["深色模式开关".to_string()],
            workflow: String::new(),
            allowed_tools: Vec::new(),
            default_agent: String::new(),
            scope: "global".to_string(),
            enabled: true,
            version: 1,
            confidence_threshold: 0.7,
            fallback_intent: String::new(),
            path: "fast".to_string(),
            auto_escalate: false,
            system_prompt_appendix: String::new(),
            model_id: String::new(),
            max_iterations: None,
            sort_order: 0,
            created_at: "2026-11-05T00:00:00Z".to_string(),
            updated_at: "2026-11-05T00:00:00Z".to_string(),
        }
    }

    fn temp_project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        fs::create_dir_all(dir.path().join("src")).unwrap();
        let mut file = fs::File::create(dir.path().join("src/settings.ts")).unwrap();
        writeln!(file, "export const darkMode = false;").unwrap();
        fs::File::create(dir.path().join("README.md")).unwrap();
        dir
    }

    #[tokio::test]
    async fn probe_returns_directory_listing_and_matches() {
        let project = temp_project();
        let mut intent = intent();
        intent.examples = vec!["darkMode".to_string()];
        let evidence = probe(&intent, project.path(), Duration::from_secs(2))
            .await
            .expect("evidence");

        assert!(evidence.contains("README.md"));
        assert!(evidence.contains("settings.ts"));
        assert!(evidence.contains("darkMode"));
    }

    #[tokio::test]
    async fn probe_without_matching_keywords_returns_listing_only() {
        let project = temp_project();
        let mut intent = intent();
        intent.name = "查询项目".to_string();
        intent.description = "zzzz".to_string();
        intent.examples = vec!["qqqq".to_string()];

        let evidence = probe(&intent, project.path(), Duration::from_secs(2))
            .await
            .expect("listing evidence");
        assert!(evidence.contains("README.md"));
        assert!(!evidence.contains("keyword matches"));
    }

    #[tokio::test]
    async fn probe_skips_paths_outside_root() {
        let project = temp_project();
        let outside = tempfile::tempdir().expect("outside");
        let outside_file = outside.path().join("secret.txt");
        fs::write(&outside_file, "secret").unwrap();

        assert!(safe_path(&project.path().canonicalize().unwrap(), &outside_file).is_none());
    }

    #[tokio::test]
    async fn missing_root_returns_none() {
        let intent = intent();
        let missing = std::env::temp_dir().join("intent-probe-does-not-exist-xyz");
        assert!(probe(&intent, &missing, Duration::from_millis(200))
            .await
            .is_none());
    }
}
