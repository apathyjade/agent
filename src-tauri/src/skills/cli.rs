//! Thin wrapper around the official `skills` CLI (skills.sh).
//!
//! Every invocation is built from an argument array — the CLI is never run
//! through a shell string. Child processes get color/telemetry disabled and,
//! on Windows, are started without a console window.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::error::{AppError, Result};

/// Pinned CLI version — the module is built against this exact release.
pub const SKILLS_CLI_VERSION: &str = "1.7.6";
/// Minimum Node.js version required by the CLI.
pub const MIN_NODE_VERSION: (u64, u64, u64) = (22, 20, 0);
/// Probe timeout: local checks only, never a download.
const PROBE_TIMEOUT: Duration = Duration::from_secs(8);
/// `CREATE_NO_WINDOW` — avoids a flashing console window for CLI shims.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Result of a local environment probe (shown on the skills page).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliStatus {
    pub available: bool,
    pub cli_version: Option<String>,
    pub node_version: Option<String>,
    pub command: Option<String>,
    pub error: Option<String>,
}

/// A resolved way to call the CLI: either `skills` directly or `npx skills@x`.
#[derive(Debug, Clone)]
pub struct CliCommand {
    pub program: PathBuf,
    pub prefix_args: Vec<String>,
    pub display: String,
}

/// Captured result of one CLI invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliOutput {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
    pub code: Option<i32>,
}

#[cfg(windows)]
const SKILLS_NAMES: &[&str] = &["skills.cmd", "skills.exe", "skills"];
#[cfg(not(windows))]
const SKILLS_NAMES: &[&str] = &["skills"];

#[cfg(windows)]
const NPX_NAMES: &[&str] = &["npx.cmd", "npx.exe", "npx"];
#[cfg(not(windows))]
const NPX_NAMES: &[&str] = &["npx"];

#[cfg(windows)]
const NODE_NAMES: &[&str] = &["node.exe"];
#[cfg(not(windows))]
const NODE_NAMES: &[&str] = &["node"];

#[cfg(windows)]
const NODE_EXE: &str = "node.exe";
#[cfg(not(windows))]
const NODE_EXE: &str = "node";

/// Resolve the CLI command, first match wins:
/// 1. `skills` on PATH, 2. `npx` on PATH, 3. the fnm default alias,
/// 4. a bare `npx` (spawn fails later and `probe` reports it).
pub fn resolve_command() -> CliCommand {
    if let Some(program) = find_in_path(SKILLS_NAMES) {
        return CliCommand {
            program,
            prefix_args: Vec::new(),
            display: "skills".to_string(),
        };
    }

    if let Some(program) = find_in_path(NPX_NAMES) {
        return npx_command(program);
    }

    if let Some(dir) = fnm_default_dir() {
        for name in NPX_NAMES {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return npx_command(candidate);
            }
        }
    }

    // Last resort: let the process spawner resolve `npx` through PATH.
    npx_command(PathBuf::from(if cfg!(windows) { "npx.cmd" } else { "npx" }))
}

/// Translate a resolved command into the concrete `(program, argv)` pair.
///
/// Windows `.cmd`/`.bat` shims cannot be spawned directly, so they are wrapped
/// in `cmd.exe /c`. The shim path is quoted so `cmd.exe` sees one token even
/// when it contains spaces (`C:\Program Files\nodejs\npx.cmd`).
pub fn spawn_parts(command: &CliCommand) -> (String, Vec<String>) {
    let mut args = command.prefix_args.clone();
    let is_script = command
        .program
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"))
        .unwrap_or(false);

    if cfg!(windows) && is_script {
        let mut wrapped = Vec::with_capacity(args.len() + 2);
        wrapped.push("/c".to_string());
        wrapped.push(format!("\"{}\"", command.program.to_string_lossy()));
        wrapped.append(&mut args);
        ("cmd.exe".to_string(), wrapped)
    } else {
        (command.program.to_string_lossy().into_owned(), args)
    }
}

/// Run the CLI with the given arguments.
///
/// `cwd` is used for project-scoped operations; global operations pass `None`.
/// The child inherits a PATH with the Node install directory prepended so
/// `.cmd` shims can find `node` even when the GUI process has no Node on PATH.
pub async fn run(
    command: &CliCommand,
    args: &[String],
    cwd: Option<&Path>,
    timeout: Duration,
) -> Result<CliOutput> {
    let (program, mut spawn_args) = spawn_parts(command);
    spawn_args.extend(args.iter().cloned());

    let node_dir = node_dir_for(command);
    execute(
        &program,
        &spawn_args,
        node_dir.as_deref(),
        cwd,
        timeout,
        &command.display,
    )
    .await
}

