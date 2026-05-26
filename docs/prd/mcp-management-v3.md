# Agent 产品需求文档 V3：MCP 管理平台

> 基于 V1 路线图 + V2 PRD 执行后的全面升级 | 创建：2026-05-21
> 覆盖方向：MCP 市场、快捷安装、启动管理、集成链接、安全管控

---

## 一、现状与目标

### 已完成（V1 + V2 基础）

| 模块 | 已实现 |
|------|--------|
| MCP 连接 | 添加/移除服务器、连接/断开、自动发现工具并注册到 ToolRegistry |
| 工具控制 | 逐工具启用/禁用开关、三种确认模式（auto_allow/confirm_once/deny） |
| 健康监控 | 定期 ping、调用统计（次数/延迟/错误率）、运行时长、自动重连（指数退避×3） |
| 模板库 | 内置 8 个常用模板（Filesystem, GitHub, PostgreSQL, Slack 等） |
| 配置持久化 | config.json 中的 `mcp_servers` 数组 |
| 前端管理页 | 连接列表、状态指示器（🟢🟡🔴）、工具开关、添加对话框 |

### 核心差距

| 维度 | 已有能力 | 缺失 |
|------|---------|------|
| **发现** | 手动输入命令 | 无市场浏览/搜索、无模板推荐 |
| **安装** | 手动填写 command/args/env | 无一键安装、无命令粘贴解析、无 MCPB 支持 |
| **启动** | auto_connect 开关 | 无启动顺序、无延迟启动、无按需启动、无热重载 |
| **集成** | 全局启用 | 无对话级别绑定、无工作流集成、无资源/提示词接入 |
| **安全** | 工具级确认模式 | 无资源访问控制、无审计日志、无环境变量管理 |
| **传输** | 仅 stdio | 无 Streamable HTTP 支持 |
| **管理** | 单个添加/删除 | 无导出/导入、无批量操作、无分享链接 |

### V3 目标

将 MCP 从"可配置的工具源"升级为**桌面端 MCP 管理平台**——用户像使用应用商店一样发现、安装、管理、链接 MCP 服务器。

---

## 二、功能规格

### ━━━ 模块 A：MCP 市场（Marketplace）━━━

#### A1. 市场浏览与搜索

**问题**：用户只能通过外部渠道（GitHub、博客）发现 MCP 服务器，再手动复制命令。

**需求**：内置市场浏览器，聚合多个来源，支持搜索和分类浏览。

```
┌───────────────────────────────────────────────────────┐
│  MCP 市场                            [搜索服务器...]   │
│                                                       │
│  精选模板 │ Smithery 市场 │ 官方 Registry │ 社区精选     │
│───────────────────────────────────────────────────────│
│                                                       │
│  ┌─ @anthropic/mcp-server-github ────── ⭐ 4.8k ──┐  │
│  │  GitHub API 集成 — 管理 Issue、PR、代码搜索       │  │
│  │  🔧 create_issue, search_repos, list_prs ...    │  │
│  │  ⚙️ 需要: GITHUB_TOKEN                          │  │
│  │                              [安装] [详情]        │  │
│  └──────────────────────────────────────────────────┘  │
│                                                       │
│  ┌─ @modelcontextprotocol/server-filesystem ── ⭐ 12k┐ │
│  │  安全的本地文件系统访问 — 读写文件、目录操作       │  │
│  │  🔧 read_file, write_file, list_dir, search ...   │  │
│  │  ⚙️ 需要: 工作目录                                │  │
│  │                              [安装] [详情]        │  │
│  └──────────────────────────────────────────────────┘  │
│                                                       │
│  ┌─ @anthropic/mcp-server-playwright ── ⭐ 3.2k ───┐ │
│  │  浏览器自动化 — 网页截图、点击、填写表单           │  │
│  │  🔧 screenshot, click, fill, navigate ...         │  │
│  │  ⚙️ 无需额外配置                                  │  │
│  │                              [安装] [详情]        │  │
│  └──────────────────────────────────────────────────┘  │
└───────────────────────────────────────────────────────┘
```

**实现要点**：
- 市场来源采用可插拔设计，初始集成：
  | 来源 | 集成方式 | 优先级 |
  |------|---------|--------|
  | **内置模板** | Rust 常量 `Vec<McpTemplate>` | Tab 1：精选模板 |
  | **Smithery.ai** | REST API（`api.smithery.ai/v1/servers`） | Tab 2：Smithery 市场 |
  | **npm registry** | 根据包名查询 `@modelcontextprotocol/*` + `@anthropic/*` | Tab 3：官方 |
  | **社区索引** | `awesome-mcp-servers` GitHub 列表 → 本地缓存 | Tab 4：社区 |
- 搜索结果缓存（本地 SQLite 表，避免重复请求）
- 详情页展示：工具列表、所需环境变量、标签、Star 数
- 分页加载（每页 20 条）

**验收**：打开 MCP 市场 → 看到精选模板 + Smithery 来源 → 搜索 "database" → 显示相关服务器列表

---

#### A2. 服务器详情与配置预览

**问题**：安装前无法预览该服务器会暴露哪些工具、需要哪些配置。

**需求**：点击服务器卡片进入详情页，展示完整信息。

```
┌───────────────────────────────────────────────────────┐
│  ← 返回市场                                           │
│                                                       │
│  @anthropic/mcp-server-github                         │
│  ════════════════════════════════════════════════════  │
│                                                       │
│  📝 描述                                              │
│  MCP 服务器，用于将 GitHub API 集成到 AI 工作流中。     │
│  支持仓库管理、Issue/PR 操作、代码搜索、用户信息查询。   │
│                                                       │
│  🔧 暴露工具（6 个）                                   │
│  ☐ create_issue     — 创建 Issue    ⚠️ 写操作         │
│  ☐ search_repos     — 搜索仓库      ✅ 安全            │
│  ☐ list_prs         — 列出 PR       ✅ 安全            │
│  ☐ merge_pr         — 合并 PR       ⚠️ 写操作         │
│  ☐ get_file_content — 获取文件内容  ✅ 安全            │
│  ☐ create_repo      — 创建仓库      ⚠️ 高风险          │
│                                                       │
│  ⚙️ 所需配置                                          │
│  ┌──────────────────────────────────────────────┐     │
│  │ GITHUB_TOKEN *   [·························] │     │
│  │ 有效期: 永久      🔒 AES 加密存储              │     │
│  └──────────────────────────────────────────────┘     │
│                                                       │
│  📊 来源: Smithery.ai | 更新: 2026-05-20              │
│                                                       │
│                    [取消]  [✓ 安装并连接]               │
└───────────────────────────────────────────────────────┘
```

**实现要点**：
- 工具列表默认按安全等级排序（安全→警告→高风险），**高风险工具默认禁用**
- 配置字段根据来源自动填充（Smithery API 返回的 `configSchema`，或模板预定义）
- 敏感字段（API key、Token）标记为密码输入框，提交时 AES 加密存储
- 安装前可预览和调整工具开关状态

**验收**：点击服务器详情 → 看到工具列表和配置表单 → 填写 Token → 点击安装 → 服务器自动添加并连接

---

#### A3. 一键安装流程

**问题**：安装一个 MCP 服务器需要手动填写 command/args/env，步骤多易出错。

**需求**：从详情页点击安装 → 自动生成完整配置 → 连接服务器。

