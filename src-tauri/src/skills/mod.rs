use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::error::{AppError, Result};

pub mod agents;
pub mod cli;
pub mod frontmatter;
pub mod legacy;
pub mod list;
pub mod market;
pub use list::InstalledSkill;

// ── Read side: CLI-backed skills service ──

/// Lifetime of a successful environment probe.
const ENV_CACHE_TTL: Duration = Duration::from_secs(300);
/// Timeout for read-only CLI invocations. Generous because the first `npx`
/// call may need to populate a cold cache.
const CLI_LIST_TIMEOUT: Duration = Duration::from_secs(60);
/// Timeout for a `add --list` preview (may warm a cold npx cache).
const CLI_PREVIEW_TIMEOUT: Duration = Duration::from_secs(60);
/// Timeout for install and update operations.
const CLI_MUTATION_TIMEOUT: Duration = Duration::from_secs(180);
/// Timeout for a remove operation.
const CLI_REMOVE_TIMEOUT: Duration = Duration::from_secs(60);
/// Upper bound for a `SKILL.md` read: 256 KiB.
const MAX_CONTENT_BYTES: u64 = 256 * 1024;

/// Parameters for one `skills add` invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallRequest {
    pub source: String,
    pub skills: Vec<String>,
    pub agents: Vec<String>,
    pub scope: String,
    pub project_path: Option<String>,
    pub copy: bool,
}

/// One skill offered by a repository source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoSkill {
    pub name: String,
    pub description: Option<String>,
}

/// Result of previewing a repository source before installing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoSkillPreview {
    pub skills: Vec<RepoSkill>,
    /// CLI output with ANSI escapes removed; used as a manual-entry fallback.
    pub raw: String,
}

/// Result of a `skills check` run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheck {
    pub has_updates: bool,
    pub has_failures: bool,
    pub output: String,
}

/// Read-only view of installed skills, backed by the official `skills` CLI.
///
/// The CLI owns the on-disk state; this service only parses its JSON output
/// and merges in `SKILL.md` frontmatter.
pub struct SkillsService {
    cli: cli::CliCommand,
    env_cache: Mutex<Option<(Instant, cli::CliStatus)>>,
}

impl SkillsService {
    pub fn new() -> Self {
        Self {
            cli: cli::resolve_command(),
            env_cache: Mutex::new(None),
        }
    }

    /// Report CLI/Node availability. Successful probes are cached for five
    /// minutes; failures are not, so a fixed environment is noticed at once.
    pub async fn env_status(&self) -> cli::CliStatus {
        let mut cache = self.env_cache.lock().await;
        if let Some((checked_at, status)) = cache.as_ref() {
            if checked_at.elapsed() < ENV_CACHE_TTL {
                return status.clone();
            }
        }

        let status = cli::probe(&self.cli).await;
        *cache = if status.available {
            Some((Instant::now(), status.clone()))
        } else {
            None
        };
        status
    }

    /// List installed skills for one scope.
    ///
    /// Project scope validates `project_path` and uses it as the CLI working
    /// directory; global scope ignores it.
    pub async fn list_installed(
        &self,
        scope: &str,
        project_path: Option<&str>,
    ) -> Result<Vec<InstalledSkill>> {
        let scope = normalize_scope(scope)?;
        let cwd = project_workdir(scope, project_path)?;

        let output = cli::run(
            &self.cli,
            &build_list_args(scope),
            cwd.as_deref(),
            CLI_LIST_TIMEOUT,
        )
        .await?;

        let entries = match list::parse_entries(&output.stdout) {
            Ok(entries) => {
                accept_listing(output.success, entries, cli_failure_detail(&output))?
            }
            Err(err) => {
                if output.success {
                    return Err(err);
                }
                return Err(AppError::Skill(format!(
                    "`skills list` failed: {}",
                    cli_failure_detail(&output)
                )));
            }
        };

        if !output.success {
            log::warn!(
                "`skills list` exited with code {:?} but returned {} entries; using them",
                output.code,
                entries.len()
            );
        }

        let entries = dedupe_entries(entries);

        // Enrichment touches the filesystem for every entry, so keep it off
        // the async runtime even though the CLI call itself is awaited.
        tokio::task::spawn_blocking(move || {
            entries.into_iter().map(list::enrich).collect::<Vec<_>>()
        })
        .await
        .map_err(|err| AppError::Skill(format!("skill enrichment task failed: {err}")))
    }

    /// Read the `SKILL.md` body of one installed skill.
    pub async fn content(
        &self,
        name: &str,
        scope: &str,
        project_path: Option<&str>,
    ) -> Result<String> {
        let scope = normalize_scope(scope)?;
        let skills = self.list_installed(scope, project_path).await?;

        let skill = find_target(&skills, name, scope).ok_or_else(|| {
            AppError::NotFound(format!("Skill '{}' not found in {} scope", name, scope))
        })?;

        let path = skill.path.clone().ok_or_else(|| {
            AppError::NotFound(format!(
                "Skill '{}' has no installed path (status: {})",
                name, skill.status
            ))
        })?;

        tokio::task::spawn_blocking(move || read_content_file(Path::new(&path)))
            .await
            .map_err(|err| AppError::Skill(format!("skill content task failed: {err}")))?
    }
}

