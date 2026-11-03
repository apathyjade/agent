# Skill Manager 重做 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把自建 `skill.yaml` 技能体系替换为以 `skills` CLI 与 skills.sh 为基础的外部编码代理技能管理模块。

**Architecture:** 后端新增无状态 `SkillsService`，通过 `skills list --json` 获取清单、读取 `SKILL.md` 补全元数据、用 `skills add/remove/check/update` 执行变更；前端重写为「已安装 / 市场 / 更新」三视图。SQLite 技能表、`ScriptTool`、`skill.yaml` 解析与 reconcile 全部退役。

**Tech Stack:** Rust 1.77.2（edition 2021）、Tauri 2、tokio、serde、reqwest；React 19 + TypeScript 5.4 + Zustand 4 + Tailwind + react-markdown；`skills` CLI v1.7.6（Node ≥ 22.20）。

**Spec:** `docs/design/2026-11-04-skill-manager-redesign.md`

## Global Constraints

- Node 运行时要求 **≥ 22.20**；CLI 固定调用 `skills@1.7.6`。应用不得自动安装 Node/npm/fnm。
- 所有 CLI 调用使用参数数组，禁止拼接 shell 字符串；`source` 必须拒绝 `-` 开头、控制字符与空串。
- 子进程环境固定 `NO_COLOR=1`、`DISABLE_TELEMETRY=1`、`DO_NOT_TRACK=1`；Windows 使用 `CREATE_NO_WINDOW`。
- 服务对象仅限外部编码代理；不得向本应用聊天代理的 preamble 注入技能，也不得注册动态工具。
- 不再读写 SQLite `skills` 表；不执行 `DROP TABLE`。
- 不删除旧技能目录；旧数据只检测与提示导入。
- 代码注释、变量名、commit message 使用英文；界面文案使用中文。
- 每个任务结束必须运行该任务的验证命令并提交。

## Review Focus

- **GUI 进程 PATH 中没有 Node/npx**（本机由 fnm 管理）：检测应回退到 `%APPDATA%\fnm\aliases\default\npx.cmd`，且调用 `.cmd` 时通过 `cmd.exe /c`；见 Task 1。
- **`skills list --json` 前有横幅/ANSI 噪声**：解析必须从第一个 `[` 开始，不能假设纯 JSON；见 Task 3。
- **锁文件有、磁盘没有的条目**（`path: null`, `status: "missing"`）：不得反序列化失败或崩溃；见 Task 3 与 Task 10。
- **`SKILL.md` 缺失或 frontmatter 损坏**：清单仍要显示该技能，描述留空；见 Task 2 与 Task 3。
- **恶意/误输入的 source**（`-g`、`--all`）：必须在进入 CLI 前拒绝；见 Task 1 与 Task 6。
- **项目路径含空格或中文**：作为 `current_dir` 传递，不经过 shell；见 Task 5。
- **安装超时后磁盘已有半成品**：失败后重新查询清单，不回滚、不静默隐藏；见 Task 6 与 Task 10。

---

## Task 1: CLI 解析与执行

**Files:**
- Create: `src-tauri/src/skills/cli.rs`
- Modify: `src-tauri/src/skills/mod.rs`（仅加 `pub mod cli;`）
- Test: 同文件内 `#[cfg(test)]`

**Interfaces:**
- Produces:
  - `pub struct CliStatus { pub available: bool, pub cli_version: Option<String>, pub node_version: Option<String>, pub command: Option<String>, pub error: Option<String> }`
  - `pub struct CliCommand { pub program: PathBuf, pub prefix_args: Vec<String>, pub display: String }`
  - `pub struct CliOutput { pub stdout: String, pub stderr: String, pub success: bool, pub code: Option<i32> }`
  - `pub fn resolve_command() -> CliCommand`
  - `pub fn parse_version(raw: &str) -> Option<(u64, u64, u64)>`
  - `pub fn is_valid_source(source: &str) -> bool`
  - `pub async fn probe(command: &CliCommand) -> CliStatus`
  - `pub async fn run(command: &CliCommand, args: &[String], cwd: Option<&Path>, timeout: Duration) -> Result<CliOutput>`
  - `pub fn spawn_parts(command: &CliCommand) -> (String, Vec<String>)`（Windows `.cmd` 包装，纯函数便于测试）

- [ ] **Step 1: 写失败测试**

```rust
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
        assert!(args[1].ends_with("npx.cmd"));
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd src-tauri && cargo test skills::cli::tests -- --nocapture`
Expected: 编译失败，`is_valid_source` 等未定义。

- [ ] **Step 3: 实现 `cli.rs`**