**流程**：
```
[详情页 "安装" 按钮]
    │
    ├─ 有必填配置 → 弹出配置对话框 → 用户填写 → [确认安装]
    │                                        │
    │                                   ┌─────▼──────┐
    │                                   │ 执行安装:   │
    │                                   │ 1. 生成 ID  │
    │                                   │ 2. 保存配置 │
    │                                   │ 3. 启动连接 │
    │                                   │ 4. 注册工具 │
    │                                   │ 5. 更新 UI  │
    │                                   └─────┬──────┘
    │                                         │
    └─ 无必填配置 → 直接安装 ─────────────────┘
                                          │
                                     ┌────▼────┐
                                     │ 完成 ✓   │
                                     │ 状态: 🟢  │
                                     └─────────┘
```

**实现要点**：
- 安装即连接，无需额外步骤
- 安装失败时显示错误详情（进程启动失败/超时/协议版本不兼容）
- 安装成功后自动跳转到 MCP 管理页，高亮新服务器

**验收**：市场选择 GitHub 模板 → 填入 Token → 安装 → 管理页新增 GitHub 服务器且状态为 🟢 Ready

---

### ━━━ 模块 B：快捷安装（Quick Install）━━━

#### B1. 命令粘贴安装

**问题**：用户从文档或博客看到 `npx -y @xxx/yyy` 命令，希望快速配置。

**需求**：支持粘贴完整命令，自动解析为服务器配置。

```
┌──────────────────────────────────────────────┐
│  添加 MCP 服务器                              │
│                                               │
│  从市场 │ 从命令 │ 从模板 │ 导入配置 │ 手动      │
│──────────────────────────────────────────────│
│                                               │
│  粘贴 MCP 启动命令:                           │
│  ┌────────────────────────────────────────┐  │
│  │ npx -y @anthropic/mcp-server-github    │  │
│  └────────────────────────────────────────┘  │
│                                               │
│  ┌──────────────────── ────────────────────┐ │
│  │ ✅ 解析结果:                             │ │
│  │   命令: npx                              │ │
│  │   参数: -y @anthropic/mcp-server-github  │ │
│  │   自动检测: 需要 GITHUB_TOKEN 环境变量   │ │
│  │   建议名称: GitHub API                   │ │
│  └──────────────────────────────────────────┘│
│                                               │
│  GITHUB_TOKEN *  [·························]  │
│                                               │
│  [测试连接]  [取消]  [✓ 保存并连接]             │
└──────────────────────────────────────────────┘
```

**解析规则**：
| 输入模式 | 解析结果 | 示例 |
|---------|---------|------|
| `npx -y <pkg> [args]` | command: `npx`, args: `-y <pkg> ...` | `npx -y @anthropic/mcp-server-github` |
| `uvx <pkg> [args]` | command: `uvx`, args: `<pkg> ...` | `uvx mcp-server-git --repo ./` |
| `python <script> [args]` | command: `python`, args: `<script> ...` | `python path/to/server.py` |
| `docker run ...` | command: `docker`, args: `run -i --rm ...` | `docker run -i --rm ghcr.io/github/github-mcp-server` |
| `node <script>` | command: `node`, args: `<script> ...` | `node dist/server.js` |
| 已知包名（无 `npx`） | 自动补全 `npx -y` | `@anthropic/mcp-server-slack` → `npx -y @anthropic/mcp-server-slack` |

**环境变量自动检测**：
- 解析常见环境变量名模式：`*_TOKEN`、`*_API_KEY`、`*_SECRET`
- 查询 npm registry 获取包的 `package.json` → 读取 `mcp.environmentVariables` 字段（如有）
- 对已知模板匹配内置配置模板

**验收**：粘贴 `npx -y @anthropic/mcp-server-slack` → 自动解析并提示 SLACK_TOKEN → 填写后安装成功

---

#### B2. MCPB 文件安装

**问题**：MCPB（`.mcpb`）是新兴的 MCP 服务器打包标准，目前不支持。

**需求**：支持双击 `.mcpb` 文件或在应用中打开，自动解析和安装。

**MCPB 文件结构**（ZIP 包）：
```
server.mcpb
├── manifest.json          ← 元数据 + 配置
├── server/
│   ├── index.js           ← Node.js 入口
│   └── node_modules/      ← 打包的依赖
├── icon.png               ← 可选
└── assets/                ← 可选
```

**manifest.json 关键字段**：
```json
{
  "manifest_version": "0.4",
  "name": "github-mcp-server",
  "version": "1.0.0",
  "server": {
    "type": "node",
    "entry_point": "server/index.js",
    "mcp_config": {
      "command": "node",
      "args": ["${__dirname}/server/index.js"],
      "env": { "GITHUB_TOKEN": "${user_config.githubToken}" }
    }
  },
  "user_config": {
    "githubToken": {
      "type": "string",
      "sensitive": true,
      "label": "GitHub Personal Access Token",
      "description": "具有 repo 和 user 权限的 GitHub Token"
    }
  },
  "tools": ["create_issue", "search_repos"],
  "compatibility": { "minClientVersion": "1.0.0" }
}
```

**实现要点**：
- 注册 `.mcpb` 文件关联（Tauri `tauri-plugin-deep-link` 或手动关联）
- 安装步骤：
  1. 读取 ZIP 解析 `manifest.json`
  2. 提取 `server.type` → 确定运行时（node/python/binary）
  3. 检查运行时可执行性（node --version / python --version）
  4. 渲染 `user_config` 为配置表单
  5. 敏感字段存入 OS 密钥链
  6. 解压到 `~/.agent/mcp/bundles/<name>-<version>/`
  7. 使用 `${__dirname}` 替换为实际路径，生成 `McpServerConfig`
  8. 自动连接

**验收**：双击 `.mcpb` 文件 → 弹出配置对话框 → 填写配置 → 安装并自动连接服务器

---

#### B3. 外部配置导入

**问题**：用户可能已在 Claude Desktop、Cursor 或 VS Code 中配好了 MCP 服务器，不想重复配置。

**需求**：支持从其他客户端导入 MCP 配置。

```
┌──────────────────────────────────────────────┐
│  导入 MCP 配置                                │
│                                               │
│  ├─ 📋 从剪贴板导入                           │
│  │   复制 claude_desktop_config.json 片段     │
│  │   ┌──────────────────────────────────┐    │
│  │   │ { "mcpServers": { ... } }       │    │
│  │   └──────────────────────────────────┘    │
│  │   [解析并预览]                             │
│  │                                           │
│  ├─ 📁 从文件导入                             │
│  │   [选择文件] → claude_desktop_config.json │
│  │             → .cursor/mcp.json            │
│  │             → .vscode/mcp.json            │
│  │             → .continue/config.json       │
│  │                                           │
│  ├─ 🔗 从 Claude Code 导入                   │
│  │   检测项目目录中的 .mcp.json               │
│  │                                           │
│  └─ 导入预览:                                │
│     ☑ GitHub API      → npx ...              │
│     ☑ Filesystem      → npx ... (路径需调整)  │
│     ☑ PostgreSQL      → (缺少密码，需补充)     │
│     ───────────────────────────               │
│     [✓ 导入选中的 2 个服务器]                  │
└──────────────────────────────────────────────┘
```

**支持的配置格式**：
| 来源 | 配置文件 | 根键 | 特点 |
|------|---------|------|------|
| Claude Desktop | `claude_desktop_config.json` | `mcpServers` | 标准格式，直接解析 |
| Cursor | `.cursor/mcp.json` | `mcpServers` | 支持 `${env:VAR}` 插值 |
| VS Code | `.vscode/mcp.json` | `servers` | 有 `inputs` 字段需处理 |
| Windsurf | `mcp_config.json` | `mcpServers` | 有 `serverUrl` vs `url` 区别 |
| Continue.dev | `config.json` | `mcpServers` (数组) | YAML 格式需转换 |
| Claude Code | `.mcp.json` | `mcpServers` | 项目级配置 |