impl Default for SkillsService {
    fn default() -> Self {
        Self::new()
    }
}

// ── Write side: install, remove, check and update through the CLI ──

impl SkillsService {
    /// Ask the CLI which skills a repository source offers.
    ///
    /// The listing format is human-oriented and therefore not a stable
    /// interface: an unparseable or failed run returns an empty `skills` list
    /// plus the raw output, and the UI falls back to installing the whole
    /// repository.
    pub async fn preview_source(&self, source: &str) -> Result<RepoSkillPreview> {
        if !cli::is_valid_source(source) {
            return Err(AppError::SkillValidation(format!(
                "invalid skill source '{}'",
                source
            )));
        }

        let args = vec![
            "add".to_string(),
            source.trim().to_string(),
            "--list".to_string(),
        ];
        let output = cli::run(&self.cli, &args, None, CLI_PREVIEW_TIMEOUT).await?;
        let raw = strip_ansi(&combine_output(&output));

        if !output.success {
            log::warn!(
                "`skills add --list` exited with code {:?}: {}",
                output.code,
                raw
            );
            return Ok(RepoSkillPreview {
                skills: Vec::new(),
                raw,
            });
        }

        Ok(RepoSkillPreview {
            skills: parse_available_skills(&raw),
            raw,
        })
    }

    /// Install one or more skills from a source.
    pub async fn install(&self, req: InstallRequest) -> Result<String> {
        let scope = normalize_scope(&req.scope)?;
        let args = build_add_args(&req)?;
        let cwd = project_workdir(scope, req.project_path.as_deref())?;

        let output = cli::run(&self.cli, &args, cwd.as_deref(), CLI_MUTATION_TIMEOUT).await?;
        accept_mutation(output)
    }

    /// Remove one skill from the requested scope.
    pub async fn remove(
        &self,
        name: &str,
        scope: &str,
        project_path: Option<&str>,
    ) -> Result<String> {
        let scope = normalize_scope(scope)?;
        let args = build_remove_args(name, scope)?;
        let cwd = project_workdir(scope, project_path)?;

        let output = cli::run(&self.cli, &args, cwd.as_deref(), CLI_REMOVE_TIMEOUT).await?;
        accept_mutation(output)
    }

    /// Check for skill updates.
    ///
    /// Exit code 1 is part of the CLI contract (updates available, or checks
    /// failed), so it is not surfaced as an error; the flags are derived from
    /// the combined output by [`interpret_check`].
    pub async fn check_updates(
        &self,
        scope: &str,
        project_path: Option<&str>,
    ) -> Result<UpdateCheck> {
        let scope = normalize_scope(scope)?;
        let cwd = project_workdir(scope, project_path)?;

        let args = build_check_args(scope)?;
        let output = cli::run(&self.cli, &args, cwd.as_deref(), CLI_REMOVE_TIMEOUT).await?;

        let combined = strip_ansi(&combine_output(&output));
        let (has_updates, has_failures) = interpret_check(output.code, &combined);

        Ok(UpdateCheck {
            has_updates,
            has_failures,
            output: combined,
        })
    }

    /// Update all installed skills in the requested scope.
    pub async fn update(&self, scope: &str, project_path: Option<&str>) -> Result<String> {
        let scope = normalize_scope(scope)?;
        let cwd = project_workdir(scope, project_path)?;

        let output = cli::run(
            &self.cli,
            &build_update_args(scope)?,
            cwd.as_deref(),
            CLI_MUTATION_TIMEOUT,
        )
        .await?;
        accept_mutation(output)
    }
}

/// `--json` arguments for a list call. An unknown scope falls back to the
/// project form; callers are expected to run [`normalize_scope`] first.
pub fn build_list_args(scope: &str) -> Vec<String> {
    match scope {
        "global" => vec!["list".to_string(), "-g".to_string(), "--json".to_string()],
        _ => vec!["list".to_string(), "--json".to_string()],
    }
}