要点：
- `resolve_command()` 依次找：PATH `skills.cmd/.exe/skills` → PATH `npx.cmd/.exe/npx` → fnm 默认别名 `%APPDATA%\fnm\aliases\default\npx.cmd` / `~/.local/share/fnm/aliases/default/bin/npx` → `npx`。
- npx 形态的 `prefix_args = ["--yes", "skills@1.7.6"]`；直接 skills 形态为空。
- `spawn_parts`：扩展名为 `cmd`/`bat` 时返回 `("cmd.exe", ["/c", program, ...prefix, ...args])`，否则 `(program, [...prefix, ...args])`。
- `run`：设置 `NO_COLOR`、`DISABLE_TELEMETRY`、`DO_NOT_TRACK`；把 Node 目录前置到 `PATH`；Windows 加 `std::os::windows::process::CommandExt::creation_flags(0x08000000)`；`tokio::time::timeout`，超时返回 `AppError::Skill("... timed out after Ns")`。
- `probe`：跑 `<cmd> --version` 与 `node --version`，填充 `CliStatus`；失败时 `available=false`，`error` 写可操作提示（含 Node ≥ 22.20）。

- [ ] **Step 4: 运行测试确认通过**

Run: `cd src-tauri && cargo test skills::cli::tests -- --nocapture`
Expected: 4 个测试通过（Windows 上 4 个）。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/skills/cli.rs src-tauri/src/skills/mod.rs
git commit -m "feat(skills): add skills CLI runner and environment probe"
```

---

## Task 2: SKILL.md frontmatter 解析

**Files:**
- Create: `src-tauri/src/skills/frontmatter.rs`
- Modify: `src-tauri/src/skills/mod.rs`（加 `pub mod frontmatter;`）
- Test: 同文件内 `#[cfg(test)]`

**Interfaces:**
- Produces:
  - `pub struct SkillMd { pub name: Option<String>, pub description: Option<String>, pub version: Option<String>, pub author: Option<String>, pub tags: Option<Vec<String>>, pub body: String }`
  - `pub fn parse(content: &str) -> SkillMd`

- [ ] **Step 1: 写失败测试**

```rust
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
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd src-tauri && cargo test skills::frontmatter::tests`
Expected: 编译失败，`parse` 未定义。

- [ ] **Step 3: 实现 `frontmatter.rs`**

要点：复用 `serde_yaml::Value`；frontmatter 以 `---` 开始、以换行后的 `---` 结束；解析失败时字段全 `None`，`body` 返回原文。`tags` 兼容数组与逗号分隔字符串。

- [ ] **Step 4: 运行测试确认通过**

Run: `cd src-tauri && cargo test skills::frontmatter::tests`
Expected: 4 passed。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/skills/frontmatter.rs src-tauri/src/skills/mod.rs
git commit -m "feat(skills): parse SKILL.md frontmatter"
```

---

## Task 3: `list --json` 解析与条目补全

**Files:**
- Create: `src-tauri/src/skills/list.rs`
- Modify: `src-tauri/src/skills/mod.rs`（加 `pub mod list;`）
- Test: 同文件内 `#[cfg(test)]`

**Interfaces:**
- Consumes: `frontmatter::parse`、`SkillMd`。
- Produces:
  - `pub struct InstalledSkill { pub name: String, pub description: String, pub version: Option<String>, pub author: Option<String>, pub tags: Option<Vec<String>>, pub path: Option<String>, pub scope: String, pub agents: Vec<String>, pub source: Option<String>, pub source_url: Option<String>, pub source_type: Option<String>, pub status: String, pub has_skill_md: bool }`
  - `pub struct RawListEntry { pub name: String, pub path: Option<String>, pub scope: String, pub agents: Vec<String>, pub source: Option<String>, pub source_url: Option<String>, pub source_type: Option<String>, pub status: String }`
  - `pub fn parse_entries(stdout: &str) -> Result<Vec<RawListEntry>>`
  - `pub fn enrich(entry: RawListEntry) -> InstalledSkill`
  - `pub fn read_skill_md(path: &Path) -> Option<SkillMd>`

- [ ] **Step 1: 写失败测试**

```rust
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
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd src-tauri && cargo test skills::list::tests`
Expected: 编译失败。

- [ ] **Step 3: 实现 `list.rs`**

要点：
- `parse_entries` 先 `find('[')`、`rfind(']')` 截取；`serde_json::from_str`。
- `RawListEntry` 用 `#[serde(rename = "sourceUrl")]` / `"sourceType"`；`status` 缺省为 `untracked`；未知状态映射为 `untracked`。
- `enrich`：`status == "missing"` 时 `has_skill_md=false`、描述空；否则读 `path/SKILL.md`，用 frontmatter 补 `description/version/author/tags`；`name` 保留 CLI 名称。
- `read_skill_md` 限制 256 KiB，读失败返回 `None`。

- [ ] **Step 4: 运行测试确认通过**