**实现要点**：
- 导入时字段映射（不同客户端命令字段名差异）
- 环境变量插值检测：`${env:VAR}` → 提示用户补充
- 敏感值检测：发现明文 Token → 提示加密存储
- 路径修正：跨平台路径调整（`/Users/name/` → `C:\Users\name\`）
- 导入后保持配置来源标记，方便重新导入时合并

**验收**：粘贴 Claude Desktop 配置 JSON → 解析出 3 个服务器 → 预览 → 全部导入 → 自动连接

---

#### B4. 分享链接安装

**问题**：想分享一个好用的 MCP 服务器配置给其他人。

**需求**：支持 URL Scheme 一键安装。

```
URL 格式:
  agent://mcp/install?name=GitHub&command=npx&args=-y,@anthropic/mcp-server-github&env=GITHUB_TOKEN

或简化格式（服务器 ID + 参数）:
  agent://mcp/install?ref=smithery/@anthropic/mcp-server-github

点击链接后的流程:
  1. 检测 Agent 是否已安装
  2. 解析参数
  3. 弹出确认对话框（显示要安装的内容）
  4. 用户补充必要配置（Token 等）
  5. 安装
```

**实现要点**：
- 使用 Tauri `tauri-plugin-deep-link` 注册 `agent://` URL Scheme
- URL 可含 `ref` 参数指向市场来源，或完整配置参数
- 安全性考虑：限制只从信任来源接受自动安装

**验收**：点击 `agent://mcp/install?...` 链接 → 弹出安装确认 → 确认后自动配置服务器

---

### ━━━ 模块 C：启动管理（Startup Management）━━━

#### C1. 智能启动策略

**问题**：所有 auto_connect 服务器在应用启动时同时启动，无优先级/延迟控制，可能导致启动峰值。

**需求**：支持分优先级、分批次、按需启动。

```
┌──────────────────────────────────────────────┐
│  MCP 服务器管理                               │
│                                               │
│  所有服务器 (5) │ 运行中 (3) │ 已停止 (2)      │
│──────────────────────────────────────────────│
│                                               │
│  ┌─ 🟢 Filesystem Server ──── 启动顺序: 1 ──┐ │
│  │  ✓ 启动时启动           ✓ 健康检查 30s     │ │
│  │  │  启动优先级: [1]  ← 先启动              │ │
│  │  │  启动延迟: [0ms]                        │ │
│  │  └──────────────────────────────────────  │ │
│  └──────────────────────────────────────────┘ │
│                                               │
│  ┌─ 🟢 GitHub API ────────── 启动顺序: 2 ──┐ │
│  │  ✓ 启动时启动           ✓ 健康检查 30s     │ │
│  │  │  启动优先级: [2]                       │ │
│  │  │  启动延迟: [500ms]                     │ │
│  │  └──────────────────────────────────────  │ │
│  └──────────────────────────────────────────┘ │
│                                               │
│  ┌─ ⚪ PostgreSQL ────────── 按需启动 ──────┐ │
│  │  ○ 启动时启动           ☑ 按需启动         │ │
│  │    需要时才启动，节省资源                    │ │
│  │    首次工具调用时自动唤醒                    │ │
│  └──────────────────────────────────────────┘ │
│                                               │
│  ┌─ ⚪ Docker Server ─────── 手动管理 ──────┐ │
│  │  ○ 启动时启动           ○ 按需启动         │ │
│  │    手动启动                               │ │
│  └──────────────────────────────────────────┘ │
└──────────────────────────────────────────────┘
```

**启动策略配置**：

```rust
pub struct StartupPolicy {
    /// 启动顺序（小数字优先，null 表示不自动启动）
    pub priority: Option<u32>,
    /// 在优先级的基础上额外延迟（毫秒）
    pub delay_ms: u64,
    /// 启动时自动启动
    pub launch_on_startup: bool,
    /// 按需启动（首次工具调用时启动）
    pub launch_on_demand: bool,
    /// 最大重试次数
    pub max_retries: u32,
    /// 健康检查间隔（毫秒）
    pub health_check_interval_ms: u64,
}
```

**启动时序**：

```
应用启动:
  T+0ms:    启动 priority=1 的服务器（并行）
  T+500ms:  启动 priority=2 的服务器（并行）
  T+2000ms: 启动 priority=3 的服务器（并行）
  ...
  按需:     launch_on_demand=true 的服务器暂不启动

按需启动流程:
  1. AgentLoop 准备执行工具
  2. 发现工具属于某个"未连接"的 MCP 服务器
  3. 自动触发该服务器的连接流程
  4. 等待握手完成 → 工具注册 → 继续执行（或超时报错）
  5. 后续调用直接使用，无需再次等待
```

**验收**：配置 3 个启动优先级不同的服务器 → 重启应用 → 观察启动顺序和延迟 → 按需服务器首次调用时自动唤醒

---

#### C2. 启动状态可视化

**问题**：现在只能看到"已连接/已断开"，缺乏中间状态。

**需求**：精细化的状态机和可视化指示。

**状态机**：

```
                    ┌──────────┐
                    │ disabled │ ← 用户禁用
                    └────┬─────┘
                         │ 用户启用
                    ┌────▼─────┐
            启动 ──▶│  waiting  │ ← 等待启动队列（如果有延迟）
                    └────┬─────┘
                         │ 到达启动时间
                    ┌────▼─────┐
                    │ starting  │ ← 子进程创建 + MCP 握手
                    └────┬─────┘
                    ┌────▼─────┐
            定时 ──▶│  ready    │ ← tools/list 完成，已注册
          健康检查   └────┬─────┘
                    ┌────▼─────┐
                    │ degraded  │ ← 健康检查连续失败 ≥ 3 次
                    └────┬─────┘
                    ┌────▼─────┐
                    │ stopping  │ ← 手动断开/重启/关闭
                    └────┬─────┘
                    ┌────▼─────┐
                    │ stopped   │ ← 进程已退出
                    └────┬─────┘
                         │ 自动重试（指数退避）
                         └──────────→ waiting
```

**状态指示器**：

| 状态 | 图标 | 颜色 | 说明 |
|------|------|------|------|
| disabled | ⚪ | 灰色 | 用户禁用 |
| waiting | ⏳ | 蓝色 | 等待启动 |
| starting | 🔄 | 蓝色（旋转） | 正在连接 |
| ready | 🟢 | 绿色 | 正常运行 |
| degraded | 🟡 | 黄色 | 健康异常但服务可用 |
| stopping | ⏹️ | 灰色 | 正在关闭 |
| stopped | ⬜ | 灰色（空心） | 已停止 |
| error | 🔴 | 红色 | 超出重试次数 |

**实现要点**：
- 状态通过 IPC 实时推送到前端（每秒轮询或事件驱动）
- 状态变更时关联声音/通知提示（可选）
- 错误状态显示最后一次错误信息
- 可点击查看日志（stderr 输出）

**验收**：断开一个服务器 → 状态变为 ⬜ stopped → 重新连接 → 经历 🔄 starting → 🟢 ready

---

#### C3. 健康监控面板

**问题**：当前健康监测在后台运行，用户看不到服务器实时状态。

**需求**：每个服务器卡片展示详细的运行指标。