/// Build `add` arguments in the CLI's required order.
pub fn build_add_args(req: &InstallRequest) -> Result<Vec<String>> {
    let scope = normalize_scope(&req.scope)?;
    if !cli::is_valid_source(&req.source) {
        return Err(AppError::SkillValidation(format!(
            "invalid skill source '{}'",
            req.source
        )));
    }
    if req.agents.is_empty() {
        return Err(AppError::SkillValidation(
            "at least one agent is required".to_string(),
        ));
    }
    // Skill and agent names become argv entries, so a flag-like value such as
    // `-g` or an empty string must never reach the CLI.
    for skill in &req.skills {
        if skill.trim() == "*" {
            continue;
        }
        if !is_valid_name(skill) {
            return Err(AppError::SkillValidation(format!(
                "invalid skill name '{}'",
                skill
            )));
        }
    }
    for agent in &req.agents {
        if !is_valid_name(agent) {
            return Err(AppError::SkillValidation(format!(
                "invalid agent '{}'",
                agent
            )));
        }
    }

    let mut args = vec![
        "add".to_string(),
        req.source.trim().to_string(),
        "-s".to_string(),
    ];
    if req.skills.is_empty() {
        // The CLI expands `*` to every skill in the source.
        args.push("*".to_string());
    } else {
        args.extend(req.skills.iter().map(|s| s.trim().to_string()));
    }
    args.push("-a".to_string());
    args.extend(req.agents.iter().map(|a| a.trim().to_string()));
    args.push("-y".to_string());
    if scope == "global" {
        args.push("-g".to_string());
    }
    if req.copy {
        args.push("--copy".to_string());
    }

    Ok(args)
}

/// Build `remove` arguments. No `-a`: removal unlinks the skill from every agent.
pub fn build_remove_args(name: &str, scope: &str) -> Result<Vec<String>> {
    let scope = normalize_scope(scope)?;
    if !is_valid_name(name) {
        return Err(AppError::SkillValidation(format!(
            "invalid skill name '{}'",
            name
        )));
    }

    let mut args = vec![
        "remove".to_string(),
        name.trim().to_string(),
        "-y".to_string(),
    ];
    if scope == "global" {
        args.push("-g".to_string());
    }
    Ok(args)
}

/// Build `check` arguments.
///
/// Project scope passes `-p` explicitly: without it the CLI auto-detects the
/// scope and silently falls back to global skills when the project has none.
pub fn build_check_args(scope: &str) -> Result<Vec<String>> {
    let scope = normalize_scope(scope)?;
    let mut args = vec!["check".to_string()];
    match scope {
        "global" => args.push("-g".to_string()),
        _ => args.push("-p".to_string()),
    }
    Ok(args)
}

/// Build `update` arguments. Project scope passes `-p` for the same reason as
/// [`build_check_args`].
pub fn build_update_args(scope: &str) -> Result<Vec<String>> {
    let scope = normalize_scope(scope)?;
    let mut args = vec!["update".to_string(), "-y".to_string()];
    match scope {
        "global" => args.push("-g".to_string()),
        _ => args.push("-p".to_string()),
    }
    Ok(args)
}

/// Parse the skill listing printed by `skills add <source> --list`.
///
/// The CLI is piped in the app, so two renderings are observed:
///
/// * framed multi-line rows — a name line followed by deeper-indented
///   description lines;
/// * single-line rows — `name   description` on one line.
///
/// Both may be preceded by a header (`Available Skills`) and noise
/// (`Source:`, `Fetching`, `Found N skills`, ...). Parsing starts after the
/// `Available Skills` header when present, otherwise it considers every
/// non-noise line. Header and noise lines are never returned as skills.
pub fn parse_available_skills(stdout: &str) -> Vec<RepoSkill> {
    let clean = strip_ansi(stdout);
    let lines: Vec<&str> = clean.lines().map(str::trim_end).collect();

    let has_header = lines
        .iter()
        .any(|line| is_available_skills_header(strip_frame_glyphs(line).trim()));

    let mut rows: Vec<(usize, String)> = Vec::new();
    let mut header_depth: Option<usize> = None;
    let mut started = !has_header;

    for line in lines {
        let stripped = strip_frame_glyphs(line);
        let text = stripped.trim();
        if text.is_empty() {
            continue;
        }
        let depth = stripped.chars().take_while(|c| *c == ' ').count();
        if is_available_skills_header(text) {
            started = true;
            header_depth = Some(depth);
            continue;
        }
        if !started
            || is_noise_line(text)
            || is_rule_line(text)
            || !text.chars().any(char::is_alphanumeric)
        {
            continue;
        }
        rows.push((depth, text.to_string()));
    }

    let mut skills: Vec<RepoSkill> = Vec::new();
    let mut i = 0;

    while i < rows.len() {
        let (depth, text) = &rows[i];

        // Single-line form: the first run of 2+ spaces separates name and
        // description.
        if let Some((name, description)) = split_inline_row(text) {
            skills.push(RepoSkill {
                name: name.to_string(),
                description: Some(description.to_string()),
            });
            i += 1;
            continue;
        }

        // Multi-line form: a deeper next line marks this line as the name and
        // every following deeper line as its (possibly wrapped) description.
        if rows.get(i + 1).is_some_and(|(next, _)| next > depth) {
            let mut parts: Vec<String> = Vec::new();
            let mut j = i + 1;
            while j < rows.len() && rows[j].0 > *depth {
                parts.push(rows[j].1.clone());
                j += 1;
            }
            skills.push(RepoSkill {
                name: text.clone(),
                description: Some(parts.join(" ")),
            });
            i = j;
            continue;
        }

        // A bare line at the header depth is leftover chrome, not a skill.
        if header_depth.is_some_and(|header| *depth <= header) {
            i += 1;
            continue;
        }

        // Anything else at a skill depth is a name without a description.
        skills.push(RepoSkill {
            name: text.clone(),
            description: None,
        });
        i += 1;
    }

    skills
}

