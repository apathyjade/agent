//! 技能管理服务。
//!
//! 读写两侧都委托给官方 `skills` CLI：读取侧解析 `list --json` 输出并合并
//! `SKILL.md` frontmatter，写入侧负责安装、删除、检查更新与旧版技能导入。
//! 磁盘状态由 CLI 拥有，本模块不做本地状态缓存（环境探测除外）。

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::core::error::{AppError, Result};

pub mod agents;
pub mod cli;
pub mod frontmatter;
pub mod legacy;
pub mod list;
pub mod market;
pub use list::InstalledSkill;

// ── 读取侧：基于 CLI 的技能服务 ──

/// 一次成功环境探测的缓存时长。
const ENV_CACHE_TTL: Duration = Duration::from_secs(300);
/// 只读 CLI 调用的超时。给得较宽，因为首次 `npx` 可能需要填充冷缓存。
const CLI_LIST_TIMEOUT: Duration = Duration::from_secs(60);
/// `add --list` 预览的超时（可能预热冷 npx 缓存）。
const CLI_PREVIEW_TIMEOUT: Duration = Duration::from_secs(60);
/// 安装与更新操作的超时。
const CLI_MUTATION_TIMEOUT: Duration = Duration::from_secs(180);
/// 删除操作的超时。
const CLI_REMOVE_TIMEOUT: Duration = Duration::from_secs(60);
/// 单次 `SKILL.md` 读取上限：256 KiB。
const MAX_CONTENT_BYTES: u64 = 256 * 1024;

/// 一次 `skills add` 调用的入参。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallRequest {
    pub source: String,
    pub skills: Vec<String>,
    pub agents: Vec<String>,
    pub scope: String,
    pub project_path: Option<String>,
    pub copy: bool,
}

/// 仓库来源提供的一个技能。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoSkill {
    pub name: String,
    pub description: Option<String>,
}

/// 安装前预览仓库来源的结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoSkillPreview {
    pub skills: Vec<RepoSkill>,
    /// 已去除 ANSI 转义的 CLI 输出；作为手动录入的回退数据。
    pub raw: String,
}

/// 一次 `skills check` 的结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheck {
    pub has_updates: bool,
    pub has_failures: bool,
    pub output: String,
}

/// 已安装技能的只读视图，后端是官方 `skills` CLI。
///
/// CLI 拥有磁盘状态；本服务只解析其 JSON 输出并合并 `SKILL.md`
/// frontmatter 元数据。
pub struct SkillsService {
    cli: cli::CliCommand,
    env_cache: Mutex<Option<(Instant, cli::CliStatus)>>,
}

impl SkillsService {
    /// 解析 CLI 命令并创建空的环境缓存。
    pub fn new() -> Self {
        Self {
            cli: cli::resolve_command(),
            env_cache: Mutex::new(None),
        }
    }

    /// 报告 CLI / Node 可用性。
    ///
    /// 成功探测结果缓存五分钟；失败结果不缓存，使环境修复后能立刻被发现。
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

    /// 列出某个作用域下已安装的技能。
    ///
    /// 项目作用域会校验 `project_path` 并把它作为 CLI 工作目录；全局作用域
    /// 忽略该参数。
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

        // 补充元数据会为每个条目访问文件系统，因此即使 CLI 调用是 await 的，
        // 也要放到阻塞线程池，避免占用异步运行时。
        tokio::task::spawn_blocking(move || {
            entries.into_iter().map(list::enrich).collect::<Vec<_>>()
        })
        .await
        .map_err(|err| AppError::Skill(format!("skill enrichment task failed: {err}")))
    }

    /// 读取一个已安装技能的 `SKILL.md` 正文。
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

// ── 写入侧：通过 CLI 安装、删除、检查与更新 ──

impl SkillsService {
    /// 询问 CLI 某个仓库来源提供哪些技能。
    ///
    /// 列表格式面向人类、不是稳定接口：无法解析或运行失败时返回空
    /// `skills` 加原始输出，界面回退为「安装整个仓库」。
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

    /// 从某个来源安装一个或多个技能。
    pub async fn install(&self, req: InstallRequest) -> Result<String> {
        let scope = normalize_scope(&req.scope)?;
        let args = build_add_args(&req)?;
        let cwd = project_workdir(scope, req.project_path.as_deref())?;

        let output = cli::run(&self.cli, &args, cwd.as_deref(), CLI_MUTATION_TIMEOUT).await?;
        accept_mutation(output)
    }

    /// 从指定作用域删除一个技能。
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