```
┌─ Filesystem Server ──────────── 🟢 Ready ──────────┐
│                                                      │
│  运行时长: 2h 15m                                    │
│  PID: 12456                                          │
│  ─────────────────────────────────────────────────    │
│  总调用:    47 次     成功率: 100%                   │
│  平均延迟:  12ms     最大延迟: 156ms                  │
│  错误数:    0 次     最后错误: —                      │
│  ─────────────────────────────────────────────────    │
│  健康检查:  30s 间隔    最近一次: 5s 前 ✓            │
│  自动重连:  开启       重试次数: 0/3                  │
│  ─────────────────────────────────────────────────    │
│  stderr 日志（最近 10 行）:                          │
│  [INFO] Server started on PID 12456                  │
│  [INFO] Tool call: search_files (12ms)               │
│  [INFO] Tool call: read_file (8ms)                   │
│                                                      │
│  [刷新]  [查看完整日志]  [断开]  [重启]  [编辑]        │
└──────────────────────────────────────────────────────┘
```

**日志查看器**：
```
┌─ Filesystem Server — 日志 ────────────────────────┐
│                                                    │
│  🔍 搜索...                          [自动滚动]     │
│                                                    │
│  [12:00:01] [INFO]  ✓ Tool call: search_files      │
│  [12:00:05] [INFO]  ✗ Tool call: write_file (拒绝) │
│  [12:00:10] [INFO]  ✓ Health check: OK             │
│  [12:00:15] [WARN]  Connection degraded, retry 1/3 │
│  [12:00:17] [INFO]  ✓ Reconnect success            │
│  [12:00:20] [ERROR] Tool call: delete_file (超时)  │
│                                                    │
│  [清除日志]  [复制]  [导出]                         │
└────────────────────────────────────────────────────┘
```

**实现要点**：
- stderr 日志：`tokio::spawn` 持续读取子进程 stderr，写入环形缓冲区（内存 1000 行 + 可选文件持久化）
- 调用日志：每次 `tools/call` 记录耗时、成功/失败、参数摘要
- 健康检查日志：每次 ping 结果记录
- 前端日志查看器支持分级过滤（INFO/WARN/ERROR）
- 日志支持搜索和导出

**验收**：查看运行中服务器的面板 → 看到调用统计和 stderr 日志 → 手动触发错误 → 日志中出现错误记录

---

#### C4. 配置变更热重载

**问题**：修改服务器配置（如新增环境变量）需要断开重连，操作繁琐。

**需求**：配置变更后（除 command/args 外）自动热重载。

**热重载规则**：

| 变更类型 | 处理方式 |
|---------|---------|
| `name`、`tags`、`description` | 直接更新元数据，无需重启 |
| `env` 变更 | 自动重启（发送 shutdown → 用新 env 重新 spawn） |
| `tool_configs` 变更 | 实时更新注册表，无需重启 |
| `command`/`args` 变更 | 需手动确认重启（因这本质上是换了一个服务器） |
| `auto_connect` 变更 | 下次启动时生效，当前连接不变 |
| `startup_order` 变更 | 下次启动时生效 |

**实现要点**：
- 前端配置编辑后，调用 `update_mcp_server` IPC
- 后端判定变更类型，自动或询问后执行重连
- 重连期间缓冲对工具的调用（最多缓冲 5 秒，超时报错）
- 重连完成后自动恢复工具注册

**验收**：修改服务器的环境变量 → 自动重启 → 新环境变量生效 → 无需手动操作

---

### ━━━ 模块 D：集成链接（Integration Linking）━━━

#### D1. 对话级别 MCP 绑定

**问题**：所有 MCP 工具全局可用，用户无法控制"哪个对话用哪些工具"。

**需求**：每个对话可以绑定特定的 MCP 服务器。

```
对话设置面板：
┌──────────────────────────────────────────────┐
│  对话设置                                      │
│                                               │
│  绑定的 MCP 服务器:                            │
│  ☑ Filesystem Server   (4 个工具)              │
│  ☑ GitHub API          (6 个工具)              │
│  ☐ PostgreSQL          (3 个工具, 已停止)      │
│  ☐ Slack               (5 个工具, 已断开)      │
│  ─────────────────────────────────────         │
│  ☑ 启用所有可用服务器                           │
│                                               │
│  [保存]                                        │
└──────────────────────────────────────────────┘
```

**实现要点**：
- `Conversation` 模型新增 `linked_mcp_servers: Vec<String>` 字段
- 默认值为空（= 全部启用），显式指定则仅使用选中服务器
- 对话切换时，AgentLoop 的 ToolRegistry 动态切换工具集
- 对话创建时可从预设模板绑定
- 绑定不影响服务器本身的运行状态（只影响 Agent 能否看到它的工具）

**验收**：创建对话 A 绑定 Filesystem → 对话中 Agent 可使用文件工具 → 创建对话 B 未绑定 → 对话中无文件工具

---

#### D2. 工作流 MCP 集成

**问题**：工作流（Pipeline）当前无法声明依赖哪些 MCP 服务器。

**需求**：工作流 YAML 定义中声明所需的 MCP 服务器，执行时自动确保就绪。

```yaml
# workflow.yaml
name: "代码审查"
description: "审查 GitHub PR 代码质量"

mcp_servers:
  required:
    - github-server      # 必需：执行前自动确保连接
  optional:
    - filesystem-server  # 可选：有则用，没有也继续

steps:
  - id: fetch_pr
    tool: github-server__get_pr_content
    params:
      pr_number: "{{ trigger.pr_number }}"

  - id: analyze_code
    type: llm_call
    prompt: "审查以下代码变更: {{ steps.fetch_pr.result }}"
```

**实现要点**：
- 工作流执行前检查 `required` 服务器的状态
  - 全部就绪 → 直接执行
  - 部分未连接 → 自动连接（等待完成）
  - 连接失败 → 工作流报错终止
- `optional` 服务器连接失败则跳过，不影响工作流执行
- 工作流执行完成后可选择断开（通过配置 `disconnect_after` 标志）
- 工具命名使用 `server-id__tool-name` 格式，避免跨服务器工具名冲突

**验收**：创建工作流声明依赖 GitHub 服务器 → 执行时自动连接 → 步骤中使用 GitHub 工具 → 执行完成后保持连接

---

#### D3. 资源与提示词接入

**问题**：MCP 协议支持 Resources（资源）和 Prompts（提示词模板），但目前只接了 Tools。

**需求**：将 MCP 服务器的 Resources 和 Prompts 也接入到对话中。

**Resources 集成**：
```
MCP 服务器可暴露资源（文件内容、数据库记录等）
    │
    ├─→ 自动挂载为上下文（类似 system prompt 注入）
    │   适用于：配置文件、知识库文档、项目 README
    │
    └─→ 用户可手动引用
        对话中输入 @github-server:README.md → 自动获取资源内容
```

**Prompts 集成**：
```
MCP 服务器可暴露提示词模板
    │
    ├─→ 在对话输入框上方显示为 "可用模板"
    │   点击 → 自动填充提示词到输入框
    │
    └─→ 用户可手动触发
        /prompt github-server:pr-review → 插入 PR 审查模板
```

**实现要点**：
- 连接时执行 `resources/list` 和 `prompts/list`，缓存资源/提示词列表
- Resources 支持 `resources/subscribe` 监听变更（可选）
- 资源内容通过 `resources/read` 按需获取（避免一次性加载大量资源）
- 前端对话输入框上方显示可用 Prompt 模板
- 对话上下文注入：选择 "绑定资源" 后自动注入到 System Prompt

**验收**：连接暴露 Resources 的服务器 → 对话中可看到可用资源列表 → 点击自动获取内容

---

#### D4. MCP ↔ 技能互操作

**问题**：现有的 ScriptTool（技能）和 MCP Tool 是两个独立系统，技能无法调用 MCP 工具。

**需求**：技能执行时也可以调用已注册的 MCP 工具。

```
技能脚本 (Python/JS) 执行时:
    │
    ├─→ 通过标准输入获取已注册的 MCP 工具上下文
    │   (技能脚本的 JSON 输入中包含工具列表)
    │
    └─→ 技能脚本可以向 AgentLoop 提交工具调用请求
        (技能返回结构化结果，AgentLoop 解析后调用)
```