Run: `cd src-tauri && cargo test skills::list::tests`
Expected: 3 passed。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/skills/list.rs src-tauri/src/skills/mod.rs
git commit -m "feat(skills): parse installed skills JSON and enrich from SKILL.md"
```

---

## Task 4: 代理注册表

**Files:**
- Create: `src-tauri/src/skills/agents.rs`
- Modify: `src-tauri/src/skills/mod.rs`（加 `pub mod agents;`）
- Test: 同文件内 `#[cfg(test)]`

**Interfaces:**
- Produces:
  - `pub struct AgentOption { pub id: String, pub name: String, pub project_path: String, pub global_path: Option<String> }`
  - `pub fn all() -> &'static [AgentOption]`

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_expected_agents_and_unique_ids() {
        let agents = all();
        assert!(agents.len() >= 20);
        assert!(agents.iter().any(|a| a.id == "opencode"));
        assert!(agents.iter().any(|a| a.id == "claude-code"));
        let mut ids: Vec<_> = agents.iter().map(|a| a.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), agents.len());
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd src-tauri && cargo test skills::agents::tests`
Expected: 编译失败。

- [ ] **Step 3: 实现 `agents.rs`**

内嵌 20 个常用代理（来自 `https://github.com/vercel-labs/skills/blob/main/docs/agents.md`）：`opencode`、`claude-code`、`codex`、`cursor`、`gemini-cli`、`github-copilot`、`windsurf`、`amp`、`cline`、`continue`、`goose`、`kilo`、`roo`、`zed`、`qwen-code`、`kimi-code-cli`、`trae`、`augment`、`openhands`、`universal`。每项写 `id / name / project_path / global_path`，未知全局路径填 `None`。

- [ ] **Step 4: 运行测试确认通过**

Run: `cd src-tauri && cargo test skills::agents::tests`
Expected: 1 passed。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/skills/agents.rs src-tauri/src/skills/mod.rs
git commit -m "feat(skills): add agent target registry"
```

---

## Task 5: SkillsService 读取侧

**Files:**
- Modify: `src-tauri/src/skills/mod.rs`（重写主体）
- Modify: `src-tauri/src/skills/market.rs`（仅删除 `install_market_skill` / `install_via_cmd` / `resolve_npx`）
- Test: `src-tauri/src/skills/mod.rs` 内 `#[cfg(test)]`

**Interfaces:**
- Consumes: `cli::{resolve_command, run, probe, CliCommand, CliOutput, CliStatus}`、`list::{parse_entries, enrich, InstalledSkill}`、`frontmatter::parse`。
- Produces:
  - `pub struct SkillsService { ... }`
  - `impl SkillsService { pub fn new() -> Self; pub async fn env_status(&self) -> CliStatus; pub async fn list_installed(&self, scope: &str, project_path: Option<&str>) -> Result<Vec<InstalledSkill>>; pub async fn content(&self, name: &str, scope: &str, project_path: Option<&str>) -> Result<String>; }`
  - `pub fn build_list_args(scope: &str) -> Vec<String>`
  - `pub fn normalize_scope(scope: &str) -> Result<&str>`（只接受 `global` / `project`）

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

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
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd src-tauri && cargo test skills::tests::builds_list_args_for_scope`
Expected: 编译失败。

- [ ] **Step 3: 实现读取侧**

要点：
- `SkillsService { cli: CliCommand, cache: tokio::sync::Mutex<Option<(Instant, CliStatus)>> }`；`new()` 用 `resolve_command()`；`env_status()` 缓存 5 分钟。
- `list_installed`：`normalize_scope`；项目范围校验 `project_path` 存在且是目录；执行 `run`；`parse_entries`；`enrich`；按 `(scope, name)` 去重。
- `content`：先 `list_installed` 找条目；`path` 为 `None` → `AppError::NotFound`；读取 `SKILL.md`，超过 256 KiB 截断。
- 删除 `market.rs` 中 `resolve_npx`、`install_market_skill`、`install_via_cmd`，只保留搜索。

- [ ] **Step 4: 运行测试确认通过**

Run: `cd src-tauri && cargo test skills::tests`
Expected: 3 passed。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/skills/mod.rs src-tauri/src/skills/market.rs
git commit -m "feat(skills): add read-side SkillsService backed by the CLI"
```

---

## Task 6: SkillsService 变更侧

**Files:**
- Modify: `src-tauri/src/skills/mod.rs`
- Test: 同文件内 `#[cfg(test)]`