    /// 检查技能更新。
    ///
    /// 退出码 1 是 CLI 契约的一部分（有更新或检查失败），因此不当作错误；
    /// 两个标志由 [`interpret_check`] 从合并输出中推导。
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

    /// 更新指定作用域下所有已安装技能。
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

/// 列表调用使用的 `--json` 参数。未知作用域回退为项目形式；调用方应先执行
/// [`normalize_scope`]。
pub fn build_list_args(scope: &str) -> Vec<String> {
    match scope {
        "global" => vec!["list".to_string(), "-g".to_string(), "--json".to_string()],
        _ => vec!["list".to_string(), "--json".to_string()],
    }
}

/// 按 CLI 要求的顺序构造 `add` 参数。
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
    // 技能名与 agent 名会成为 argv 项，因此 `-g` 这类类参数值或空字符串
    // 绝不能传到 CLI。
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
        // CLI 会把 `*` 展开为来源里的全部技能。
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

/// 构造 `remove` 参数。不带 `-a`：删除会解除技能与所有 agent 的关联。
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

/// 构造 `check` 参数。
///
/// 项目作用域显式传 `-p`：不传时 CLI 会自动探测作用域，并在项目没有技能
/// 时静默回退到全局技能。
pub fn build_check_args(scope: &str) -> Result<Vec<String>> {
    let scope = normalize_scope(scope)?;
    let mut args = vec!["check".to_string()];
    match scope {
        "global" => args.push("-g".to_string()),
        _ => args.push("-p".to_string()),
    }
    Ok(args)
}

/// 构造 `update` 参数。项目作用域传 `-p`，原因同 [`build_check_args`]。
pub fn build_update_args(scope: &str) -> Result<Vec<String>> {
    let scope = normalize_scope(scope)?;
    let mut args = vec!["update".to_string(), "-y".to_string()];
    match scope {
        "global" => args.push("-g".to_string()),
        _ => args.push("-p".to_string()),
    }
    Ok(args)
}

/// 解析 `skills add <source> --list` 打印的技能列表。
///
/// 应用内以管道方式调用 CLI，因此会观察到两种渲染：
///
/// * 带边框的多行行：名称一行，随后是缩进更深的描述行；
/// * 单行行：`name   description` 在同一行。
///
/// 两者前面都可能有标题（`Available Skills`）与噪声（`Source:`、
/// `Fetching`、`Found N skills` 等）。存在 `Available Skills` 标题时从标题
/// 之后开始解析，否则考虑所有非噪声行。标题与噪声行永远不会被当作技能返回。
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

        // 单行形式：第一段连续 2 个以上空格分隔名称与描述。
        if let Some((name, description)) = split_inline_row(text) {
            skills.push(RepoSkill {
                name: name.to_string(),
                description: Some(description.to_string()),
            });
            i += 1;
            continue;
        }

        // 多行形式：下一行缩进更深，说明本行是名称，后续更深的行是它的
        // （可能换行的）描述。
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

        // 与标题同深度的裸行是残留装饰，不是技能。
        if header_depth.is_some_and(|header| *depth <= header) {
            i += 1;
            continue;
        }

        // 技能深度上的其他行按「只有名称、没有描述」处理。
        skills.push(RepoSkill {
            name: text.clone(),
            description: None,
        });
        i += 1;
    }

    skills
}

/// 在第一段连续两个以上空格处切分单行列表行。
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

/// 列表起始标题，包括 `Available Skills:`、`Available Skills (5)` 等变体。
fn is_available_skills_header(text: &str) -> bool {
    text.trim().to_lowercase().starts_with("available skills")
}

/// 绝不能被误认为技能的标题/进度行。
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

/// 纯边框/分隔线：只含制表符、破折号或空格。
fn is_rule_line(text: &str) -> bool {
    text.chars()
        .all(|c| c.is_whitespace() || c == '-' || ('\u{2500}'..='\u{257F}').contains(&c))
}

/// CLI 在每行内容前加的边框符号。单行渲染以 `ℹ` 开头。
fn is_frame_glyph(c: char) -> bool {
    matches!(c, '│' | '└' | '├' | '◇' | '●' | '•' | 'ℹ')
}

/// 去掉行首的边框符号，同时保留符号之后的正文缩进。
///
/// 符号前的空格会随符号一起消耗；若后面没有符号，则保留原始前导空格，
/// 使纯缩进的测试样例仍具有可度量的深度。
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