**实现要点**：
- 技能执行时在其输入 JSON 中包含当前可用的 MCP 工具列表
- 脚本可返回 `{ "type": "tool_call", "tool": "server__name", "args": {...} }`
- AgentLoop 接受技能返回的 tool_call 并执行
- 执行结果再传回技能脚本继续处理

**验收**：Python 技能读取 GitHub API MCP 工具 → 返回数据 → Agent 展示结果

---

### ━━━ 模块 E：安全与权限（Security）━━━

#### E1. 细粒度工具级权限

**问题**：当前只有启用/禁用 + 三种确认模式，粒度不够细。

**需求**：每个工具可配置独立的权限策略。

```rust
pub struct ToolPermission {
    /// 执行模式
    pub execution: ExecutionMode,
    /// 会话最大调用次数（null = 不限制）
    pub max_calls_per_session: Option<u32>,
    /// 单次执行超时（毫秒）
    pub timeout_ms: Option<u64>,
    /// 允许的参数值白名单（按参数名）
    pub allowed_params: Option<HashMap<String, Vec<serde_json::Value>>>,
    /// 敏感参数（调用日志中脱敏）
    pub sensitive_params: Vec<String>,
    /// 需要用户确认的参数条件
    pub confirm_conditions: Vec<ConfirmCondition>,
}

pub enum ExecutionMode {
    /// 自动允许
    AutoAllow,
    /// 每会话首次确认
    ConfirmOnce,
    /// 每次确认
    ConfirmAlways,
    /// 完全禁止
    Deny,
    /// 只允许特定参数
    AllowWithParams { allowed: Vec<String> },
}

pub struct ConfirmCondition {
    pub param: String,
    pub operator: ConfirmOperator,
    pub value: serde_json::Value,
}

pub enum ConfirmOperator {
    Equals,
    Contains,
    GreaterThan,
    Matches(String), // regex
}
```

**实例**：
- `write_file` → `AllowWithParams { allowed: ["path"] }` 只允许写文件路径参数
- `delete_file` → `ConfirmAlways` 每次调用都要确认
- `search_repos` → `AutoAllow` 安全读操作
- `execute_command` → `Deny` 高风险工具默认禁用

**验收**：将 write_file 设为每次确认 → Agent 调用时弹窗 → 点拒绝 → 工具调用失败 → Agent 继续执行其他工具

---

#### E2. 敏感配置管理

**问题**：API key、Token 等敏感信息明文存储在 config.json 中。

**需求**：敏感配置加密存储，不在 UI 中明文回传。

**加密方案**：

```
用户输入 API Key
    │
    ├─→ 前端：输入时 mask 显示（密码框）
    │   提交时作为 IPC 参数（内存中）
    │
    └─→ 后端：
         ├─→ 优先：OS 密钥链
         │   Windows: Credential Manager (winapi credential)
         │   macOS: Keychain (security framework)
         │   Linux: Secret Service (libsecret)
         │
         └─→ 回退：AES-256-GCM 加密后存入 config.json
             密钥从设备唯一标识派生
```

**实现要点**：
- `McpServerConfig.env` 中标记 `sensitive: true` 的字段
- 存储时加密，读取时解密（仅内存中明文）
- IPC 回传时始终脱敏（`sk-...xyz` 仅显示最后 4 位）
- 编辑时显示"已设置"标记，重新输入才覆盖
- 导出配置时可选"脱敏导出"（移除敏感值）

```
配置编辑器中的敏感字段：
┌──────────────────────────────────────┐
│  GITHUB_TOKEN:  [················]   │
│  已设置: sk-****bcde                  │
│  [取消设置]                           │
│                                       │
│  DATABASE_URL:  [················]   │
│  已设置: postgresql://****@localhost  │
│  [取消设置]                           │
└──────────────────────────────────────┘
```

**验收**：设置 GITHUB_TOKEN → 查看 config.json → Token 为密文 → 前端显示脱敏值 → 编辑页面显示"已设置"

---

#### E3. 审计日志

**问题**：MCP 工具被谁在何时调用了什么参数，无记录可查。

**需求**：所有工具调用记录持久化到审计日志，支持查询和导出。

**审计日志表**：
```sql
CREATE TABLE mcp_audit_log (
    id TEXT PRIMARY KEY,
    server_id TEXT NOT NULL,
    server_name TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    arguments_snapshot TEXT,     -- 参数快照（敏感参数脱敏）
    result_summary TEXT,         -- 结果摘要（截断）
    duration_ms INTEGER,
    success INTEGER NOT NULL,
    error_message TEXT,
    confirmation_mode TEXT,      -- 执行时的确认模式
    user_confirmed INTEGER,      -- 用户是否确认
    conversation_id TEXT,        -- 关联对话
    agent_iteration INTEGER,     -- Agent 执行轮次
    created_at TEXT NOT NULL
);

CREATE INDEX idx_audit_server ON mcp_audit_log(server_id, created_at);
CREATE INDEX idx_audit_tool ON mcp_audit_log(tool_name, created_at);
CREATE INDEX idx_audit_conv ON mcp_audit_log(conversation_id);
```

**审计查看器**：
```
┌──────────────────────────────────────────────────────┐
│  审计日志              [搜索工具名...]   [导出 CSV]    │
│──────────────────────────────────────────────────────│
│  时间         │ 服务器     │ 工具        │ 状态 │ 耗时 │
│──────────────────────────────────────────────────────│
│  12:00:01     │ Filesystem │ search_files │ ✓   │ 12ms │
│  12:00:05     │ GitHub     │ create_issue │ ✓   │ 1.2s │
│                │            │              │ 🔒   │      │
│                │            │ 参数: repo=x │ 用户确认   │
│  12:00:10     │ GitHub     │ merge_pr     │ ✗   │ 5s   │
│                │            │              │ 超时       │
│──────────────────────────────────────────────────────│
│  显示 1-3 / 共 47 条               [上一页] [下一页]  │
└──────────────────────────────────────────────────────┘
```

**实现要点**：
- 每次 `Tool::execute()` 调用即写审计日志（异步写入，不阻塞调用）
- 敏感参数在快照中脱敏（`password: "***"`, `token: "sk-***bcde"`）
- 审计日志保留策略：默认 90 天，可配置
- 审计日志支持按对话、服务器、时间范围过滤
- 支持导出为 CSV/JSON

**验收**：调用 3 次 MCP 工具 → 审计日志中看到 3 条记录 → 搜索过滤 → 导出 CSV

---

#### E4. 信任机制与首次确认

**问题**：添加新服务器时自动启动连接，没有任何确认。

**需求**：新服务器首次安装时显示信任确认，类似 VS Code 的"信任此扩展"。

```
┌──────────────────────────────────────────────┐
│  ⚠️ 首次连接确认                              │
│                                               │
│  服务器: GitHub API                           │
│  来源: Smithery 市场                          │
│  命令: npx -y @anthropic/mcp-server-github   │
│                                               │
│  此服务器将:                                  │
│  • 读取文件系统: ❌                            │
│  • 网络访问:    ✅ api.github.com              │
│  • 执行命令:    ❌                            │
│                                               │
│  暴露工具 (6 个):                             │
│  ✅ search_repos      (安全)                  │
│  ✅ get_file_content  (安全)                  │
│  ⚠️ create_issue      (写操作)                │
│  ⚠️ merge_pr          (写操作)                │
│  🔴 create_repo       (高风险, 默认禁用)      │
│                                               │
│  [拒绝]  [信任并连接—风险自担]                  │
└──────────────────────────────────────────────┘
```