/// Split a single-line listing row at its first run of two or more spaces.
fn split_inline_row(text: &str) -> Option<(&str, &str)> {
    let bytes = text.as_bytes();
    for i in 0..bytes.len().saturating_sub(1) {
        if bytes[i] == b' ' && bytes[i + 1] == b' ' {
            let name = text[..i].trim();
            let description = text[i..].trim();
            if !name.is_empty() && !description.is_empty() {
                return Some((name, description));
            }
        }
    }
    None
}

/// A start-of-listing header, including variants such as
/// `Available Skills:` or `Available Skills (5)`.
fn is_available_skills_header(text: &str) -> bool {
    text.trim().to_lowercase().starts_with("available skills")
}

/// Header/progress lines that must never be mistaken for a skill.
fn is_noise_line(text: &str) -> bool {
    let lower = text.to_lowercase();
    is_available_skills_header(text)
        || lower.contains("use --skill")
        || (lower.starts_with("found ") && lower.contains("skill"))
        || [
            "source:",
            "local path",
            "fetching",
            "selected",
            "installing",
            "installed",
            "security",
            "done",
        ]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

/// A pure border/rule line: only box-drawing characters, dashes or spaces.
fn is_rule_line(text: &str) -> bool {
    text.chars()
        .all(|c| c.is_whitespace() || c == '-' || ('\u{2500}'..='\u{257F}').contains(&c))
}

/// Frame glyphs the CLI puts before every content line. `ℹ` prefixes the
/// single-line rendering.
fn is_frame_glyph(c: char) -> bool {
    matches!(c, '│' | '└' | '├' | '◇' | '●' | '•' | 'ℹ')
}

/// Drop leading frame glyphs from a line while preserving the content
/// indentation behind them.
///
/// Spaces before a glyph are consumed with it; when no glyph follows, the
/// original leading spaces are kept so plain-indented fixtures still carry a
/// measurable depth.
fn strip_frame_glyphs(line: &str) -> &str {
    let mut rest = line;
    loop {
        let trimmed = rest.trim_start_matches(' ');
        match trimmed.chars().next() {
            Some(c) if is_frame_glyph(c) => {
                rest = &trimmed[c.len_utf8()..];
            }
            _ => return rest,
        }
    }
}

/// Classify a `skills check` result as `(has_updates, has_failures)`.
///
/// The CLI uses exit code 1 both for "updates available" and for check
/// failures. Failure keywords (`failed`, `error`, `denied`, `cannot`) take
/// precedence over the word `update`, so "failed to check for updates" is a
/// failure; otherwise a non-zero exit whose output mentions `update` counts as
/// updates, and every other non-zero exit counts as a failure. A missing exit
/// code (killed process) is always a failure.
pub fn interpret_check(code: Option<i32>, output: &str) -> (bool, bool) {
    match code {
        Some(0) => (false, false),
        _ => {
            let output = output.to_lowercase();
            let failed = ["failed", "error", "denied", "cannot"]
                .iter()
                .any(|keyword| output.contains(keyword));
            if failed {
                (false, true)
            } else if output.contains("update") {
                (true, false)
            } else {
                (false, true)
            }
        }
    }
}

/// Accept only the two scopes the UI exposes.
pub fn normalize_scope(scope: &str) -> Result<&str> {
    match scope {
        "global" | "project" => Ok(scope),
        other => Err(AppError::SkillValidation(format!(
            "unknown skill scope '{}': expected 'global' or 'project'",
            other
        ))),
    }
}

/// Working directory for a list call; project scope must be a real directory.
fn project_workdir(scope: &str, project_path: Option<&str>) -> Result<Option<PathBuf>> {
    if scope != "project" {
        return Ok(None);
    }

    let raw = project_path.ok_or_else(|| {
        AppError::SkillValidation("project scope requires a project path".to_string())
    })?;
    let dir = PathBuf::from(raw);
    if !dir.is_dir() {
        return Err(AppError::SkillValidation(format!(
            "project path '{}' is not an existing directory",
            raw
        )));
    }
    Ok(Some(dir))
}

/// Find an installed skill by both name and scope.
///
/// The CLI can report the same skill name in both scopes, so matching on the
/// name alone would return whichever row happened to come first. The requested
/// scope is authoritative.
fn find_target<'a>(
    skills: &'a [InstalledSkill],
    name: &str,
    scope: &str,
) -> Option<&'a InstalledSkill> {
    skills
        .iter()
        .find(|skill| skill.name == name && skill.scope == scope)
}

