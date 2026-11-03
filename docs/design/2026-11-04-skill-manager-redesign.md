# Skill Manager 重做设计 — 基于 skills.sh 生态

> 日期: 2026-11-04
> 范围: `src-tauri/src/skills/`、技能相关 IPC、`src-ui` 技能管理页
> 类型: 模块重做（设计文档）
> 状态: 待确认

## 1. 背景与目标

当前 `SkillManager` 是一套自建体系：

- 以 `skill.yaml` 定义技能；
- 只把 `type: script` 的技能包装成 `ScriptTool` 注册进 `ToolRegistry`，供本应用聊天代理调用；
- 用 SQLite `skills` 表保存记录，扫描 `~/.agent/skills`、`~/.agents/skills` 后 `reconcile`；
- 市场页只借用 skills.sh 的 `/api/search`，安装时直接执行 `npx skills add <source> -y -g`。

这套实现与 skills.sh 生态的实际模型不一致：skills.sh 的技能是 **`SKILL.md` 指令包**，由各类编码代理（OpenCode、Claude Code、Codex、Cursor 等）各自从固定目录加载；`skills` npm 包负责发现、安装（canonical 目录 + 指向各代理目录的 symlink/copy）、更新与卸载。

### 目标

1. 把自建技能管理改为 **skills.sh / `skills` CLI 的原生托管层**。
2. 安装、更新、卸载、清单全部以 `skills` CLI 与磁盘为唯一事实源。
3. 明确服务对象：只管理 **外部编码代理** 的技能，不接入本应用 Rig 代理的运行时。
4. 删除自建 `skill.yaml` / `ScriptTool` / SQLite 技能表 / reconcile 体系，消除双事实源。

### 非目标

- 不实现本应用聊天代理的技能按需加载（preamble 注入、`skill` 工具等）。
- 不实现私有仓库认证 UI；沿用本机 git / `gh` 凭据（CLI 已有能力）。
- 不实现 skills.sh 的 pack 创建与发布。
- 不改造 `skills` CLI 本身，不内嵌 Node 运行时。
- 不在本模块提供「启用/禁用」语义：外部代理会加载目录内全部技能，卸载才是有效操作。

### 反目标

- 出现「数据库说已安装、磁盘上没有」或反过来的状态。
- 自己重新实现 git 拉取、symlink、lock 文件逻辑。
- 让用户以为安装的技能会影响本应用自己的聊天代理。

## 2. 关键事实（已核实）

| 事实 | 结论 |
|------|------|
| `skills` npm 包 v1.7.6，要求 Node ≥ 22.20 | 启动时检测，缺依赖给可操作提示，不自动安装运行时 |
| `skills list --json` 实测输出：`name / path / scope / agents / source / sourceUrl / sourceType / status`，**不含描述与版本** | 清单以它为准；描述/版本需读 `SKILL.md`，缺失时留空 |
| `skills find` 是 fzf 式交互 TUI | 不能程序化调用；搜索继续用同一个 `/api/search` API |
| `skills add <source> --list` 输出带 ANSI 的人类可读列表 | 只能尽力解析为「名称 + 描述」；解析失败时回退为手动输入技能名 |
| `-a universal` 会展开为所有直接读取 `.agents/skills` 的代理 | 不做默认值；安装时传明确的代理 ID，避免范围歧义 |
| canonical 目录是 `<base>/.agents/skills`，全局 `~/.agents/skills` | 代理目录通过 symlink/copy 指向 canonical |
| `skills add` 支持 `-g`、`-a`、`-s`、`--copy`、`-y`、`--list` | 安装与预览都可非交互执行 |
| `skills remove` 支持技能名、`-g`、`-a`、`-y` | 卸载按名称 + 范围执行 |
| `skills check` 在「有更新」或「检查失败」时都可能退出码 1；`skills update -y` 非交互更新 | 需要解析输出区分 `has_updates` 与 `has_failures`，不能只看退出码 |

## 3. 方案选择

### 方案 A：纯 CLI + 磁盘为唯一事实源（已选）

- 清单：`skills list --json`（全局与项目各执行一次，合并）。
- 元数据：读取清单返回路径下的 `SKILL.md` frontmatter（name/description/version/author/tags）。
- 搜索/榜单：skills.sh `/api/search`。
- 变更：`skills add / remove / check / update`。
- 删除自建体系与 SQLite 技能表。

优点：单一事实源、行为与生态一致、后端显著变薄。
代价：每次刷新启动一次 Node；依赖 Node/npx；CLI 不可用时无法列出或变更，只能给出安装提示。

### 方案 B：CLI + SQLite 缓存（未选）