**信任存储**：
```sql
CREATE TABLE mcp_trusted_servers (
    server_hash TEXT PRIMARY KEY,  -- command + args 的 SHA256
    first_trusted_at TEXT NOT NULL,
    last_connected_at TEXT NOT NULL,
    trust_level INTEGER DEFAULT 1,  -- 0=不可信, 1=可信, 2=完全信任
);

-- server_hash 计算示例
-- SHA256("npx -y @anthropic/mcp-server-github")
-- 用于识别相同服务器（即使位置不同）
```

**验收**：添加新服务器 → 弹信任确认 → 点信任 → 连接成功 → 再次添加同一服务器 → 不再弹窗

---

### ━━━ 模块 F：多传输协议支持（Transport）━━━

#### F1. Streamable HTTP 传输

**问题**：当前仅支持 stdio 传输，无法连接远程 HTTP MCP 服务器。

**需求**：支持 Streamable HTTP 传输（2025-03-26+ 协议标准）。

**配置示例**：
```json
{
  "id": "remote-llm",
  "name": "远程 LLM 服务",
  "transport": "streamable_http",
  "url": "https://api.example.com/mcp",
  "headers": {
    "Authorization": "Bearer sk-xxx"
  }
}
```

**实现要点**：
- 使用 `reqwest`（已有依赖）作为 HTTP 客户端
- 实现 `rmcp` 的 `Transport` trait 适配 HTTP
- 正确处理 `Mcp-Session-Id` header 实现会话管理
- 支持 JSON-RPC over HTTP POST + SSE 响应
- 初始化时通过 GET 请求建立 SSE 连接（服务器 → 客户端消息）

**验收**：添加 HTTP 传输类型的 MCP 服务器 → 输入 URL 和认证信息 → 连接成功 → 工具列表自动发现

---

#### F2. 混合传输支持

**需求**：同一系统同时支持 stdio 和 HTTP 服务器，对 AgentLoop 透明。

**架构**：
```
ToolRegistry
    │
    ├─ Stdio MCP Server  ──→ McpToolWrapper ──→ rmcp Client ──→ Child Process
    │
    └─ HTTP MCP Server  ──→ McpToolWrapper ──→ rmcp Client ──→ HTTP API
                                                      │
                                               Transport trait
                                                  │
                                           ┌──────┴──────┐
                                           │  StdioTransport │  HTTPTransport
                                           └──────────────┘
```

**实现要点**：
- `McpToolWrapper` 不关心传输层，只通过 `rmcp::client::Client` 调用工具
- 传输层由 `McpServerManager` 在连接时选择
- `TransportType` 枚举扩展：`Stdio | StreamableHttp`

**验收**：同时连接一个 stdio 服务器和一个 HTTP 服务器 → Agent 可调用两个服务器的工具 → 传输层差异对用户透明

---

## 三、数据模型变更

### Rust 数据模型（config.rs）

```rust
/// MCP 服务器配置（持久化）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub source: Option<String>,             // "market" | "template" | "import" | "manual"
    pub transport: TransportType,
    // stdio 传输
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    // HTTP 传输
    pub url: Option<String>,
    pub headers: Option<HashMap<String, String>>,
    // 环境变量（敏感值加密存储）
    pub env: HashMap<String, EnvValue>,
    // 启动策略
    pub startup: StartupPolicy,
    // 连接管理
    pub auto_connect: bool,
    pub tool_configs: HashMap<String, ToolConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransportType {
    Stdio,
    #[serde(rename = "streamable_http")]
    StreamableHttp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvValue {
    pub value: Option<String>,      // null = 未设置
    pub sensitive: bool,
    pub key: String,                // 环境变量名，用于显示
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartupPolicy {
    pub priority: Option<u32>,
    pub delay_ms: u64,
    pub launch_on_startup: bool,
    pub launch_on_demand: bool,
    pub max_retries: u32,
    pub retry_delay_ms: u64,
    pub health_check_interval_ms: u64,
}

impl Default for StartupPolicy {
    fn default() -> Self {
        Self {
            priority: None,
            delay_ms: 0,
            launch_on_startup: true,
            launch_on_demand: false,
            max_retries: 3,
            retry_delay_ms: 2000,
            health_check_interval_ms: 30000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolConfig {
    pub enabled: bool,
    pub permission: ToolPermission,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPermission {
    pub execution: ExecutionMode,
    pub max_calls_per_session: Option<u32>,
    pub timeout_ms: Option<u64>,
    pub allowed_params: Option<HashMap<String, Vec<serde_json::Value>>>,
    pub sensitive_params: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutionMode {
    AutoAllow,
    ConfirmOnce,
    ConfirmAlways,
    Deny,
    AllowWithParams { allowed: Vec<String> },
}
```

### 运行时状态（manager.rs）

```rust
pub struct McpConnection {
    pub id: String,
    pub config: McpServerConfig,
    pub status: ConnectionStatus,
    pub status_message: Option<String>,
    pub started_at: Option<Instant>,
    pub pid: Option<u32>,
    pub stats: ConnectionStats,
    // 运行时
    pub child: Option<Child>,
    pub client: Option<RunningService<...>>,
    pub tools: Vec<McpToolInfo>,
    pub retry_count: u32,
    pub consecutive_failures: u32,
    pub health_check_handle: Option<JoinHandle<()>>,
    pub stderr_buffer: Arc<Mutex<VecDeque<String>>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionStatus {
    Disabled,
    Waiting,
    Starting,
    Ready,
    Degraded,
    Stopping,
    Stopped,
    Error(String),
}

pub struct ConnectionStats {
    pub total_calls: u64,
    pub successful_calls: u64,
    pub failed_calls: u64,
    pub total_duration_ms: u128,
    pub last_call_at: Option<Instant>,
    pub last_error: Option<String>,
    pub last_health_check: Option<HealthCheckResult>,
}
```

### 数据库表（新增）

```sql
-- MCP 服务器配置迁移到 DB
CREATE TABLE mcp_servers (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    config_json TEXT NOT NULL,       -- McpServerConfig 完整序列化
    status TEXT NOT NULL DEFAULT 'disabled',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- 审计日志
CREATE TABLE mcp_audit_log (
    id TEXT PRIMARY KEY,
    server_id TEXT NOT NULL,
    server_name TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    arguments_snapshot TEXT,
    result_summary TEXT,
    duration_ms INTEGER,
    success INTEGER NOT NULL,
    error_message TEXT,
    confirmation_mode TEXT,
    user_confirmed INTEGER,
    conversation_id TEXT,
    agent_iteration INTEGER,
    created_at TEXT NOT NULL
);

-- 信任记录
CREATE TABLE mcp_trusted_servers (
    server_hash TEXT PRIMARY KEY,
    first_trusted_at TEXT NOT NULL,
    last_connected_at TEXT NOT NULL,
    trust_level INTEGER DEFAULT 1
);

-- 市场缓存
CREATE TABLE mcp_market_cache (
    source TEXT NOT NULL,            -- "smithery" | "npm" | "community"
    server_id TEXT NOT NULL,
    cache_json TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    PRIMARY KEY (source, server_id)
);

-- 对话 MCP 绑定
ALTER TABLE conversations ADD COLUMN linked_mcp_servers TEXT;
-- linked_mcp_servers: JSON array of server IDs, null = all enabled
```

---

## 四、新 IPC 命令