/// Decide whether a `skills list` invocation is usable.
///
/// A zero exit is always accepted. A non-zero exit is accepted only when it
/// still produced at least one entry; an empty listing from a failed run is
/// indistinguishable from "no skills installed" in the UI, so it is returned
/// as an error carrying the CLI failure detail.
fn accept_listing(
    success: bool,
    entries: Vec<list::RawListEntry>,
    failure_detail: String,
) -> Result<Vec<list::RawListEntry>> {
    if success || !entries.is_empty() {
        return Ok(entries);
    }

    Err(AppError::Skill(format!(
        "`skills list` failed: {}",
        failure_detail
    )))
}

/// Keep the first row of each `(scope, name)` pair.
fn dedupe_entries(entries: Vec<list::RawListEntry>) -> Vec<list::RawListEntry> {
    let mut seen = std::collections::HashSet::new();
    entries
        .into_iter()
        .filter(|entry| seen.insert((entry.scope.clone(), entry.name.clone())))
        .collect()
}

/// Reject empty, flag-like, quoted and control-character skill/agent names.
///
/// Quotes are rejected for the same reason as in [`cli::is_valid_source`]:
/// `\"` is not a cmd.exe escape, so a name containing `"` and `&`/`|` could
/// break out of the quoting on the Windows `cmd.exe /c` path.
fn is_valid_name(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty()
        && !trimmed.starts_with('-')
        && !name.contains('"')
        && !name.chars().any(char::is_control)
}

/// Accept a write invocation: stdout on success, an error with the CLI
/// diagnostic otherwise. A mutation has no partial-success semantics worth
/// preserving here; callers re-list to show the actual state afterwards.
fn accept_mutation(output: cli::CliOutput) -> Result<String> {
    if output.success {
        return Ok(output.stdout.trim().to_string());
    }

    Err(AppError::Skill(mutation_failure_detail(&output)))
}

/// Prefer stderr, fall back to stdout, then to the exit code.
fn mutation_failure_detail(output: &cli::CliOutput) -> String {
    let stderr = output.stderr.trim();
    if !stderr.is_empty() {
        return stderr.to_string();
    }

    let stdout = output.stdout.trim();
    if !stdout.is_empty() {
        return stdout.to_string();
    }

    output
        .code
        .map(|code| format!("exit code {code}"))
        .unwrap_or_else(|| "unknown error".to_string())
}

/// Join stdout and stderr, separated by a newline when both are present.
fn combine_output(output: &cli::CliOutput) -> String {
    let stdout = output.stdout.trim_end();
    let stderr = output.stderr.trim_end();

    match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => String::new(),
        (false, true) => stdout.to_string(),
        (true, false) => stderr.to_string(),
        (false, false) => format!("{stdout}\n{stderr}"),
    }
}

/// Remove ANSI escape sequences (CSI and OSC) from CLI output.
///
/// `NO_COLOR=1` is set for every invocation, but npx and some shims still emit
/// color when stdout is a pipe, so the listing is cleaned before parsing.
fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }

        match chars.peek().copied() {
            // CSI: ESC [ ... final byte in 0x40..=0x7e
            Some('[') => {
                chars.next();
                for next in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&next) {
                        break;
                    }
                }
            }
            // OSC: ESC ] ... BEL or ESC \
            Some(']') => {
                chars.next();
                while let Some(next) = chars.next() {
                    if next == '\u{7}' {
                        break;
                    }
                    if next == '\u{1b}' {
                        if chars.peek() == Some(&'\\') {
                            chars.next();
                        }
                        break;
                    }
                }
            }
            // Any other escape sequence: drop ESC plus one character.
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }

    out
}

/// One actionable line describing a failed CLI invocation.
fn cli_failure_detail(output: &cli::CliOutput) -> String {
    let stderr = output
        .stderr
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty());
    match stderr {
        Some(line) => line.to_string(),
        None => output
            .code
            .map(|code| format!("exit code {code}"))
            .unwrap_or_else(|| "unknown error".to_string()),
    }
}