/// Probe the local environment without downloading anything.
///
/// Checks `node --version` and `<cli> --version`; a failure never returns an
/// error, it fills [`CliStatus::error`] with an actionable hint instead.
pub async fn probe(command: &CliCommand) -> CliStatus {
    let node_dir = node_dir_for(command);
    let mut status = CliStatus {
        available: false,
        cli_version: None,
        node_version: None,
        command: Some(command.display.clone()),
        error: None,
    };

    // ── Node.js ──
    let node_program = node_program(node_dir.as_deref());
    let node_args = vec!["--version".to_string()];
    let node_result = execute(
        &node_program,
        &node_args,
        node_dir.as_deref(),
        None,
        PROBE_TIMEOUT,
        "node --version",
    )
    .await;

    let node_version = node_result
        .as_ref()
        .ok()
        .and_then(|out| first_line(&out.stdout));
    let node_parsed = node_version.as_deref().and_then(parse_version);
    status.node_version = node_version.clone();

    if !node_parsed.map(|v| v >= MIN_NODE_VERSION).unwrap_or(false) {
        status.error = Some(match &node_version {
            Some(found) => format!(
                "Node.js 版本过低（{found}）：需要 Node.js ≥ 22.20。"
            ),
            None => "未检测到 Node.js：需要 Node.js ≥ 22.20 与 npx（npm）。".to_string(),
        });
        return status;
    }

    // The npx form is pinned to `skills@1.7.6`, and running it here would hit
    // the network on a cold cache. Node alone is enough to declare it usable;
    // the version is verified by the pinned invocation itself.
    if !is_direct_skills(command) {
        status.available = true;
        return status;
    }

    // ── skills CLI (direct binary only) ──
    let (program, mut cli_args) = spawn_parts(command);
    cli_args.push("--version".to_string());
    let cli_result = execute(
        &program,
        &cli_args,
        node_dir.as_deref(),
        None,
        PROBE_TIMEOUT,
        &command.display,
    )
    .await;

    let cli_output = match cli_result {
        Ok(out) => out,
        Err(err) => {
            status.error = Some(format!(
                "无法运行 skills CLI（{err}）：需要 Node.js ≥ 22.20 与 npx（npm）。"
            ));
            return status;
        }
    };

    let cli_raw = first_line(&cli_output.stdout).or_else(|| first_line(&cli_output.stderr));
    status.cli_version = cli_raw.clone();
    let cli_parsed = cli_raw.as_deref().and_then(parse_version);

    if !cli_output.success {
        let detail = first_line(&cli_output.stderr)
            .or_else(|| cli_output.code.map(|code| format!("exit code {code}")))
            .unwrap_or_else(|| "unknown error".to_string());
        status.error = Some(format!(
            "skills CLI 运行失败（{detail}）：需要 Node.js ≥ 22.20 与 npx（npm）。"
        ));
        return status;
    }

    match cli_parsed {
        Some(version) if version >= parse_version(SKILLS_CLI_VERSION).unwrap_or((1, 7, 6)) => {
            status.available = true;
        }
        Some(version) => {
            status.error = Some(format!(
                "skills CLI 版本过低（{}.{}.{}）：需要 ≥ {}。",
                version.0, version.1, version.2, SKILLS_CLI_VERSION
            ));
        }
        None => {
            status.error = Some("无法识别 skills CLI 的版本输出。".to_string());
        }
    }

    status
}

/// Parse `1.7.6`, `v1.7.6` or `v22.20.0` into a comparable tuple.
pub fn parse_version(raw: &str) -> Option<(u64, u64, u64)> {
    let trimmed = raw.trim();
    let without_prefix = trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))
        .unwrap_or(trimmed);
    let core: String = without_prefix
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();

    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

/// Reject empty, flag-like and control-character sources before they reach the CLI.
pub fn is_valid_source(source: &str) -> bool {
    if source.chars().any(char::is_control) {
        return false;
    }
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return false;
    }
    // A leading `-` would be parsed as a flag by the CLI.
    !trimmed.starts_with('-')
}

// ── Internals ──

