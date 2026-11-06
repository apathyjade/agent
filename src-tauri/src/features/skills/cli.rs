//! 官方 `skills` CLI（skills.sh）的薄封装。
//!
//! 所有调用都由参数数组构造——CLI 从不通过 shell 字符串执行。子进程会关闭
//! 颜色与遥测，Windows 下不弹出控制台窗口。

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::core::error::{AppError, Result};

/// 固定的 CLI 版本——本模块针对该版本构建。
pub const SKILLS_CLI_VERSION: &str = "1.7.6";
/// CLI 要求的最低 Node.js 版本。
pub const MIN_NODE_VERSION: (u64, u64, u64) = (22, 20, 0);
/// 探测超时：只做本地检查，绝不下载。
const PROBE_TIMEOUT: Duration = Duration::from_secs(8);
/// `CREATE_NO_WINDOW` —— 避免 CLI shim 闪出控制台窗口。
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 本地环境探测结果（显示在技能页面）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliStatus {
    pub available: bool,
    pub cli_version: Option<String>,
    pub node_version: Option<String>,
    pub command: Option<String>,
    pub error: Option<String>,
}

/// 解析出的 CLI 调用方式：直接 `skills` 或 `npx skills@x`。
#[derive(Debug, Clone)]
pub struct CliCommand {
    pub program: PathBuf,
    pub prefix_args: Vec<String>,
    pub display: String,
}

/// 一次 CLI 调用的捕获结果。
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

/// 解析 CLI 命令，首个命中者胜出：
/// 1. PATH 上的 `skills`；2. PATH 上的 `npx`；3. fnm 默认别名目录；
/// 4. 裸 `npx`（后续 spawn 失败时由 `probe` 报告）。
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

    // 最后手段：交给进程启动器通过 PATH 解析 `npx`。
    npx_command(PathBuf::from(if cfg!(windows) { "npx.cmd" } else { "npx" }))
}

/// 把解析出的命令翻译成具体的 `(program, argv)` 对。
///
/// Windows 的 `.cmd` / `.bat` shim 不能直接 spawn，因此包一层
/// `cmd.exe /c`。shim 路径会加引号，使 `cmd.exe` 即使遇到含空格的路径
/// （`C:\Program Files\nodejs\npx.cmd`）也只看到一个 token。
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

/// 以给定参数运行 CLI。
///
/// `cwd` 用于项目级操作；全局操作传 `None`。子进程的 PATH 会前置 Node
/// 安装目录，使 `.cmd` shim 在 GUI 进程 PATH 里没有 Node 时也能找到
/// `node`。
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

/// 探测本地环境，不下载任何内容。
///
/// 检查 `node --version` 与 `<cli> --version`；失败不会返回错误，而是把可
/// 操作的提示写进 [`CliStatus::error`]。
pub async fn probe(command: &CliCommand) -> CliStatus {
    let node_dir = node_dir_for(command);
    let mut status = CliStatus {
        available: false,
        cli_version: None,
        node_version: None,
        command: Some(command.display.clone()),
        error: None,
    };

    // ── Node.js 环境 ──
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

    // npx 形式固定使用 `skills@1.7.6`，在这里运行会在冷缓存时访问网络。
    // 只要 Node 可用即可判定为可用；版本由固定版本的调用本身验证。
    if !is_direct_skills(command) {
        status.available = true;
        return status;
    }

    // ── skills CLI（仅直接二进制） ──
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

/// 把 `1.7.6`、`v1.7.6` 或 `v22.20.0` 解析成可比较的元组。
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

/// 在来源进入 CLI 前拒绝空、类参数、含引号与控制字符的值。
///
/// 字面量 `"` 被拒绝，因为 `\"` 不是 cmd.exe 的转义：在 Windows 的
/// `cmd.exe /c` 路径上，引号后跟 `&`/`|` 可能突破参数引号。`&`、`?`、`=`
/// 对 URL 仍保持有效。
pub fn is_valid_source(source: &str) -> bool {
    if source.chars().any(char::is_control) || source.contains('"') {
        return false;
    }
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return false;
    }
    // 开头的 `-` 会被 CLI 当作参数。
    !trimmed.starts_with('-')
}

// ── 内部实现 ──

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
        // Windows 上由 `tokio::process::Command` 自身提供。
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

/// 把 `args` 传给子进程，并对 `cmd.exe /c` 包装做特殊处理。
///
/// `spawn_parts` 已给 shim 路径加引号；[`build_cmd_line`] 会给其余每个 token
/// 加引号并用 `raw_arg` 追加，使 `cmd.exe` 永远看不到来自用户来源或技能名的
/// 未加引号 `&`、`|`、`^`、`<`、`>`。用 raw 追加还能让引号保持字面量，而
/// 不会被 Windows argv 构造器转义成 `\"`（cmd.exe 无法理解）。
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

/// 组装 `cmd.exe` 命令行，每个 token 都加双引号。
///
/// `tokens[0]` 是 [`spawn_parts`] 已加过引号的 shim 路径；先去掉这层引号，
/// 避免结果出现双重引号。内嵌引号按 cmd 风格转义为 `\"`。整行再包一层
/// 引号：`cmd /c` 解析前会剥掉最外层，剩下的带引号 token 因而保持完整。
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

/// 解析出的程序是 `skills` 二进制而非 npx shim 时返回 true。
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

/// fnm 稳定的 "default" 别名目录，存放 Node shim。
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

/// 必须出现在子进程 PATH 上的目录（也用于定位 `node`）。
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
            "https://x/a?b=1&c=2",
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
        assert!(!is_valid_source("https://x/a\"&calc"));
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
        // cmd /c 会剥掉最外层引号，使完整带引号的命令行保持原样。
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