/// Read a skill's `SKILL.md`, truncating the text at 256 KiB.
///
/// `path` may be the skill directory or the file itself. A missing or
/// unreadable file is reported as [`AppError::NotFound`].
fn read_content_file(path: &Path) -> Result<String> {
    let file_path = if path.is_dir() {
        path.join("SKILL.md")
    } else {
        path.to_path_buf()
    };

    let file = std::fs::File::open(&file_path)
        .map_err(|err| AppError::NotFound(format!("cannot read {}: {err}", file_path.display())))?;

    let mut bytes = Vec::new();
    file.take(MAX_CONTENT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| AppError::Skill(format!("cannot read {}: {err}", file_path.display())))?;

    if bytes.len() as u64 > MAX_CONTENT_BYTES {
        bytes.truncate(MAX_CONTENT_BYTES as usize);
    }

    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_entry(name: &str, scope: &str, path: Option<String>) -> list::RawListEntry {
        list::RawListEntry {
            name: name.into(),
            path,
            scope: scope.into(),
            agents: vec![],
            source: None,
            source_url: None,
            source_type: None,
            status: "tracked".into(),
        }
    }

    #[test]
    fn builds_list_args_for_scope() {
        assert_eq!(build_list_args("global"), vec!["list", "-g", "--json"]);
        assert_eq!(build_list_args("project"), vec!["list", "--json"]);
    }

    #[test]
    fn rejects_unknown_scope() {
        assert!(normalize_scope("both").is_err());
        assert!(normalize_scope("GLOBAL").is_err());
    }

    #[test]
    fn accepts_supported_scopes() {
        assert_eq!(normalize_scope("global").unwrap(), "global");
        assert_eq!(normalize_scope("project").unwrap(), "project");
    }

    #[test]
    fn missing_entries_survive_enrichment() {
        let entry = list::RawListEntry {
            name: "gone".into(),
            path: None,
            scope: "global".into(),
            agents: vec![],
            source: None,
            source_url: None,
            source_type: None,
            status: "missing".into(),
        };
        let skill = list::enrich(entry);
        assert_eq!(skill.status, "missing");
        assert!(skill.path.is_none());
        assert!(!skill.has_skill_md);
    }

    #[test]
    fn dedupes_entries_by_scope_and_name() {
        let entries = vec![
            raw_entry("dup", "global", Some("a".into())),
            raw_entry("dup", "global", Some("b".into())),
            raw_entry("dup", "project", Some("c".into())),
            raw_entry("other", "global", None),
        ];
        let deduped = dedupe_entries(entries);
        assert_eq!(deduped.len(), 3);
        assert_eq!(deduped[0].path.as_deref(), Some("a"));
    }

    #[test]
    fn finds_target_by_name_and_scope() {
        let skills = vec![
            list::enrich(raw_entry("same", "global", Some("global-path".into()))),
            list::enrich(raw_entry("same", "project", Some("project-path".into()))),
        ];

        let project = find_target(&skills, "same", "project").unwrap();
        assert_eq!(project.scope, "project");
        assert_eq!(project.path.as_deref(), Some("project-path"));

        let global = find_target(&skills, "same", "global").unwrap();
        assert_eq!(global.scope, "global");
        assert_eq!(global.path.as_deref(), Some("global-path"));

        assert!(find_target(&skills, "same", "other").is_none());
        assert!(find_target(&skills, "absent", "global").is_none());
    }

    #[test]
    fn rejects_failed_listing_only_when_empty() {
        // A successful run may legitimately report no skills.
        assert!(accept_listing(true, vec![], "ignored".into())
            .unwrap()
            .is_empty());

        // A failed run that still printed entries keeps working.
        let entries = vec![raw_entry("one", "global", Some("p".into()))];
        assert_eq!(
            accept_listing(false, entries, "ignored".into())
                .unwrap()
                .len(),
            1
        );

        // A failed run with an empty listing must surface the failure.
        let err = accept_listing(false, vec![], "boom".into()).unwrap_err();
        assert!(matches!(err, AppError::Skill(_)));
        assert!(err.to_string().contains("boom"));
    }

    #[test]
    fn project_scope_requires_an_existing_directory() {
        // Global scope never passes a working directory.
        assert!(project_workdir("global", None).unwrap().is_none());

        assert!(project_workdir("project", None).is_err());
        assert!(project_workdir("project", Some("this/path/does/not/exist")).is_err());

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_string_lossy().into_owned();
        let cwd = project_workdir("project", Some(&path)).unwrap();
        assert_eq!(cwd.as_deref(), Some(dir.path()));

        let file = dir.path().join("not-a-dir.txt");
        std::fs::write(&file, "x").unwrap();
        let file_path = file.to_string_lossy().into_owned();
        assert!(project_workdir("project", Some(&file_path)).is_err());
    }

    #[test]
    fn reads_skill_md_from_directory_or_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("SKILL.md"),
            "---\nname: demo\n---\n\nHello there\n",
        )
        .unwrap();

        let from_dir = read_content_file(dir.path()).unwrap();
        assert!(from_dir.contains("Hello there"));

        let from_file = read_content_file(&dir.path().join("SKILL.md")).unwrap();
        assert_eq!(from_file, from_dir);
    }

    #[test]
    fn truncates_oversized_skill_md() {
        let dir = tempfile::tempdir().unwrap();
        let oversized = vec![b'a'; MAX_CONTENT_BYTES as usize + 10];
        std::fs::write(dir.path().join("SKILL.md"), oversized).unwrap();

        let text = read_content_file(dir.path()).unwrap();
        assert_eq!(text.len(), MAX_CONTENT_BYTES as usize);
    }

    #[test]
    fn reports_not_found_for_unreadable_skill_md() {
        assert!(matches!(
            read_content_file(Path::new("this/path/does/not/exist")),
            Err(AppError::NotFound(_))
        ));
    }
}

#[cfg(test)]
mod mutation_tests {
    use super::*;