async fn execute(
    program: &str,
    args: &[String],
    node_dir: Option<&Path>,
    cwd: Option<&Path>,
    timeout: Duration,
    display: &str,
) -> Result<CliOutput> {
    let mut cmd = Command::new(program);
    append_args(&mut cmd, program, args);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("NO_COLOR", "1")
        .env("DISABLE_TELEMETRY", "1")
        .env("DO_NOT_TRACK", "1")
        .kill_on_drop(true);

    if let Some(dir) = node_dir {
        if let Some(path) = path_with_prefix(dir) {
            cmd.env("PATH", path);
        }
    }

    if let Some(dir) = cwd {
        if dir.is_dir() {
            cmd.current_dir(dir);
        }
    }

    #[cfg(windows)]
    {
        // Provided by `tokio::process::Command` itself on Windows.
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    match tokio::time::timeout(timeout, cmd.output()).await {
        Err(_) => Err(AppError::Skill(format!(
            "`{}` timed out after {}s",
            display,
            timeout.as_secs()
        ))),
        Ok(Err(err)) => Err(AppError::Skill(format!(
            "Failed to start `{}`: {}",
            display, err
        ))),
        Ok(Ok(output)) => Ok(CliOutput {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            success: output.status.success(),
            code: output.status.code(),
        }),
    }
}

/// Feed `args` to the child, special-casing the `cmd.exe /c` wrapper.
///
/// `spawn_parts` already quotes the shim path; [`build_cmd_line`] quotes every
/// remaining token and appends the result with `raw_arg`, so `cmd.exe` never
/// sees an unquoted `&`, `|`, `^`, `<` or `>` from a user-supplied source or
/// skill name. Appending raw also keeps the quotes literal instead of letting
/// the Windows argv builder escape them as `\"` (which `cmd.exe` would not
/// understand).
fn append_args(cmd: &mut Command, program: &str, args: &[String]) {
    #[cfg(windows)]
    {
        if program.eq_ignore_ascii_case("cmd.exe") && args.len() >= 2 && args[0] == "/c" {
            cmd.arg("/c");
            cmd.raw_arg(build_cmd_line(&args[1..]));
            return;
        }
    }
    #[cfg(not(windows))]
    {
        let _ = program;
    }
    cmd.args(args);
}

/// Assemble a `cmd.exe` command line with every token double-quoted.
///
/// `tokens[0]` is the shim path that [`spawn_parts`] already quoted; it is
/// unquoted first so the result never contains doubled-up quotes. Embedded
/// quotes are escaped as `\"` (cmd-style). The whole line is then wrapped in
/// one extra pair of quotes: `cmd /c` strips the outermost pair before
/// parsing, which is what leaves the remaining quoted tokens intact.
#[cfg(windows)]
fn build_cmd_line(tokens: &[String]) -> String {
    let quoted: Vec<String> = tokens
        .iter()
        .enumerate()
        .map(|(index, token)| {
            let raw = if index == 0 {
                token
                    .strip_prefix('"')
                    .and_then(|t| t.strip_suffix('"'))
                    .unwrap_or(token.as_str())
            } else {
                token.as_str()
            };
            format!("\"{}\"", raw.replace('"', "\\\""))
        })
        .collect();

    format!("\"{}\"", quoted.join(" "))
}

/// True when the resolved program is a `skills` binary, not the npx shim.
fn is_direct_skills(command: &CliCommand) -> bool {
    command
        .program
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.eq_ignore_ascii_case("skills"))
        .unwrap_or(false)
}

fn npx_command(program: PathBuf) -> CliCommand {
    CliCommand {
        program,
        prefix_args: vec!["--yes".to_string(), format!("skills@{SKILLS_CLI_VERSION}")],
        display: format!("npx skills@{SKILLS_CLI_VERSION}"),
    }
}

fn find_in_path(names: &[&str]) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// fnm's stable "default" alias directory, which holds the Node shims.
fn fnm_default_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let appdata = std::env::var_os("APPDATA")?;
        let dir = PathBuf::from(appdata)
            .join("fnm")
            .join("aliases")
            .join("default");
        if dir.is_dir() {
            return Some(dir);
        }
        None
    }
    #[cfg(not(windows))]
    {
        let dir = dirs::home_dir()?
            .join(".local")
            .join("share")
            .join("fnm")
            .join("aliases")
            .join("default")
            .join("bin");
        if dir.is_dir() {
            return Some(dir);
        }
        None
    }
}

/// Directory that must be on the child PATH (and used to locate `node`).
fn node_dir_for(command: &CliCommand) -> Option<PathBuf> {
    if let Some(parent) = command.program.parent() {
        if !parent.as_os_str().is_empty() && parent.is_dir() {
            return Some(parent.to_path_buf());
        }
    }
    if let Some(dir) = fnm_default_dir() {
        if dir.join(NODE_EXE).is_file() {
            return Some(dir);
        }
    }
    find_in_path(NODE_NAMES).and_then(|node| node.parent().map(Path::to_path_buf))
}