保留技能表做 UI 缓存。列表快、可离线，但会出现双写漂移，需要迁移旧表和 reconcile，复杂度回到现状。

### 方案 C：纯文件管理（未选）

自己实现 git 拉取、归档、symlink、lock。无 Node 依赖，但等于重写 `skills` CLI，违背「以 skills 包为基础工具」的前提。

## 4. 总体架构

```
React SkillManagerPage
        │ invoke
        ▼
commands::skill (IPC)
        │
        ▼
skills::SkillsService
  ├── cli.rs        npx/ skills 解析、进程执行、超时、输出
  ├── list.rs       `skills list --json` 解析 + SKILL.md 补全
  ├── frontmatter  SKILL.md YAML frontmatter 解析
  ├── market.rs     skills.sh /api/search
  └── agents.rs     代理静态注册表（完整支持列表）
        │
        ▼
skills CLI ──► ~/.agents/skills ──symlink──► 各代理 skills 目录
```

`SkillsService` 无状态，只持有 CLI 解析结果与超时配置；不再持有 `Database` 与 `ToolRegistry`。

## 5. 后端设计

### 5.1 模块结构

```text
src-tauri/src/skills/
├── mod.rs          # 类型、SkillsService façade
├── cli.rs          # 命令解析与执行
├── list.rs         # 清单解析与 SKILL.md 补全
├── frontmatter.rs  # SKILL.md frontmatter / 正文
├── market.rs       # skills.sh 搜索
└── agents.rs       # 代理注册表（id / 显示名 / 项目路径 / 全局路径）
```

删除：`loader.rs`（skill.yaml）、`scanner.rs`（reconcile 扫描）、旧 `SkillManager` 实现。

### 5.2 CLI 解析（cli.rs）

解析顺序（按顺序探测，第一个可用即停止）：

1. PATH 上的 `skills`（Windows 依次尝试 `skills.cmd` / `skills.exe`），且 `skills --version` ≥ 1.7.6；
2. `npx`（PATH 上的 `npx.cmd` / `npx`）；
3. fnm 默认别名：Windows `%APPDATA%\fnm\aliases\default\npx.cmd`；Unix `~/.local/share/fnm/aliases/default/bin/npx`；
4. 都不可用 → 返回 `AppError::Skill`，消息包含「需要 Node.js ≥ 22.20 与 npx」。

调用 npx 时显式带上 Node 所在目录到子进程 `PATH`，避免 `.cmd` shim 找不到 `node`；仍失败时退回 `cmd.exe /c npx ...`（沿用现有 `market.rs` 的 Windows 兜底思路）。

`get_skills_env()` 只做本地探测（PATH / 常见安装路径 / `--version`），超时 8s，不主动联网下载。`npx --yes skills@1.7.6` 的首次下载发生在真正执行安装/列表时，由页面异步 spinner 承担。

执行约定：

- 参数数组传递，永不拼接 shell 字符串。
- `NO_COLOR=1`；Windows 下加 `CREATE_NO_WINDOW`。
- 默认设置 `DISABLE_TELEMETRY=1` 与 `DO_NOT_TRACK=1`（隐私默认；后续可由设置页放开）。
- 超时：列表 60s（首次 npx 下载留余量），安装/更新 180s，卸载 60s，探测 8s。
- 返回 `CliOutput { stdout, stderr, success, code }`；失败时优先返回 stderr 的可读部分。
- `project_path` 作为进程 `current_dir`；全局操作不需要。
- 失败或超时后**不自动回滚**（CLI 的增量写入无法安全撤销，自动 remove 可能误删既有技能）；统一重新执行清单查询，把半成品如实展示，并提供卸载入口。

### 5.3 数据模型

```rust
pub struct CliStatus {
    pub available: bool,
    pub cli_version: Option<String>,
    pub node_version: Option<String>,
    pub command: Option<String>,   // 实际使用的命令形态
    pub error: Option<String>,
}

pub struct InstalledSkill {
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub author: Option<String>,
    pub tags: Option<Vec<String>>,
    pub path: Option<String>,      // "missing" 时为 None
    pub scope: String,             // "global" | "project"
    pub agents: Vec<String>,       // CLI 返回的 display name
    pub source: Option<String>,    // owner/repo
    pub source_url: Option<String>,
    pub source_type: Option<String>,
    pub status: String,            // "tracked" | "untracked" | "missing"
    pub has_skill_md: bool,
}

pub struct AgentOption {
    pub id: String,                // CLI 的 --agent 值
    pub name: String,              // 显示名
    pub project_path: String,      // 如 .agents/skills
    pub global_path: Option<String>,
}

pub struct RepoSkill {            // `skills add <source> --list`
    pub name: String,
    pub description: Option<String>,
}

pub struct RepoSkillPreview {
    pub skills: Vec<RepoSkill>,
    pub raw: String,               // 解析失败时供前端展示/手动选择
}

pub struct LegacySkill {          // 旧体系遗留目录
    pub name: String,
    pub path: String,
    pub importable: bool,          // 含 SKILL.md
    pub reason: Option<String>,    // 为什么不能导入（如仅有 skill.yaml）
}

pub struct LegacyImportResult {
    pub imported: Vec<String>,
    pub failed: Vec<LegacyImportFailure>,
}

pub struct LegacyImportFailure {
    pub path: String,
    pub error: String,
}

pub struct UpdateCheck {
    pub has_updates: bool,
    pub has_failures: bool,
    pub output: String,
}
```