**Interfaces:**
- Produces:
  - `pub struct InstallRequest { pub source: String, pub skills: Vec<String>, pub agents: Vec<String>, pub scope: String, pub project_path: Option<String>, pub copy: bool }`
  - `pub struct RepoSkill { pub name: String, pub description: Option<String> }`
  - `pub struct RepoSkillPreview { pub skills: Vec<RepoSkill>, pub raw: String }`
  - `pub struct UpdateCheck { pub has_updates: bool, pub has_failures: bool, pub output: String }`
  - `impl SkillsService { pub async fn preview_source(&self, source: &str) -> Result<RepoSkillPreview>; pub async fn install(&self, req: InstallRequest) -> Result<String>; pub async fn remove(&self, name: &str, scope: &str, project_path: Option<&str>) -> Result<String>; pub async fn check_updates(&self, scope: &str, project_path: Option<&str>) -> Result<UpdateCheck>; pub async fn update(&self, scope: &str, project_path: Option<&str>) -> Result<String>; }`
  - `pub fn build_add_args(req: &InstallRequest) -> Result<Vec<String>>`
  - `pub fn build_remove_args(name: &str, scope: &str) -> Result<Vec<String>>`
  - `pub fn build_check_args(scope: &str) -> Result<Vec<String>>`
  - `pub fn build_update_args(scope: &str) -> Result<Vec<String>>`
  - `pub fn parse_available_skills(stdout: &str) -> Vec<RepoSkill>`
  - `pub fn interpret_check(code: Option<i32>, output: &str) -> (bool, bool)`

- [ ] **Step 1: 写失败测试**

```rust
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
            vec!["add", "vercel-labs/agent-skills", "-s", "web-design-guidelines",
                 "-a", "opencode", "claude-code", "-y", "-g"]
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
    fn builds_remove_check_update_args() {
        assert_eq!(build_remove_args("x", "global").unwrap(), vec!["remove", "x", "-y", "-g"]);
        assert_eq!(build_check_args("project").unwrap(), vec!["check"]);
        assert_eq!(build_update_args("global").unwrap(), vec!["update", "-y", "-g"]);
    }

    #[test]
    fn parses_available_skills_output() {
        let raw = "\n  vercel-composition-patterns\n    React composition patterns that scale.\n  deploy-to-vercel\n    Deploy applications and websites.\n\n  Use --skill <name> to install specific skills\n";
        let skills = parse_available_skills(raw);
        assert_eq!(skills.len(), 2);
        assert_eq!(skills[0].name, "vercel-composition-patterns");
        assert_eq!(skills[0].description.as_deref(), Some("React composition patterns that scale."));
    }

    #[test]
    fn interprets_check_exit_codes() {
        assert_eq!(interpret_check(Some(1), "x has an update available"), (true, false));
        assert_eq!(interpret_check(Some(1), "failed to check y"), (false, true));
        assert_eq!(interpret_check(Some(0), "all good"), (false, false));
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd src-tauri && cargo test skills::mutation_tests`
Expected: 编译失败。

- [ ] **Step 3: 实现变更侧**

要点：
- `build_add_args`：校验 `is_valid_source`、agents 非空、scope；`skills` 为空时用 `"*"`；参数顺序固定为 `add / source / -s / skills... / -a / agents... / -y / [-g] / [--copy]`。
- `preview_source`：`run(["add", source, "--list"])`；剥离 ANSI 后 `parse_available_skills`；失败时返回空 `skills` 与 `raw=stdout+stderr`。
- `install`：执行 CLI；失败返回 `AppError::Skill(stderr or stdout)`；成功返回 stdout。
- `remove` / `update` 同理。
- `check_updates`：退出码非 0 不算错误；用 `interpret_check` 生成结果，`output` 为 stdout + stderr。
- 项目范围必须传 `current_dir`；全局范围不传。

- [ ] **Step 4: 运行测试确认通过**

Run: `cd src-tauri && cargo test skills::mutation_tests`
Expected: 6 passed。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/skills/mod.rs
git commit -m "feat(skills): add install, remove, update and preview operations"
```

---

## Task 7: 旧技能检测与导入

**Files:**
- Create: `src-tauri/src/skills/legacy.rs`
- Modify: `src-tauri/src/skills/mod.rs`（加 `pub mod legacy;`）
- Test: 同文件内 `#[cfg(test)]`