/// 把 `skills check` 的结果归类为 `(has_updates, has_failures)`。
///
/// CLI 用退出码 1 同时表示「有更新」与「检查失败」。失败关键词
/// （`failed`、`error`、`denied`、`cannot`）优先于 `update` 一词，因此
/// "failed to check for updates" 判为失败；否则非零退出且输出提到 `update`
/// 判为有更新，其余非零退出判为失败。退出码缺失（进程被杀）始终判为失败。
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

/// 只接受界面暴露的两个作用域。
pub fn normalize_scope(scope: &str) -> Result<&str> {
    match scope {
        "global" | "project" => Ok(scope),
        other => Err(AppError::SkillValidation(format!(
            "unknown skill scope '{}': expected 'global' or 'project'",
            other
        ))),
    }
}

/// 列表调用的工作目录；项目作用域必须是真实目录。
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

/// 同时按名称与作用域查找已安装技能。
///
/// CLI 可能在两个作用域里报告同名技能，只按名称匹配会返回先出现的那一行。
/// 请求的作用域才是权威。
fn find_target<'a>(
    skills: &'a [InstalledSkill],
    name: &str,
    scope: &str,
) -> Option<&'a InstalledSkill> {
    skills
        .iter()
        .find(|skill| skill.name == name && skill.scope == scope)
}

/// 判断一次 `skills list` 调用是否可用。
///
/// 退出码 0 始终接受。非零退出只有在仍产出至少一个条目时才接受；失败运行
/// 返回空列表在界面上与「没有安装技能」无法区分，因此作为错误返回并附带
/// CLI 失败细节。
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

/// 每个 `(scope, name)` 组合只保留第一行。
fn dedupe_entries(entries: Vec<list::RawListEntry>) -> Vec<list::RawListEntry> {
    let mut seen = std::collections::HashSet::new();
    entries
        .into_iter()
        .filter(|entry| seen.insert((entry.scope.clone(), entry.name.clone())))
        .collect()
}

/// 拒绝空、类参数、含引号与控制字符的技能/agent 名称。
///
/// 拒绝引号的原因与 [`cli::is_valid_source`] 相同：`\"` 不是 cmd.exe 的
/// 转义，因此含 `"` 且带 `&`/`|` 的名称可能在 Windows `cmd.exe /c` 路径上
/// 突破引号。
fn is_valid_name(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty()
        && !trimmed.starts_with('-')
        && !name.contains('"')
        && !name.chars().any(char::is_control)
}

/// 接受一次写调用：成功返回 stdout，否则返回带 CLI 诊断的错误。
/// 写操作没有值得保留的「部分成功」语义；调用方事后会重新列出以显示真实
/// 状态。
fn accept_mutation(output: cli::CliOutput) -> Result<String> {
    if output.success {
        return Ok(output.stdout.trim().to_string());
    }

    Err(AppError::Skill(mutation_failure_detail(&output)))
}

/// 优先用 stderr，其次 stdout，最后退出码。
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

/// 用换行连接 stdout 与 stderr（两者都有时）。
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

/// 去除 CLI 输出中的 ANSI 转义序列（CSI 与 OSC）。
///
/// 每次调用都设置了 `NO_COLOR=1`，但 npx 与部分 shim 在 stdout 是管道时
/// 仍会输出颜色，因此解析前先清洗。
fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }

        match chars.peek().copied() {
            // CSI：ESC [ ... 结束字节在 0x40..=0x7e
            Some('[') => {
                chars.next();
                for next in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&next) {
                        break;
                    }
                }
            }
            // OSC：ESC ] ... BEL 或 ESC \
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
            // 其他转义序列：丢弃 ESC 及其后一个字符。
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }

    out
}

/// 描述一次失败 CLI 调用的单行可操作信息。
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

/// 读取技能的 `SKILL.md`，超过 256 KiB 时截断。
///
/// `path` 可以是技能目录或文件本身。文件缺失或不可读时返回
/// [`AppError::NotFound`]。
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
        // 成功运行可能合法地报告没有技能。
        assert!(accept_listing(true, vec![], "ignored".into())
            .unwrap()
            .is_empty());

        // 失败但仍有输出的运行保持可用。
        let entries = vec![raw_entry("one", "global", Some("p".into()))];
        assert_eq!(
            accept_listing(false, entries, "ignored".into())
                .unwrap()
                .len(),
            1
        );

        // 失败且输出为空的运行必须暴露错误。
        let err = accept_listing(false, vec![], "boom".into()).unwrap_err();
        assert!(matches!(err, AppError::Skill(_)));
        assert!(err.to_string().contains("boom"));
    }

    #[test]
    fn project_scope_requires_an_existing_directory() {
        // 全局作用域从不传工作目录。
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