### 5.4 清单与详情

`list_installed(scope, project_path)`：

1. `scope=global`：在任意目录执行 `skills list -g --json`。
2. `scope=project`：若 `project_path` 有效，则以它为 cwd 执行 `skills list --json`。
3. 结果按 `(scope, name)` 去重。JSON 从第一个 `[` 开始截取，容忍 CLI 前置提示。
4. 对 `status=tracked/untracked` 且 `path` 非空的条目，读取 `path/SKILL.md` frontmatter 补全描述/版本；解析失败不丢弃条目，`has_skill_md=false`。
5. `status=missing`（锁文件有、磁盘没有）的条目保留并标记为「缺失」；`path=None`，详情不可读，前端只提供「重新安装」或「从清单移除」。
6. 未知 `status` 按 `untracked` 处理，避免 CLI 升级后整页失败。

`get_skill_content(name, scope, project_path)`：

- 先走清单找到 canonical 路径，再读取 `SKILL.md` 全文（限制 256 KiB），返回给前端预览。

`preview_source(source)`：

- 执行 `skills add <source> --list`（`NO_COLOR=1`），先剥离 ANSI，再按「缩进较浅的行为技能名、下一行缩进较深的行为描述」解析为 `RepoSkill`。
- 该输出不是稳定接口，解析失败时返回空列表与原始输出；前端回退为「手动输入技能名」或「安装该仓库全部技能」。

### 5.5 变更操作

| 操作 | CLI |
|------|-----|
| 安装 | `skills add <source> -s <skill...> -a <agent...> -y [-g] [--copy]` |
| 安装该仓库全部技能 | `skills add <source> -s '*' -a <已选代理...> -y [-g] [--copy]` |
| 安装到所有代理（危险） | `skills add <source> --all`，仅作为独立二次确认操作 |
| 卸载 | `skills remove <name...> -y [-g]`（不传 `-a`，移除该技能在所有代理中的链接） |
| 检查更新 | `skills check [-g]`；退出码 1 且输出含 `update` → `has_updates`，否则退出码非 0 → `has_failures` |
| 更新 | `skills update -y [-g] [-p]` |

安装参数约束：

- `source` 拒绝以 `-` 开头、含控制字符、空串；允许 `owner/repo`、`http(s)://`、`git@`、`ssh://`、已存在的本地路径。
- 技能名与代理名必须来自白名单或 CLI 返回值。
- 至少选择一个代理；未选择时前端阻止提交。
- 不使用 `-a universal` 作为默认值：它会把技能散到所有通用代理，范围不可控；必须传明确代理 ID。

### 5.6 IPC 命令

替换现有技能命令：

```text
get_skills_env() -> CliStatus
list_skill_agents() -> Vec<AgentOption>
list_installed_skills(scope, projectPath) -> Vec<InstalledSkill>
get_skill_content(name, scope, projectPath) -> String
preview_skill_source(source) -> RepoSkillPreview
detect_legacy_skills() -> Vec<LegacySkill>
import_legacy_skills(paths, agents, copy) -> LegacyImportResult
install_skills(source, skills, agents, scope, projectPath, copy) -> String
remove_skill(name, scope, projectPath) -> String
check_skill_updates(scope, projectPath) -> UpdateCheck
update_skills(scope, projectPath) -> String
list_market_top_skills(limit) -> Vec<MarketSkill>
search_market_skills(query, limit) -> Vec<MarketSkill>
```

删除：`get_skill_detail`、`install_skill_from_path`、`uninstall_skill`、`toggle_skill`、`configure_skill`、`reconcile_skills`。

### 5.7 旧代码退役