**Interfaces:**
- Produces:
  - `pub struct LegacySkill { pub name: String, pub path: String, pub importable: bool, pub reason: Option<String> }`
  - `pub struct LegacyImportResult { pub imported: Vec<String>, pub failed: Vec<LegacyImportFailure> }`
  - `pub struct LegacyImportFailure { pub path: String, pub error: String }`
  - `pub fn roots() -> Vec<PathBuf>`
  - `pub fn scan_root(root: &Path) -> Vec<LegacySkill>`
  - `pub fn build_import_args(path: &str, agents: &[String], copy: bool) -> Result<Vec<String>>`
  - `impl SkillsService { pub async fn detect_legacy(&self) -> Result<Vec<LegacySkill>>; pub async fn import_legacy(&self, paths: &[String], agents: &[String], copy: bool) -> Result<LegacyImportResult>; }`

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn scans_importable_and_incompatible_skills() {
        let dir = tempfile::tempdir().unwrap();
        let md = dir.path().join("good");
        fs::create_dir_all(&md).unwrap();
        fs::write(md.join("SKILL.md"), "---\nname: good\n---\nBody").unwrap();

        let yaml = dir.path().join("legacy");
        fs::create_dir_all(&yaml).unwrap();
        fs::write(yaml.join("skill.yaml"), "id: legacy").unwrap();

        let found = scan_root(dir.path());
        assert_eq!(found.len(), 2);
        let good = found.iter().find(|s| s.name == "good").unwrap();
        assert!(good.importable);
        let legacy = found.iter().find(|s| s.name == "legacy").unwrap();
        assert!(!legacy.importable);
        assert!(legacy.reason.is_some());
    }

    #[test]
    fn missing_root_is_empty() {
        assert!(scan_root(Path::new("Z:\\does-not-exist\\skills")).is_empty());
    }

    #[test]
    fn builds_global_import_args() {
        let args = build_import_args("/tmp/good", &["opencode".into()], true).unwrap();
        assert_eq!(args, vec!["add", "/tmp/good", "-a", "opencode", "-g", "-y", "--copy"]);
    }

    #[test]
    fn rejects_import_without_agents() {
        assert!(build_import_args("/tmp/good", &[], false).is_err());
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd src-tauri && cargo test skills::legacy::tests`
Expected: 编译失败。

- [ ] **Step 3: 实现 `legacy.rs`**

要点：
- `roots()`：`dirs::data_dir()/agent/skills`、`dirs::home_dir()/.agent/skills`，去重。
- `scan_root`：遍历一级子目录；`SKILL.md` 存在 → `importable=true`；否则 `skill.yaml` 存在 → `importable=false, reason=Some("旧格式（skill.yaml），不符合 Agent Skills 规范")`；两者都无跳过。
- `detect_legacy`：合并所有 root，按 `path` 去重。
- `build_import_args`：校验 agents 非空；参数顺序 `add / path / -a / agents... / -g / -y / [--copy]`。
- `import_legacy`：对每个 path 校验存在且含 `SKILL.md`；用 `build_import_args` 执行；成功 push 到 `imported`，失败 push 到 `failed` 并继续。

- [ ] **Step 4: 运行测试确认通过**

Run: `cd src-tauri && cargo test skills::legacy::tests`
Expected: 2 passed。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/skills/legacy.rs src-tauri/src/skills/mod.rs
git commit -m "feat(skills): detect and import legacy skill directories"
```

---

## Task 8: IPC、状态与旧后端退役

**Files:**
- Modify: `src-tauri/src/commands/skill.rs`（整体重写）
- Modify: `src-tauri/src/state.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/db/models.rs`（删除 `SkillRecord`）
- Modify: `src-tauri/src/db/repository.rs`（删除 skills 表创建、CRUD、`skill_exists`）
- Delete: `src-tauri/src/skills/loader.rs`
- Delete: `src-tauri/src/skills/scanner.rs`
- Delete: `src-tauri/src/tools/script_tool.rs`
- Modify: `src-tauri/src/tools/mod.rs`
- Verify: `cargo check`

**Interfaces:**
- Consumes: `SkillsService` 全部公开方法。
- Produces: Tauri commands：
  - `get_skills_env`
  - `list_skill_agents`
  - `list_installed_skills(scope: Option<String>, project_path: Option<String>)`
  - `get_skill_content(name: String, scope: String, project_path: Option<String>)`
  - `preview_skill_source(source: String)`
  - `detect_legacy_skills`
  - `import_legacy_skills(paths: Vec<String>, agents: Vec<String>, copy: bool)`
  - `install_skills(source, skills, agents, scope, project_path, copy)`
  - `remove_skill(name, scope, project_path)`
  - `check_skill_updates(scope, project_path)`
  - `update_skills(scope, project_path)`

- [ ] **Step 1: 重写 `commands/skill.rs`**

每个命令保持薄层，直接调用 `state.skills.*`；`scope` 缺省时按 `"global"` 处理；`install_skills` 构造 `InstallRequest`。保留 `list_market_top_skills` 与 `search_market_skills`，删除 `install_market_skill`。

- [ ] **Step 2: 更新 `state.rs`**

`pub skills: SkillsService`；`AppState::new` 中 `let skills = SkillsService::new();`，移除 `Arc<Mutex<...>>` 与 `db_arc/tools_arc` 传入。

- [ ] **Step 3: 更新 `lib.rs`**

删除 setup 中「1. Initialize skills」整段；在 `generate_handler!` 中用新命令替换旧技能命令。

- [ ] **Step 4: 删除旧代码**

- 删除 `skills/loader.rs`、`skills/scanner.rs`，从 `skills/mod.rs` 移除 `pub mod loader; pub mod scanner; pub use loader...` 与全部旧 `SkillManager` 实现。
- 删除 `tools/script_tool.rs` 与 `tools/mod.rs` 的 `pub mod script_tool;`。
- 删除 `db/models.rs::SkillRecord`。
- 删除 `db/repository.rs` 中 `CREATE TABLE ... skills`、`insert_skill`、`get_skill`、`list_skills`、`update_skill_*`、`delete_skill`、`delete_skills_by_source_type`、`skill_exists`。
- 保留 SQLite 中已存在的表，不做迁移/删除。

- [ ] **Step 5: 编译检查**

Run: `cd src-tauri && cargo check`
Expected: 编译通过，无 `SkillRecord` / `ScriptTool` / `loader` / `scanner` 未定义引用。

- [ ] **Step 6: 提交**

```bash
git add -A src-tauri/src
git commit -m "refactor(skills): replace SkillManager with CLI-backed service"
```

---

## Task 9: 前端类型与 API

**Files:**
- Modify: `src-ui/src/types/index.ts`
- Modify: `src-ui/src/api/management.ts`
- Verify: `cd src-ui && npx tsc --noEmit`

**Interfaces:**
- Produces（TS）：
  - `SkillScope = 'global' | 'project'`
  - `CliStatus`、`InstalledSkill`、`AgentOption`、`RepoSkill`、`RepoSkillPreview`、`LegacySkill`、`LegacyImportResult`、`UpdateCheck`
  - `api`：`getSkillsEnv`、`listSkillAgents`、`listInstalledSkills(scope, projectPath?)`、`getSkillContent(name, scope, projectPath?)`、`previewSkillSource(source)`、`detectLegacySkills()`、`importLegacySkills(paths, agents, copy)`、`installSkills(...)`、`removeSkill(name, scope, projectPath?)`、`checkSkillUpdates(scope, projectPath?)`、`updateSkills(scope, projectPath?)`
  - 保留 `listMarketTopSkills` / `searchMarketSkills`

- [ ] **Step 1: 更新 `types/index.ts`**

删除 `SkillInfo`、`SkillDetail`、`ReconcileResult`、旧 `AgentSource`；新增设计 §5.3 对应接口（字段保持 snake_case，与 Rust 序列化一致）。Rust 的 `Option<T>` 字段在 TS 中一律写成可选属性（`?`），测试数据可以省略。

- [ ] **Step 2: 更新 `api/management.ts`**

删除旧技能函数；新增上述 invoke 包装；注意 Tauri 参数用 camelCase（`projectPath`、`copy`）。

- [ ] **Step 3: 类型检查**

Run: `cd src-ui && npx tsc --noEmit`
Expected: 仅剩 `SkillManagerPage` / `SkillDetailPanel` / `skillSlice` 的旧引用错误（下一步修复）。

- [ ] **Step 4: 提交**

```bash
git add src-ui/src/types/index.ts src-ui/src/api/management.ts
git commit -m "feat(ui): add skill manager types and IPC bindings"
```

---

## Task 10: 前端 skillSlice

**Files:**
- Modify: `src-ui/src/store/skillSlice.ts`（整体重写）
- Create: `src-ui/src/store/__tests__/skillSlice.test.ts`
- Test: `cd src-ui && npx vitest run src/store/__tests__/skillSlice.test.ts`

**Interfaces:**
- Consumes: Task 9 的 `api` 与类型。
- Produces `SkillSlice`：`cliStatus / scope / projectPath / installed / legacySkills / marketSkills / skillContent / loading / busy / error / updates`，以及 `fetchEnv / fetchAgents / fetchInstalled / fetchContent / installSkill / removeSkill / checkUpdates / updateAll / fetchLegacy / importLegacy / fetchMarketTopSkills / searchMarketSkills / setScope / setProjectPath`。

- [ ] **Step 1: 写失败测试**

```ts
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { create } from 'zustand';

vi.mock('../../api/tauri', () => ({
  getSkillsEnv: vi.fn().mockResolvedValue({ available: true }),
  listInstalledSkills: vi.fn().mockResolvedValue([{ name: 'x', description: '', agents: [], status: 'tracked' }]),
  listSkillAgents: vi.fn().mockResolvedValue([]),
  listMarketTopSkills: vi.fn().mockResolvedValue([]),
  searchMarketSkills: vi.fn().mockResolvedValue([]),
  installSkills: vi.fn().mockResolvedValue('ok'),
  removeSkill: vi.fn().mockResolvedValue('ok'),
  checkSkillUpdates: vi.fn().mockResolvedValue({ has_updates: false, has_failures: false, output: '' }),
  updateSkills: vi.fn().mockResolvedValue('ok'),
  detectLegacySkills: vi.fn().mockResolvedValue([]),
  importLegacySkills: vi.fn().mockResolvedValue({ imported: [], failed: [] }),
  previewSkillSource: vi.fn().mockResolvedValue({ skills: [], raw: '' }),
}));

import { createSkillSlice, type SkillSlice } from '../skillSlice';

function testStore() {
  return create<SkillSlice>()((...args) => createSkillSlice(...args));
}

describe('skillSlice', () => {
  beforeEach(() => vi.clearAllMocks());

  it('loads installed skills', async () => {
    const store = testStore();
    await store.getState().fetchInstalled();
    expect(store.getState().installed).toHaveLength(1);
    expect(store.getState().loading).toBe(false);
  });

  it('refreshes after install', async () => {
    const api = await import('../../api/tauri');
    const store = testStore();
    await store.getState().installSkill({
      source: 'o/r', skills: ['x'], agents: ['opencode'], scope: 'global', copy: false,
    });
    expect(api.installSkills).toHaveBeenCalled();
    expect(api.listInstalledSkills).toHaveBeenCalled();
  });

  it('records errors without throwing', async () => {
    const api = await import('../../api/tauri');
    (api.listInstalledSkills as any).mockRejectedValueOnce(new Error('boom'));
    const store = testStore();
    await store.getState().fetchInstalled();
    expect(store.getState().error).toContain('boom');
  });
});
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd src-ui && npx vitest run src/store/__tests__/skillSlice.test.ts`
Expected: 编译失败或 `createSkillSlice` 未导出。

- [ ] **Step 3: 实现 slice**

要点：所有异步操作先 `set({ busy: ... })`，结束时清空；`installSkill` 完成后调用 `fetchInstalled`；`removeSkill` 同理；`fetchEnv` 缓存 `cliStatus`；`error` 使用 `String(err)`。

- [ ] **Step 4: 运行测试确认通过**

Run: `cd src-ui && npx vitest run src/store/__tests__/skillSlice.test.ts`
Expected: 3 passed。

- [ ] **Step 5: 提交**

```bash
git add src-ui/src/store/skillSlice.ts src-ui/src/store/__tests__/skillSlice.test.ts
git commit -m "feat(ui): rewrite skill store around the skills CLI"
```

---

## Task 11: 技能管理页面重做（UI 由 designer 负责）

**Files:**
- Modify: `src-ui/src/components/management/SkillManagerPage.tsx`（整体重写）
- Modify: `src-ui/src/components/management/SkillDetailPanel.tsx`（整体重写）
- Create: `src-ui/src/components/management/InstallSkillDrawer.tsx`
- Create: `src-ui/src/components/management/LegacySkillsBanner.tsx`
- Create: `src-ui/src/components/management/UpdatePanel.tsx`
- Create: `src-ui/src/components/management/skillUtils.ts`
- Create: `src-ui/src/components/management/__tests__/skillUtils.test.ts`
- Delete: `src-ui/src/components/management/SkillInstallDialog.tsx`
- Modify: `src-ui/src/components/settings/SettingsModal.tsx`（移除 skills 区块）
- Modify: `src-ui/src/components/settings/SettingsPage.tsx`（移除 skills 区块）
- Verify: `cd src-ui && npx tsc --noEmit && npx vitest run`

**Interfaces:**
- Consumes: `useStore` 的 `SkillSlice`、设计 §6 的信息架构。
- Produces:
  - `SkillManagerPage` 渲染三个视图（tabs）。
  - `SkillDetailPanel({ skill, onClose })`：`skill: InstalledSkill`。
  - `InstallSkillDrawer({ open, onClose })`。
  - `LegacySkillsBanner({ skills, onImport })`。
  - `UpdatePanel({ scope, projectPath })`。
  - `skillUtils.filterInstalled(skills, query, agentId): InstalledSkill[]`
  - `skillUtils.isInstalled(marketName, installed): boolean`

- [ ] **Step 1: 写失败测试（纯函数）**

```ts
import { describe, expect, it } from 'vitest';
import { filterInstalled, isInstalled } from '../skillUtils';
import type { InstalledSkill } from '../../../types';

const rows: InstalledSkill[] = [
  { name: 'find-skills', description: 'Find', path: '/a', scope: 'global', agents: ['OpenCode'], status: 'tracked', has_skill_md: true },
  { name: 'web-design', description: 'UI', path: '/b', scope: 'project', agents: ['Claude Code'], status: 'tracked', has_skill_md: true },
];

describe('skillUtils', () => {
  it('filters by query and agent', () => {
    expect(filterInstalled(rows, 'find', null).map((s) => s.name)).toEqual(['find-skills']);
    expect(filterInstalled(rows, '', 'OpenCode').map((s) => s.name)).toEqual(['find-skills']);
  });
  it('matches installed names case-insensitively', () => {
    expect(isInstalled('Find-Skills', rows)).toBe(true);
    expect(isInstalled('missing', rows)).toBe(false);
  });
});
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd src-ui && npx vitest run src/components/management/__tests__/skillUtils.test.ts`
Expected: 模块不存在。

- [ ] **Step 3: 实现 `skillUtils.ts`**

`filterInstalled` 对 `name/description/agents` 做小写包含匹配，`agentId` 为 `null` 时不过滤。`isInstalled` 只比较 `name` 小写。

- [ ] **Step 4: 运行测试确认通过**

Run: `cd src-ui && npx vitest run src/components/management/__tests__/skillUtils.test.ts`
Expected: 2 passed。

- [ ] **Step 5: 实现页面与子组件**

按设计 §6 实现：
- 页面：`ManagerPageLayout` + tabs（已安装/市场/更新）；页头右侧刷新与「安装技能」。
- 已安装：范围切换（全局/项目）、项目选择（`api.listProjects()`）、代理筛选、CLI 不可用空态、技能卡片（`status=missing` 标记）、详情抽屉。
- 市场：搜索/榜单；安装抽屉（source、`previewSkillSource` 结果多选、代理多选、自定义代理 id、scope、`copy` 开关）；「全部技能」= `-s '*'` 到已选代理；「所有代理」独立危险操作。
- 更新：`checkSkillUpdates` / `updateSkills`，分别提示 `has_updates` 与 `has_failures`，可展开原始输出。
- 旧数据：页头下方横幅与导入对话框。
- 详情：`react-markdown` + `remark-gfm` 渲染 `SKILL.md`；`missing` 时不请求正文。
- 无启用开关、无配置表单。

设置页清理：从 `SettingsModal.tsx`、`SettingsPage.tsx` 删除技能 tab/区块、`SkillDetailPanel` 引用、`skills/fetchSkills/toggleSkill` 解构与 `showSkillInstall` 状态；技能入口只保留在 `ModuleBar` 的技能管理页。删除不再被引用的 `SkillInstallDialog.tsx`。

UI 文案中文；视觉交由 designer；实现完成后用浏览器预览确认空态、长描述、代理标签换行、窄窗口。

- [ ] **Step 6: 全量前端验证**

Run: `cd src-ui && npx tsc --noEmit && npx vitest run`
Expected: 类型检查通过；现有测试 + 新增 2 个测试全部通过。

- [ ] **Step 7: 提交**

```bash
git add src-ui/src/components src-ui/src/store/skillSlice.ts
git commit -m "feat(ui): rebuild skill manager page for skills.sh"
```

---

## Task 12: 文档、清理与端到端验证

**Files:**
- Modify: `docs/agents/overview.md:146-155`
- Modify: `docs/agents/traps.md:30-34`
- Modify: `docs/roadmap/mcp-and-pipeline.md:184-201`
- Modify: `src-tauri/src/memory/seeds.rs:217`（种子文案去掉 `script_tool`）
- Verify: 全量命令

- [ ] **Step 1: 更新文档**

- `overview.md`：删除「动态脚本工具 / SkillManager」行，改为「技能（外部代理）| — | 由 `skills` CLI 安装到各代理目录，不注册为本应用工具」。
- `traps.md`：删除「动态工具通过 SkillManager 注册」，补充「技能管理只服务外部代理；清单来自 `skills list --json`」。
- `mcp-and-pipeline.md`：把「与现有 SkillManager 的关系」改为「与技能管理的关系：技能是 SKILL.md 指令包，MCP 是运行时工具，两者独立」。
- `seeds.rs`：把 `script_tool（脚本工具）` 从工具列表文案移除。

- [ ] **Step 2: 全量后端验证**

Run: `cd src-tauri && cargo test && cargo check`
Expected: 所有测试通过，无 warning（或仅既有 warning）。

- [ ] **Step 3: 全量前端验证**

Run: `cd src-ui && npx tsc --noEmit && npx vitest run && npm run build`
Expected: 全部通过。

- [ ] **Step 4: CLI 冒烟测试（手工）**

在临时目录执行：

```bash
npx --yes skills@1.7.6 add vercel-labs/skills -s find-skills -a opencode -y
npx --yes skills@1.7.6 list --json
npx --yes skills@1.7.6 remove find-skills -y
```

预期：安装后 JSON 出现 `find-skills`；`remove` 后为空。确认应用页面刷新后与 CLI 一致。

- [ ] **Step 5: 提交**

```bash
git add docs src-tauri/src/memory/seeds.rs
git commit -m "docs: document CLI-backed skill management"
```

---

## Self-Review

- Spec §5.1–5.8 分别由 Task 1–8 覆盖；§6 由 Task 9–11 覆盖；§8 验证由 Task 12 覆盖。
- 类型名在 Task 5/6 定义、Task 8 使用、Task 9 镜像为 TS；字段统一 snake_case。
- Review Focus 每一条都有对应测试：Task 1（PATH/source）、Task 3（噪声/missing）、Task 2（frontmatter）、Task 6（安装失败/超时后的清单刷新在 Task 10 测试）、Task 5（项目路径）。
- 计划长度约为设计的 1.3 倍，未逐行转录实现。