    fn req(scope: &str, copy: bool) -> InstallRequest {
        InstallRequest {
            source: "vercel-labs/agent-skills".into(),
            skills: vec!["web-design-guidelines".into()],
            agents: vec!["opencode".into(), "claude-code".into()],
            scope: scope.into(),
            project_path: None,
            copy,
        }
    }

    #[test]
    fn builds_global_install_args() {
        assert_eq!(
            build_add_args(&req("global", false)).unwrap(),
            vec![
                "add",
                "vercel-labs/agent-skills",
                "-s",
                "web-design-guidelines",
                "-a",
                "opencode",
                "claude-code",
                "-y",
                "-g"
            ]
        );
    }

    #[test]
    fn builds_project_install_args_with_copy() {
        let args = build_add_args(&req("project", true)).unwrap();
        assert!(!args.contains(&"-g".to_string()));
        assert!(args.contains(&"--copy".to_string()));
    }

    #[test]
    fn rejects_install_without_agents_or_valid_source() {
        let mut r = req("global", false);
        r.agents.clear();
        assert!(build_add_args(&r).is_err());
        let mut r = req("global", false);
        r.source = "--all".into();
        assert!(build_add_args(&r).is_err());
    }

    #[test]
    fn rejects_flag_like_skills_and_blank_agents() {
        let mut r = req("global", false);
        r.skills = vec!["-g".into()];
        assert!(build_add_args(&r).is_err());

        let mut r = req("global", false);
        r.skills = vec!["--copy".into()];
        assert!(build_add_args(&r).is_err());

        let mut r = req("global", false);
        r.agents = vec!["".into()];
        assert!(build_add_args(&r).is_err());

        let mut r = req("global", false);
        r.agents = vec!["opencode".into(), "".into()];
        assert!(build_add_args(&r).is_err());
    }

    #[test]
    fn rejects_quotes_in_names_and_sources() {
        assert!(!is_valid_name("a\"&calc"));
        assert!(!cli::is_valid_source("https://x/a\"&calc"));
        assert!(cli::is_valid_source("https://x/a?b=1&c=2"));

        let mut r = req("global", false);
        r.skills = vec!["a\"&calc".into()];
        assert!(build_add_args(&r).is_err());

        let mut r = req("global", false);
        r.source = "https://x/a\"&calc".into();
        assert!(build_add_args(&r).is_err());
    }

    #[test]
    fn accepts_wildcard_and_trims_valid_values() {
        let mut r = req("global", false);
        r.skills = vec!["*".into()];
        r.agents = vec![" opencode ".into()];
        let args = build_add_args(&r).unwrap();
        assert!(args.contains(&"*".to_string()));
        assert!(args.contains(&"opencode".to_string()));
        assert!(!args.contains(&" opencode ".to_string()));
    }

    #[test]
    fn builds_remove_check_update_args() {
        assert_eq!(
            build_remove_args("x", "global").unwrap(),
            vec!["remove", "x", "-y", "-g"]
        );
        assert_eq!(build_check_args("global").unwrap(), vec!["check", "-g"]);
        assert_eq!(build_check_args("project").unwrap(), vec!["check", "-p"]);
        assert_eq!(
            build_update_args("global").unwrap(),
            vec!["update", "-y", "-g"]
        );
        assert_eq!(
            build_update_args("project").unwrap(),
            vec!["update", "-y", "-p"]
        );
    }

    #[test]
    fn parses_framed_cli_output() {
        let raw = "\
│    alpha
│      First skill
│    beta
│      Second skill

  Use --skill <name> to install specific skills
";
        let skills = parse_available_skills(raw);
        assert_eq!(skills.len(), 2);
        assert_eq!(skills[0].name, "alpha");
        assert_eq!(skills[0].description.as_deref(), Some("First skill"));
        assert_eq!(skills[1].name, "beta");
        assert_eq!(skills[1].description.as_deref(), Some("Second skill"));
    }

    #[test]
    fn parses_single_line_cli_output() {
        let raw = "\
ℹ   find-skills   Helps users discover and install agent skills
ℹ   other-skill   Does other things
";
        let skills = parse_available_skills(raw);
        assert_eq!(skills.len(), 2);
        assert_eq!(skills[0].name, "find-skills");
        assert_eq!(
            skills[0].description.as_deref(),
            Some("Helps users discover and install agent skills")
        );
        assert_eq!(skills[1].name, "other-skill");
        assert_eq!(skills[1].description.as_deref(), Some("Does other things"));
    }