- 删除 `skills::{loader, scanner}` 与旧 `SkillManager`。
- 删除 `db::models::SkillRecord`、`Database` 的 skill CRUD、迁移中 `skills` 表的读写路径。
- **不执行** `DROP TABLE skills`：避免破坏性迁移；旧表成为遗留数据，不再读写。
- 删除 `tools/script_tool.rs` 与 `tools/mod.rs` 中的声明（仅被旧技能体系使用）。
- `AppState.skills` 改为 `SkillsService`（无锁或轻量锁），`lib.rs` 启动阶段不再 cleanup/reconcile/sync。
- 更新 `docs/agents/overview.md`、`docs/agents/traps.md`、`docs/roadmap/mcp-and-pipeline.md` 中关于 SkillManager/ScriptTool 的描述。

### 5.8 旧数据检测与导入

旧体系可能留下两类目录：

- `%APPDATA%/agent/skills`（`dirs::data_dir()/agent/skills`，旧 `install_from_path` 的复制目标）；
- `~/.agent/skills`（旧 scanner 的扫描目录，单数）。

处理原则：**只读检测、绝不删除源目录**。

- `detect_legacy_skills()` 扫描两组目录的子目录：含 `SKILL.md` → `importable=true`；仅有 `skill.yaml` → `importable=false`，`reason="旧格式（skill.yaml），不符合 Agent Skills 规范"`。
- `import_legacy_skills(paths, agents, copy)` 对每个可导入目录执行 `skills add <绝对路径> -a <代理...> -g -y [--copy]`，由 CLI 写入全局 canonical 目录并建立链接；目标已存在时由 CLI 报错，UI 展示并跳过。
- 源目录保留在原处，用户确认后可自行清理；模块不提供自动删除。
- 页面顶部横幅：检测到可导入技能时显示「发现 N 个旧技能」+「导入」入口；仅检测到不兼容技能时显示说明与路径。横幅可在本次会话内关闭。

## 6. 前端设计

### 6.1 信息架构

`SkillManagerPage` 三个视图：

1. **已安装**
   - 顶部：范围切换（全局 / 项目）、代理筛选、刷新。
   - 项目范围需要选择项目目录（复用 `listProjects()`）；未选择时展示空态引导。
   - 卡片：名称、描述、版本、代理标签、来源链接、路径、查看 / 卸载；`status=missing` 单独标记「缺失」，只提供重新安装 / 从清单移除。
   - 无启用开关，无配置表单。
2. **市场**
   - 搜索框 + 榜单；沿用 `/api/search`。
   - 安装抽屉：来源、可安装技能多选（`preview_source`）、目标代理多选、范围、是否复制（默认 symlink）。
   - 「全部技能」= 该仓库所有技能装到**已选代理**（`-s '*' -a ...`）；「装到所有代理」是独立危险操作，需二次确认。
   - 安装完成后刷新已安装清单。
3. **更新**
   - 检查更新（按当前范围）、全部更新。
   - 展示 CLI 原始输出摘要；失败时保留完整输出可展开。

全局页头显示 CLI 状态：可用时显示版本；不可用时显示可操作提示与命令。检测到旧技能时在页头下方显示导入横幅。

导入对话框：列出可导入/不兼容的旧技能，选择目标代理，复用安装的 scope=global 与 `--copy` 选项；导入完成后刷新清单。不兼容项只展示原因，不提供操作。

### 6.2 详情

- 右侧抽屉展示 `SKILL.md` 全文（`react-markdown` + `remark-gfm`，复用 `MessageBubble` 的渲染方式）。
- 元信息：路径、范围、`status`、代理、来源（可打开 skills.sh 页面）。
- `status=missing` 时不请求正文，展示「磁盘上已不存在」与重新安装 / 移除入口。
- 卸载需二次确认。

### 6.3 状态

`skillSlice` 重写为：

```ts
{
  cliStatus: CliStatus | null;
  scope: 'global' | 'project';
  projectPath: string | null;
  installed: InstalledSkill[];
  legacySkills: LegacySkill[];
  marketSkills: MarketSkill[];
  skillContent: string | null;   // 当前详情抽屉的 SKILL.md
  loading: boolean;
  busy: string | null;        // 正在执行的操作标识
  error: string | null;
  updates: UpdateCheck | null;
}
```

移除 `installDialogOpen`、`selectedSkillDetail`、`reconciling` 等旧状态。

## 7. 错误与边界