| 命令 | 方向 | 类别 | 用途 |
|------|------|------|------|
| `search_mcp_market` | C→S | 市场 | 搜索 MCP 市场 |
| `get_mcp_market_detail` | C→S | 市场 | 获取服务器详情 |
| `install_mcp_from_market` | C→S | 安装 | 从市场安装 + 配置 |
| `install_mcp_from_command` | C→S | 安装 | 从命令粘贴安装 |
| `parse_mcp_command` | C→S | 安装 | 预解析命令但不安装 |
| `import_mcp_config` | C→S | 安装 | 导入外部配置文件 |
| `parse_mcpb_file` | C→S | 安装 | 解析 MCPB 文件 |
| `install_mcp_from_mcpb` | C→S | 安装 | 从 MCPB 文件安装 |
| `export_mcp_servers` | C→S | 管理 | 导出配置为 JSON |
| `restart_mcp_server` | C→S | 管理 | 重启服务器 |
| `get_mcp_server_logs` | C→S | 管理 | 获取 stderr 日志 |
| `test_mcp_connection` | C→S | 管理 | 测试配置是否正确 |
| `get_mcp_templates` | C→S | 模板 | 获取内置模板列表 |
| `batch_set_mcp_tools` | C→S | 权限 | 批量配置工具权限 |
| `get_mcp_audit_logs` | C→S | 审计 | 获取审计日志 |
| `export_mcp_audit_logs` | C→S | 审计 | 导出审计日志 |
| `clear_mcp_audit_logs` | C→S | 审计 | 清理审计日志 |
| `update_mcp_env` | C→S | 配置 | 更新环境变量（仅改值） |
| `update_mcp_startup` | C→S | 配置 | 更新启动策略 |
| `update_conversation_mcp` | C→S | 集成 | 更新对话绑定的 MCP 服务器 |

---

## 五、前端组件规划

| 组件 | 用途 | 状态 |
|------|------|------|
| `McpManagerPage.tsx` | 增强版管理主页（状态列表 + 统计 + 日志） | 增强 |
| `McpServerCard.tsx` | 服务器卡片（状态指示 + 工具列表 + 指标） | 新增 |
| `McpMarketPage.tsx` | 市场浏览页（多 Tab 来源） | 新增 |
| `McpMarketDetail.tsx` | 市场服务器详情页 | 新增 |
| `McpInstallDialog.tsx` | 安装对话框（多 Tab：市场/命令/模板/导入/手动） | 新增 |
| `McpCommandParser.tsx` | 命令粘贴解析器 | 新增 |
| `McpImportWizard.tsx` | 导入配置向导 | 新增 |
| `McpConfirmDialog.tsx` | 工具调用确认弹窗 | 新增 |
| `McpTrustDialog.tsx` | 首次信任确认弹窗 | 新增 |
| `McpLogViewer.tsx` | 日志查看器 | 新增 |
| `McpAuditLogPage.tsx` | 审计日志页 | 新增 |
| `McpEnvEditor.tsx` | 环境变量编辑器（敏感值脱敏） | 新增 |
| `McpBindingPanel.tsx` | 对话绑定面板 | 新增 |
| `ConversationMcpSettings.tsx` | 对话设置中的 MCP 标签 | 新增 |

---

## 六、研发计划

### Phase 1：启动管理与健康监控（1-2 周）

| 任务 | 说明 | 预估 |
|------|------|------|
| **C1. 启动策略** | StartupPolicy 模型、优先级队列、延迟启动、按需启动 | 3-4h |
| **C2. 状态可视化** | 精细化状态机、状态指示 UI、状态实时推送 | 2-3h |
| **C3. 健康监控面板** | 调用统计、运行指标、stderr 日志记录与查看 | 3-4h |
| **C4. 热重载** | 配置 diff 判定、自动重启、调用缓冲 | 2-3h |

**Phase 1 总预估**：10-14h（1-2 天）

### Phase 2：市场与快捷安装（2-3 周）

| 任务 | 说明 | 预估 |
|------|------|------|
| **A1. 市场浏览** | 多来源可插拔架构、Smithery API 集成、搜索/分类 | 4-6h |
| **A2. 详情与配置** | 详情页 UI、工具预览、配置表单自动生成 | 3-4h |
| **A3. 一键安装** | 安装流程编排、配置自动生成、连接后验证 | 2-3h |
| **B1. 命令粘贴** | 命令解析器、npm registry 包名查询、环境变量检测 | 3-4h |
| **B2. MCPB 支持** | ZIP 解析、manifest 校验、user_config 渲染 | 4-5h |
| **B3. 外部导入** | 多格式解析器、字段映射、路径修正 | 3-4h |
| **B4. 分享链接** | URL Scheme 注册、参数解析、安全校验 | 2-3h |

**Phase 2 总预估**：21-29h（3-4 天）

### Phase 3：集成链接（1-2 周）

| 任务 | 说明 | 预估 |
|------|------|------|
| **D1. 对话绑定** | Conversation 模型扩展、AgentLoop 动态工具切换 | 2-3h |
| **D2. 工作流集成** | PipelineEngine MCP 依赖声明、自动连接/断开 | 3-4h |
| **D3. 资源与提示词** | Resources/Prompts 发现与缓存、对话注入 | 3-4h |
| **D4. 技能互操作** | 技能输入扩展 tool_call 返回格式 | 2-3h |

**Phase 3 总预估**：10-14h（1-2 天）

### Phase 4：安全与审计（1-2 周）

| 任务 | 说明 | 预估 |
|------|------|------|
| **E1. 工具级权限** | 细粒度权限模型、参数白名单、执行条件 | 3-4h |
| **E2. 敏感配置管理** | 加密存储/OS 密钥链、前端脱敏显示 | 3-4h |
| **E3. 审计日志** | 异步日志写入、查看器、搜索/过滤/导出 | 3-4h |
| **E4. 信任机制** | 首次确认弹窗、信任哈希存储 | 2-3h |

**Phase 4 总预估**：11-15h（1-2 天）

### Phase 5：多传输协议（1 周）

| 任务 | 说明 | 预估 |
|------|------|------|
| **F1. Streamable HTTP** | Transport trait 适配、reqwest 集成、会话管理 | 4-6h |
| **F2. 混合传输** | 传输类型统一、管理 UI 扩展 | 2-3h |

**Phase 5 总预估**：6-9h（1 天）

---

### 里程碑

```
第 1 周  ████████████████████ Phase 1: 启动管理 + 健康监控
第 2-3 周 ████████████████████████████████ Phase 2: 市场 + 快捷安装
第 4 周  ████████████████████ Phase 3: 集成链接
第 5 周  ████████████████████ Phase 4: 安全 + 审计
第 6 周  ████████████         Phase 5: 多传输协议
```

---

## 七、验收清单

### Phase 1 完成标志
- [ ] 3 个不同优先级的服务器按顺序延迟启动
- [ ] 按需启动的服务器首次工具调用时自动唤醒
- [ ] 服务器卡片显示精细化运行状态（🟢🟡🔴⚪）
- [ ] 健康监控面板显示调用统计、延迟、错误率
- [ ] 可查看子进程 stderr 日志
- [ ] 修改环境变量后服务器自动热重启

### Phase 2 完成标志
- [ ] 市场页显示多个来源的服务器列表
- [ ] 点击服务器进入详情页，展示工具和配置表单
- [ ] 从市场一键安装并自动连接
- [ ] 粘贴 `npx -y @xxx/yyy` 命令自动解析为配置
- [ ] 双击 `.mcpb` 文件弹出安装向导
- [ ] 从 Claude Desktop 配置 JSON 导入 3 个服务器
- [ ] 点击 `agent://mcp/install?...` 链接弹出安装确认

### Phase 3 完成标志
- [ ] 对话 A 绑定 Filesystem → 对话 A 可用，对话 B 不可用
- [ ] 工作流 YAML 声明 required 服务器 → 执行时自动连接
- [ ] 连接 Resources 服务器 → 对话中可引用资源
- [ ] 技能执行时可调用 MCP 工具