    #[test]
    fn parses_header_and_noise_around_framed_list() {
        let raw = "\
Available Skills
Source: vercel-labs/agent-skills
Fetching skills from GitHub...
Found 9 skills
│    alpha
│      First skill
│    beta
│      Second skill
│    gamma
│      Third skill
│    delta
│      Fourth skill
│    epsilon
│      Fifth skill
│    zeta
│      Sixth skill
│    eta
│      Seventh skill
│    theta
│      Eighth skill
│    iota
│      Ninth skill
Use --skill <name> to install specific skills
";
        let skills = parse_available_skills(raw);
        assert_eq!(skills.len(), 9);
        assert_eq!(skills[0].name, "alpha");
        assert_eq!(skills[0].description.as_deref(), Some("First skill"));
        assert_eq!(skills[8].name, "iota");
        assert_eq!(skills[8].description.as_deref(), Some("Ninth skill"));
        assert!(skills.iter().all(|skill| skill.name != "Available Skills"));
        assert!(skills
            .iter()
            .all(|skill| !skill.name.contains("Found 9 skills")));
    }

    #[test]
    fn ignores_noise_before_header_and_border_lines() {
        let raw = "\
ℹ  Local path validated
ℹ  Found 1 skill
│  Available Skills
│    find-skills
│      Helps users discover and install agent skills
│  Use --skill <name> to install specific skills
";
        let skills = parse_available_skills(raw);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "find-skills");
        assert_eq!(
            skills[0].description.as_deref(),
            Some("Helps users discover and install agent skills")
        );
    }

    #[test]
    fn skips_rule_and_symbol_only_lines() {
        let raw = "
  Available Skills
  ──────────────
  ●
  alpha
    First skill
";
        let skills = parse_available_skills(raw);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "alpha");
        assert_eq!(skills[0].description.as_deref(), Some("First skill"));
    }

    #[test]
    fn skips_header_even_when_list_is_empty() {
        let raw = "\
Available Skills
Source: vercel-labs/agent-skills
Fetching skills...
Found 0 skills
Use --skill <name> to install specific skills
";
        assert!(parse_available_skills(raw).is_empty());
    }

    #[test]
    fn joins_wrapped_descriptions() {
        let raw = "
  wrapped
    First line of the description
    second line of the description
  plain
";
        let skills = parse_available_skills(raw);
        assert_eq!(skills.len(), 2);
        assert_eq!(
            skills[0].description.as_deref(),
            Some("First line of the description second line of the description")
        );
        assert_eq!(skills[1].name, "plain");
        assert!(skills[1].description.is_none());
    }

    #[test]
    fn parses_available_skills_output() {
        let raw = "\n  vercel-composition-patterns\n    React composition patterns that scale.\n  deploy-to-vercel\n    Deploy applications and websites.\n\n  Use --skill <name> to install specific skills\n";
        let skills = parse_available_skills(raw);
        assert_eq!(skills.len(), 2);
        assert_eq!(skills[0].name, "vercel-composition-patterns");
        assert_eq!(
            skills[0].description.as_deref(),
            Some("React composition patterns that scale.")
        );
    }

    #[test]
    fn interprets_check_exit_codes() {
        assert_eq!(
            interpret_check(Some(1), "x has an update available"),
            (true, false)
        );
        assert_eq!(interpret_check(Some(1), "failed to check y"), (false, true));
        assert_eq!(interpret_check(Some(0), "all good"), (false, false));
    }

    #[test]
    fn failure_keywords_override_the_word_update() {
        assert_eq!(
            interpret_check(Some(1), "failed to check for updates"),
            (false, true)
        );
        assert_eq!(
            interpret_check(Some(1), "error: update denied"),
            (false, true)
        );
        assert_eq!(
            interpret_check(Some(1), "cannot reach registry for updates"),
            (false, true)
        );
    }

    #[test]
    fn trims_successful_mutation_stdout() {
        let output = cli::CliOutput {
            stdout: "installed\n\n".into(),
            stderr: String::new(),
            success: true,
            code: Some(0),
        };
        assert_eq!(accept_mutation(output).unwrap(), "installed");
    }

    #[test]
    fn installs_every_skill_when_none_selected() {
        let mut r = req("global", false);
        r.skills.clear();
        let args = build_add_args(&r).unwrap();
        let skill_pos = args.iter().position(|a| a == "-s").unwrap();
        assert_eq!(args[skill_pos + 1], "*");
    }

    #[test]
    fn rejects_unknown_scope_in_every_builder() {
        let mut r = req("workspace", false);
        r.skills.clear();
        assert!(build_add_args(&r).is_err());
        assert!(build_remove_args("x", "workspace").is_err());
        assert!(build_check_args("workspace").is_err());
        assert!(build_update_args("workspace").is_err());
    }

    #[test]
    fn strips_ansi_sequences_before_parsing() {
        let raw = "\u{1b}[36m  one\u{1b}[0m\n\u{1b}[2m    First skill\u{1b}[0m\n";
        let skills = parse_available_skills(raw);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "one");
        assert_eq!(skills[0].description.as_deref(), Some("First skill"));
    }

    #[test]
    fn treats_missing_exit_code_as_failure() {
        assert_eq!(interpret_check(None, "check timed out"), (false, true));
        assert_eq!(interpret_check(Some(2), "boom"), (false, true));
    }
}