| 场景 | 行为 |
|------|------|
| 未安装 Node / npx | 返回 `CliStatus.available=false`；页面显示「需要 Node.js ≥ 22.20」，不自动安装 |
| CLI 版本过旧或缺 `--json` | 回退到 `npx skills@1.7.6` |
| 网络失败（搜索） | 保留上次列表；toast 显示失败原因 |
| 安装失败 / 超时 | 保留 CLI 输出；立即重新查询清单，如实展示半成品并允许卸载；不回滚 |
| `SKILL.md` 缺失或 frontmatter 损坏 | 仍显示条目，描述回退为空，详情标注「无法解析」 |
| 清单出现 `status=missing` | 卡片标记「缺失」，禁止预览，提供重新安装 / 从清单移除 |
| 项目目录不存在 | 后端返回明确错误；前端项目范围空态 |
| 卸载一个被多个代理链接的技能 | 交给 CLI 处理 agent 链接；UI 只按名称与范围调用 |
| 安装耗时较长 | spinner + 180s 超时；超时后按「安装失败」处理 |
| `check` 退出码 1 但无法区分更新/失败 | 输出含 `update` 计为 `has_updates`，否则计为 `has_failures`；UI 分别提示并展示原始输出 |
| 旧技能导入时目标已存在 | CLI 报错；UI 标记该条失败并跳过，其余继续；源目录不动 |
| 旧目录仅有 `skill.yaml` | 标记为不兼容，不执行任何导入 |
| 路径含空格/中文 | 参数数组传递，不经过 shell |

## 8. 测试与验证

### 后端单元测试

- CLI 参数构造：安装/卸载/更新/列表的范围与代理参数。
- `list --json` 解析：正常、前置噪声、空数组、缺字段、`path: null` 的 `missing` 条目、未知 `status`。
- `SKILL.md` frontmatter：完整、缺 frontmatter、非法 YAML。
- `source` 校验：合法 shorthand/URL/本地路径；拒绝 `-` 开头与空串。
- 旧技能检测：含 `SKILL.md`、仅 `skill.yaml`、空目录、不存在的根目录。
- 不启动真实 CLI 的纯函数测试优先；真实 CLI 测试标记 `#[ignore]`。

### 前端检查

- `npx tsc --noEmit`。
- 手工路径：CLI 可用 / 不可用、全局安装、项目安装、市场搜索、安装指定技能、预览、卸载、检查更新。

### 验收标准

1. 已安装列表与 `npx skills list` 结果一致（名称、范围、代理、`status`）。
2. 从市场安装一个真实技能后，无需手动扫描即可出现在列表，且目标代理目录出现链接/副本。
3. 卸载后 canonical 目录与代理链接都被移除。
4. 未安装 Node 时页面给出可操作提示，不崩溃。
5. 仓库中不再有 `skill.yaml` 解析、`ScriptTool` 注册、skills 表读写代码。
6. 存在旧目录时能检测到并列出；导入后新列表出现该技能，源目录保持不变。

## 9. 实施顺序

1. 新建 `cli.rs` / `list.rs` / `frontmatter.rs` / `agents.rs`，配套单元测试。
2. 重写 `skills::mod.rs` 与 IPC 命令，更新 `state.rs` / `lib.rs` 注册。
3. 删除旧体系（loader/scanner/ScriptTool/DB 读写），加入旧目录检测/导入。
4. 前端类型、API、slice 重写。
5. `SkillManagerPage` 与详情/安装抽屉重做（UI 由 designer 负责）。
6. 文档更新与端到端手工验证。

## 10. 已定的设计决定

- 代理注册表内嵌约 20 个常用代理（id / 显示名 / 项目路径 / 全局路径）：OpenCode、Claude Code、Codex、Cursor、Gemini CLI、GitHub Copilot、Windsurf、Amp、Cline、Continue、Goose、Kilo、Roo、Zed、Qwen、Kimi、Trae、Augment、OpenHands、Universal；另提供「自定义代理 id」文本输入，避免为长尾代理维护全表。
- 安装默认使用 symlink；暴露「复制文件」开关，Windows 上 symlink 失败时提示切换。
- 更新页首版只做整体检查/更新；`check` 的输出用于判断 `has_updates` / `has_failures`，不展示逐技能操作。
- 清单保留 `missing` 条目并标注，不静默过滤。
- 安装失败不回滚，改为重新查询清单并如实展示。
- 遥测默认关闭（`DISABLE_TELEMETRY=1` / `DO_NOT_TRACK=1`）。
- 旧数据只检测与提示导入，不删除源目录；仅含 `skill.yaml` 的目录标注为不兼容。

## 11. 留给实施者的决定

- CLI 探测结果是否在进程内缓存（建议首次探测后缓存 5 分钟；失败不缓存）。
- 安装抽屉中代理列表的默认勾选（建议：上次成功安装的代理；无记录时仅勾选 OpenCode）。
- `SKILL.md` 正文中相对链接/图片的处理（首版按纯 Markdown 渲染，不解析仓库内资源）。