### Phase 4 完成标志
- [ ] write_file 设为 ConfirmAlways → 每次调用弹窗确认
- [ ] 敏感配置加密存储 → config.json 中为密文
- [ ] 审计日志记录每次工具调用 → 支持搜索和导出
- [ ] 首次安装新服务器弹出信任确认

### Phase 5 完成标志
- [ ] 添加 HTTP 传输类型的 MCP 服务器 → 连接成功
- [ ] 同时连接 stdio + HTTP 服务器 → Agent 可同时使用

---

## 八、技术架构总图

```
┌────────────────────────────────────────────────────────────────────┐
│                        前端 (React + Zustand)                       │
│                                                                     │
│  ┌─────────────────┐  ┌─────────────────┐  ┌────────────────────┐  │
│  │ McpMarketPage    │  │ McpManagerPage  │  │ McpAuditLogPage    │  │
│  │ - 浏览/搜索       │  │ - 状态列表       │  │ - 审计日志          │  │
│  │ - 详情/配置       │  │ - 统计/日志      │  │ - 搜索/导出         │  │
│  │ - 一键安装        │  │ - 启动策略       │  └────────────────────┘  │
│  └────────┬────────┘  │ - 环境变量编辑    │                          │
│           │           └────────┬────────┘  ┌────────────────────┐  │
│  ┌────────▼────────┐           │           │ ConversationMcp    │  │
│  │ McpInstallDialog │           │           │ Settings           │  │
│  │ - 命令粘贴解析    │           │           │ - 对话绑定          │  │
│  │ - MCPB 解析      │  IPC invoke() / listen()                    │  │
│  │ - 外部导入        │           │                                │  │
│  └─────────────────┘           │                                │  │
├────────────────────────────────┼─────────────────────────────────┤  │
│                        后端 (Rust + Tauri)                        │  │
│                                │                                │  │
│  ┌────────────────────────────┼─────────────────────────────┐   │  │
│  │                    commands/mcp.rs (20+ IPC)              │   │  │
│  └────────────────────────────┼─────────────────────────────┘   │  │
│                                │                                │  │
│  ┌────────────────────────────▼─────────────────────────────┐   │  │
│  │                    mcp/ 模块                              │   │  │
│  │                                                          │   │  │
│  │  ┌──────────────┐  ┌──────────────┐  ┌────────────────┐  │   │  │
│  │  │ manager.rs   │  │ market.rs    │  │ security.rs    │  │   │  │
│  │  │ - 生命周期    │  │ - Smithery   │  │ - 权限模型      │  │   │  │
│  │  │ - 健康监控    │  │ - 模板库      │  │ - 加密存储      │  │   │  │
│  │  │ - 热重载      │  │ - 缓存       │  │ - 信任管理      │  │   │  │
│  │  └──────┬───────┘  └──────────────┘  └────────────────┘  │   │  │
│  │         │                                                │   │  │
│  │  ┌──────▼───────┐  ┌──────────────┐  ┌────────────────┐  │   │  │
│  │  │ install.rs   │  │ config.rs    │  │ storage.rs     │  │   │  │
│  │  │ - 命令解析    │  │ - 配置模型    │  │ - DB 迁移       │  │   │  │
│  │  │ - MCPB 解析   │  │ - 模板常量    │  │ - 配置 CRUD     │  │   │  │
│  │  │ - 外部导入    │  │ - 序列化      │  │ - 缓存管理      │  │   │  │
│  │  └──────────────┘  └──────────────┘  └────────────────┘  │   │  │
│  │                                                          │   │  │
│  │  ┌──────────────────────────────────────────────────┐    │   │  │
│  │  │ transport/                                       │    │   │  │
│  │  │  ├─ mod.rs   (Transport trait)                    │    │   │  │
│  │  │  ├─ stdio.rs (通过 rmcp TokioChildProcess)        │    │   │  │
│  │  │  └─ http.rs  (reqwest + rmcp Transport impl)      │    │   │  │
│  │  └──────────────────────────────────────────────────┘    │   │  │
│  └────────────────────────┬──────────────────────────────┘   │  │
│                           │                                  │  │
│                           ▼                                  │  │
│  ┌──────────────────────────────────────────────────────┐    │  │
│  │               ToolRegistry (统一入口)                 │    │  │
│  │  内置工具 │ MCP工具(McpToolWrapper) │ 技能(ScriptTool) │    │  │
│  └──────────────────────┬───────────────────────────────┘    │  │
│                          │                                    │  │
│  ┌──────────────────────▼───────────────────────────────┐    │  │
│  │   AgentLoop (agent/loop.rs) | PipelineEngine          │    │  │
│  │   - get_enabled() → 含 MCP 工具                       │    │  │
│  │   - execute_tool() → 检查权限 → 确认 → 执行 → 审计    │    │  │
│  └──────────────────────────────────────────────────────┘    │  │
└─────────────────────────────────────────────────────────────────┘
```

---

## 九、依赖与风险

### 新增依赖

| 依赖 | 版本 | 用途 | 替代方案 |
|------|------|------|---------|
| `reqwest` | 已有 | HTTP 传输 (Streamable HTTP) | — |
| `aes-gcm` | 新增 | 环境变量加密存储 | OS 密钥链原生 API |
| `zip` | 新增 | MCPB ZIP 解析 | — |
| `sha2` | 新增 | 信任服务器哈希计算 | — |
| `tauri-plugin-deep-link` | 新增 | URL Scheme 注册 | — |

### 风险与缓解

| 风险 | 可能性 | 影响 | 缓解措施 |
|------|--------|------|---------|
| Smithery API 变更 | 中 | 市场功能不可用 | 多来源回退 + GraphQL 终端缓存 |
| MCPB 格式持续演进 | 中 | 兼容性问题 | manifest_version 版本检测 + 适配 |
| OS 密钥链 API 差异 | 低 | 加密不一致 | 统一抽象层 + AES 回退 |
| 子进程泄漏 | 低 | 系统资源耗尽 | 进程组 kill + 应用关闭时强制清理 |
| HTTP 传输安全性 | 中 | 中间人攻击 | TLS 强制 + OAuth 2.1 推荐 |
| 审计日志膨胀 | 低 | 磁盘占用 | 默认 90 天保留策略 + 自动清理 |

---

## 十、与 V2 PRD 的关系

V3 是 V2 的**横向扩展**——V2 将 MCP 从"基础连接"升级到"安全可控"，V3 则将 MCP 从"可控工具源"进一步升级到"桌面端 MCP 管理平台"。

| 维度 | V2 覆盖 | V3 新增 |
|------|---------|---------|
| 工具控制 | 逐工具开关 + 确认模式 | 细粒度权限 + 参数白名单 |
| 健康监控 | 基础 ping + 自动重连 | 完整监控面板 + 调用统计 + 日志 |
| 模板 | 8 个内置 | 市场浏览 + Smithery + 社区 |
| 安装 | 手动添加 | 一键安装 + 命令粘贴 + MCPB + 导入 |
| 集成 | — | 对话绑定 + 工作流集成 + 资源/提示词 |
| 安全 | 基础确认 | 加密存储 + 审计日志 + 信任机制 |
| 传输 | stdio 仅 | stdio + Streamable HTTP |

V3 完成后，V2 的 A1-A4 模块仍然有效，作为 V3 的基础能力保留。

---

> 📁 本文档路径：`.omo/plans/prd-mcp-management-v3.md`
> 🔗 前置依赖：V1 路线图 (`.omo/plans/mcp-and-pipeline-roadmap.md`)、V2 PRD (`.omo/plans/prd-mcp-workflow-v2.md`)