fn node_program(node_dir: Option<&Path>) -> String {
    if let Some(dir) = node_dir {
        let candidate = dir.join(NODE_EXE);
        if candidate.is_file() {
            return candidate.to_string_lossy().into_owned();
        }
    }
    "node".to_string()
}

fn path_with_prefix(dir: &Path) -> Option<OsString> {
    let mut parts = vec![dir.to_path_buf()];
    if let Some(path) = std::env::var_os("PATH") {
        parts.extend(std::env::split_paths(&path));
    }
    std::env::join_paths(parts).ok()
}

fn first_line(raw: &str) -> Option<String> {
    raw.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_supported_sources() {
        for s in [
            "vercel-labs/agent-skills",
            "https://github.com/vercel-labs/agent-skills",
            "git@github.com:vercel-labs/agent-skills.git",
            "ssh://git@example.com/x.git",
            "./local-skills",
            r"C:\skills\local",
        ] {
            assert!(is_valid_source(s), "{s} should be valid");
        }
    }

    #[test]
    fn rejects_flag_like_empty_and_control_sources() {
        assert!(!is_valid_source(""));
        assert!(!is_valid_source("   "));
        assert!(!is_valid_source("-g"));
        assert!(!is_valid_source("--all"));
        assert!(!is_valid_source("a\0b"));
    }

    #[test]
    fn parses_versions_with_optional_prefix() {
        assert_eq!(parse_version("1.7.6"), Some((1, 7, 6)));
        assert_eq!(parse_version("v1.7.6"), Some((1, 7, 6)));
        assert_eq!(parse_version("nope"), None);
    }

    #[cfg(windows)]
    #[test]
    fn wraps_cmd_shims_with_cmd_exe() {
        let cmd = CliCommand {
            program: PathBuf::from(r"C:\node\npx.cmd"),
            prefix_args: vec!["--yes".into(), "skills@1.7.6".into()],
            display: "npx skills@1.7.6".into(),
        };
        let (program, args) = spawn_parts(&cmd);
        assert_eq!(program, "cmd.exe");
        assert_eq!(args[0], "/c");
        assert!(args[1].contains("npx.cmd"));
        assert!(
            args[1].starts_with('"') && args[1].ends_with('"'),
            "shim path must be quoted: {}",
            args[1]
        );
    }

    #[cfg(windows)]
    #[test]
    fn quotes_shim_paths_containing_spaces() {
        let cmd = CliCommand {
            program: PathBuf::from(r"C:\Program Files\nodejs\npx.cmd"),
            prefix_args: vec!["--yes".into(), "skills@1.7.6".into()],
            display: "npx skills@1.7.6".into(),
        };
        let (program, args) = spawn_parts(&cmd);
        assert_eq!(program, "cmd.exe");
        assert_eq!(args[1], r#""C:\Program Files\nodejs\npx.cmd""#);
        assert_eq!(&args[2..], ["--yes", "skills@1.7.6"]);
    }

    #[cfg(windows)]
    #[test]
    fn quotes_every_cmd_argument_and_escapes_quotes() {
        let tokens = vec![
            r#""C:\node\npx.cmd""#.to_string(),
            "add".to_string(),
            "a&b".to_string(),
            "x|y".to_string(),
            r#"say"hi"#.to_string(),
        ];
        let line = build_cmd_line(&tokens);

        assert!(line.contains(r#""a&b""#), "line: {line}");
        assert!(line.contains(r#""x|y""#), "line: {line}");
        assert!(line.contains(r#""add""#), "line: {line}");
        assert!(line.contains(r#""say\"hi""#), "line: {line}");
        assert!(
            line.starts_with("\"\"C:\\node\\npx.cmd\""),
            "shim path must stay one token: {line}"
        );
        // cmd /c strips the outermost pair of quotes, leaving the fully
        // quoted command line intact.
        assert!(line.starts_with("\"\""), "line: {line}");
        assert!(line.ends_with("\"\""), "line: {line}");
    }

    #[test]
    fn only_direct_skills_binaries_are_version_checked() {
        let direct = CliCommand {
            program: PathBuf::from("skills"),
            prefix_args: Vec::new(),
            display: "skills".into(),
        };
        let via_npx = CliCommand {
            program: PathBuf::from("npx.cmd"),
            prefix_args: vec!["--yes".into(), "skills@1.7.6".into()],
            display: "npx skills@1.7.6".into(),
        };
        assert!(is_direct_skills(&direct));
        assert!(!is_direct_skills(&via_npx));
    }
}
