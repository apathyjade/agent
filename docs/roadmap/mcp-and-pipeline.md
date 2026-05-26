# Agent 产品升级路线图：MCP 运行时 + 工作流自动化

> 状态：设计阶段 | 创建：2026-05-21

---

## 一、总体愿景

将 Agent 从"单轮对话 AI 客户端"升级为 **AI Agent 工作台** ——

1. **MCP 运行时**：连接任意 MCP Server 作为工具源，成为桌面端的 MCP 管理平台
2. **工作流自动化**：多步骤 LLM 驱动的自动化流水线，支持定时触发器

两者形成飞轮：MCP 提供丰富工具 → 工作流编排工具 → 更多用户 → 更多 MCP Server 开发者。

---

## 二、方向一：MCP 运行时平台

### 2.1 产品设计

#### 用户故事

- **开发者**：配置一个 MCP Server（如 `@anthropic/mcp-server-filesystem`），Agent 自动识别 Server 暴露的工具并在对话中使用
- **企业用户**：管理员统一配置 MCP Server（数据库查询、内部 API），推送到团队所有成员的 Agent 客户端
- **普通用户**：从 MCP 市场一键安装经过审核的 Server，无需理解命令行配置

#### 界面设计

```
设置 → MCP 连接
┌─────────────────────────────────────────┐
│ 🔌 MCP Server 连接                       │
│                                         │
│ ┌─ 已连接的 Servers ──────────────────┐ │
│ │ 📁 Filesystem       ● 运行中  2 tools │ │
│ │ 🗄️ PostgreSQL       ● 运行中  1 tool  │ │
│ │ 🌐 Brave Search     ○ 已断开          │ │
│ └─────────────────────────────────────┘ │
│                                         │
│ [+ 添加 MCP Server]  [🌐 浏览市场]       │
└─────────────────────────────────────────┘

添加 MCP Server 弹窗：
┌─────────────────────────────────────────┐
│ 名称:  Filesystem Bridge                │
│ 命令:  npx                              │
│ 参数:  -y @anthropic/mcp-server-fs     │
│        /path/to/allowed/dir            │
│ 环境:  HOME=/home/user                 │
│                                         │
│ [测试连接]  [保存]                       │
└─────────────────────────────────────────┘
```

#### 市场页面

```
MCP 市场
┌─────────────────────────────────────────┐
│ 🔍 搜索 MCP Server...                    │
│                                         │
│ 热门 Servers:                            │
│ ┌─ @anthropic/filesystem      42K ⭐ ─┐ │
│ │ 安全的本地文件系统访问               │ │
│ └────────────────────────────────────┘ │
│ ┌─ @modelcontextprotocol/server-     ┌─┐
│ │   brave-search              28K ⭐   │ │
│ │ Web 和本地搜索                      │ │
│ └────────────────────────────────────┘ │
└─────────────────────────────────────────┘
```

### 2.2 技术架构

#### MCP 协议要点

MCP 使用 JSON-RPC 2.0 over stdio（也支持 SSE over HTTP，但 stdio 是桌面端的首选）：

```
Client (Agent)                    Server (子进程)
     │                                  │
     │─── initialize ──────────────────>│  握手
     │<── capabilities ────────────────│
     │                                  │
     │─── tools/list ──────────────────>│  获取工具列表
     │<── [ToolDef, ...] ──────────────│
     │                                  │
     │─── tools/call {name, args} ─────>│  调用工具
     │<── {result/error} ──────────────│
```

关键 JSON-RPC 消息：

| 方法 | 方向 | 用途 |
|---|---|---|
| `initialize` | C→S | 握手，协商协议版本和能力 |
| `notifications/initialized` | C→S | 确认初始化完成 |
| `tools/list` | C→S | 获取 Server 提供的工具列表 |
| `tools/call` | C→S | 调用指定工具 |
| `resources/list` | C→S | 获取资源列表（可选） |
| `prompts/list` | C→S | 获取提示词列表（可选） |

#### 架构设计

```
┌─────────────── Frontend ───────────────┐
│  SettingsModal   McpMarketPage         │
│  ┌─────────────┐ ┌──────────────────┐ │
│  │McpServerCard│ │MarketSkillCard   │ │
│  └─────────────┘ └──────────────────┘ │
└──────────────────┬────────────────────┘
                   │ IPC (invoke)
┌─────────────── Backend ────────────────┐
│                                         │
│  ┌──────────────────────────────────┐  │
│  │         McpManager               │  │
│  │  - connections: HashMap<id, Conn>│  │
│  │  - connect(id)                   │  │
│  │  - disconnect(id)                │  │
│  │  - list_tools(id) -> [ToolInfo]  │  │
│  │  - call_tool(id, name, args)     │  │
│  └──────────┬───────────────────────┘  │
│             │                           │
│  ┌──────────▼───────────────────────┐  │
│  │       McpClient (per conn)       │  │
│  │  - child: tokio::process::Child  │  │
│  │  - stdin: write JSON-RPC         │  │
│  │  - stdout: read JSON-RPC         │  │
│  │  - tools: Vec<McpTool>           │  │
│  └──────────┬───────────────────────┘  │
│             │                           │
│  ┌──────────▼───────────────────────┐  │
│  │       ToolRegistry               │  │
│  │  - register_dynamic(name, tool)   │  │
│  │  - unregister(name)              │  │
│  └──────────────────────────────────┘  │
│                                         │
│  ┌──────────────────────────────────┐  │
│  │         AgentLoop                │  │
│  │  - get_enabled() → 含 MCP tools  │  │
│  └──────────────────────────────────┘  │
└─────────────────────────────────────────┘
```

#### 数据模型

```rust
// 数据库表: mcp_connections
struct McpConnectionRecord {
    id: String,             // UUID
    name: String,           // 用户可见名称
    command: String,        // 命令 (npx, python, node)
    args: Vec<String>,      // 参数
    env: Option<HashMap<String, String>>,  // 环境变量
    enabled: bool,
    auto_connect: bool,     // 启动时自动连接
    installed_at: String,
    updated_at: String,
}
```

```rust
// MCP 工具包装器
struct McpTool {
    connection_id: String,
    tool_name: String,      // MCP Server 暴露的工具名
    description: String,
    input_schema: Value,    // JSON Schema
}

impl Tool for McpTool {
    fn name(&self) -> &str { &self.tool_name }
    fn description(&self) -> &str { &self.description }
    fn parameters(&self) -> Value { self.input_schema.clone() }
    async fn execute(&self, input: Value) -> Result<Value> {
        // 序列化 JSON-RPC tools/call 请求
        // 通过 McpClient 发送
        // 解析响应
    }
}
```

#### 与现有 SkillManager 的关系

MCP Server 是一种特殊的 Skill 来源：

```
SkillManager 现有能力:
├── ScriptTool (type: script → skill.yaml)
├── 未来: McpServer (type: mcp → mcp_config.json)
└── 未来: Wasm (type: wasm → .wasm module)

统一入口: ToolRegistry.register_dynamic()
```

两种设计选择：
1. **MCP 作为 SkillEntry 的新变体**：`SkillEntry::Mcp { command, args }`
2. **独立的 McpManager**：更干净，但需要另一套管理界面

推荐方案：**独立 McpManager + 统一注册到 ToolRegistry**。Skill 和 MCP 的生命周期不同（Skill 是静态安装的，MCP 需要连接状态管理）。

### 2.3 安全设计

MCP Server 以子进程运行，安全是桌面端的关键卖点：

```
权限模型:
┌─────────────────────────────────────────┐
│ 🔒 权限控制                              │
│                                         │
│ 📁 Filesystem:  □ 允许读取  □ 允许写入   │
│                 路径白名单: /home/user/  │
│ 🌐 Network:     □ 允许外部请求           │
│ 🗄️ Database:    □ 允许查询  □ 允许写入   │
│ ⚙️ Shell:       □ 允许执行 (危险!)       │
│                                         │
│ 每次执行前:  □ 需要用户确认              │
│              □ 静默执行 (仅白名单工具)    │
└─────────────────────────────────────────┘
```

每个 MCP 工具注册时可配置：
- 是否需要用户确认
- 最大执行超时
- 允许的参数白名单

### 2.4 研发阶段

| 阶段 | 内容 | 工期 |
|---|---|---|
| **P0: MCP 客户端核心** | JSON-RPC stdio 通道、initialize/tools/list/tools/call、进程生命周期管理 | 3-4 天 |
| **P1: ToolRegistry 集成** | McpTool 实现 Tool trait、连接时批量注册、断开时注销 | 1-2 天 |
| **P2: 管理界面** | MCP 连接配置卡片、连接状态指示器、工具预览 | 2-3 天 |
| **P3: MCP 市场** | 搜索界面、一键安装、版本管理 | 2-3 天 |
| **P4: 安全与权限** | 权限模型、工具审批、沙箱（可选） | 2-3 天 |

**MVP 范围（P0 + P1）**：用 JSON 配置一个 MCP Server（如 `@anthropic/mcp-server-filesystem`），对话中自动识别并使用 MCP 工具。P0 用 `rmcp` 官方 SDK，不重复造轮子。

---

## 三、方向二：工作流自动化流水线

### 3.1 产品设计

#### 用户故事

- **信息工作者**："每天早上 9 点，扫描收件箱文件夹里的新 PDF，提取摘要并翻译成英文，发送到指定邮箱"
- **开发者**："收到 GitHub PR 后，自动运行代码审查 Agent，在 PR 下评论审查结果"
- **运营**："每小时检查竞品网站是否有更新，有变化时生成对比报告发 Slack"

#### 界面设计

```
工作流管理页面
┌─────────────────────────────────────────┐
│ ⚡ 工作流                                │
│                                         │
│ ┌─ 活跃的工作流 ──────────────────────┐ │
│ │ 📧 Daily Summary       ⏰ 09:00     │ │
│ │    上次: 今天 09:00 ✅ 成功          │ │
│ │                                     │ │
│ │ 🔍 PR Code Review      📨 Webhook   │ │
│ │    上次: 10 分钟前 ✅ 成功           │ │
│ │                                     │ │
│ │ 📊 Competitor Monitor  ⏰ 每小时     │ │
│ │    上次: 45 分钟前 ❌ 超时           │ │
│ └─────────────────────────────────────┘ │
│                                         │
│ [+ 创建工作流]  [📋 模板市场]            │
└─────────────────────────────────────────┘

工作流编辑器
┌─────────────────────────────────────────┐
│ 工作流名称: Daily PDF Summary            │
│                                         │
│ 触发器: ⏰ Cron  0 9 * * *              │
│                                         │
│ ┌─ Step 1: 扫描文件 ──────────────────┐ │
│ │ 工具: 📁 file_system.list           │ │
│ │ 参数:                               │ │
│ │   path: "~/documents/inbox"        │ │
│ │   pattern: "*.pdf"                 │ │
│ └────────────────────────────────────┘ │
│              ↓                          │
│ ┌─ Step 2: AI 判断 ──────────────────┐ │
│ │ 如果: 结果为空 → 结束               │ │
│ │ 否则: 继续                          │ │
│ └────────────────────────────────────┘ │
│              ↓                          │
│ ┌─ Step 3: 提取文本 ────────────────┐ │
│ │ 工具: 🧠 LLM                       │ │
│ │ 提示词: "提取以下 PDF 的文本..."    │ │
│ │ 输入: {{ step_1.files }}           │ │
│ └────────────────────────────────────┘ │
│              ↓                          │
│ ┌─ Step 4: 发送邮件 ────────────────┐ │
│ │ 工具: ✉️ email.send                │ │
│ │ 参数: to, subject, body            │ │
│ └────────────────────────────────────┘ │
│                                         │
│ [▶ 测试运行]  [💾 保存]                  │
└─────────────────────────────────────────┘
```

### 3.2 技术架构

#### 工作流 DSL

```yaml
# workflow.yaml
name: "Daily PDF Summary"
description: "每天早上扫描新 PDF 并发送摘要邮件"

trigger:
  type: cron
  schedule: "0 9 * * *"

steps:
  - id: scan_files
    tool: file_system
    params:
      action: list
      path: "~/documents/inbox"
      pattern: "*.pdf"

  - id: check_empty
    type: condition
    condition: "{{steps.scan_files.result.files.length}} > 0"
    on_false: end

  - id: extract_text
    tool: llm_call
    params:
      prompt: |
        提取以下文件的内容摘要：
        {{ steps.scan_files.result | json }}

  - id: send_email
    tool: email_send
    params:
      to: "user@example.com"
      subject: "Daily Document Summary - {{ date }}"
      body: "{{ steps.extract_text.result }}"
```

#### 执行引擎

```rust
struct PipelineEngine {
    workflows: HashMap<String, Workflow>,
    schedulers: HashMap<String, SchedulerHandle>,
    tool_registry: Arc<Mutex<ToolRegistry>>,
}

struct WorkflowInstance {
    id: String,
    workflow_id: String,
    status: InstanceStatus,  // Running | Completed | Failed | TimedOut
    started_at: Instant,
    step_results: HashMap<String, Value>,
    current_step: usize,
}

enum TriggerType {
    Cron { schedule: String },
    FileWatch { path: String, pattern: String },
    Webhook { port: u16, path: String },
    Manual,
}

impl PipelineEngine {
    async fn execute_step(&self, instance: &mut WorkflowInstance, step: &Step) -> Result<Value>;
    async fn run_workflow(&self, workflow_id: &str, trigger: TriggerType) -> Result<()>;
    async fn start_scheduler(&self, workflow_id: &str) -> Result<()>;
}
```

#### 数据库表

```sql
CREATE TABLE workflows (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    definition TEXT NOT NULL,    -- YAML/JSON workflow definition
    trigger_config TEXT,         -- JSON trigger configuration
    enabled INTEGER DEFAULT 1,
    last_run_at TEXT,
    last_status TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE workflow_runs (
    id TEXT PRIMARY KEY,
    workflow_id TEXT NOT NULL,
    trigger_type TEXT NOT NULL,
    status TEXT NOT NULL,        -- running | completed | failed | cancelled
    step_results TEXT,           -- JSON map of step_id -> result
    error TEXT,
    started_at TEXT NOT NULL,
    finished_at TEXT,
    FOREIGN KEY (workflow_id) REFERENCES workflows(id)
);
```

#### 触发器系统

```
触发器类型:
┌──────────────────────────────────────────┐
│ ⏰ Cron 触发器                            │
│   - 使用 tokio-cron-schedule 或自定义     │
│   - 支持秒级精度                          │
│   - 时区感知                              │
├──────────────────────────────────────────┤
│ 📁 文件监听触发器                         │
│   - 使用 notify crate (跨平台)            │
│   - 支持模式匹配 *.pdf                    │
│   - 防抖 (debounce)                       │
├──────────────────────────────────────────┤
│ 📨 Webhook 触发器                         │
│   - 内嵌 HTTP server (axum/warp)         │
│   - 支持签名验证                          │
│   - 可配置端口和路径                      │
├──────────────────────────────────────────┤
│ 🖱️ 手动触发器                             │
│   - 用户点击"运行"按钮                    │
│   - 对话中 `/run workflow-name` 命令      │
└──────────────────────────────────────────┘
```

### 3.3 研发阶段

| 阶段 | 内容 | 工期 |
|---|---|---|
| **P0: 工作流 DSL + 执行引擎** | YAML 定义解析、顺序执行、步骤间数据传递、错误处理 | 4-5 天 |
| **P1: 触发器系统** | Cron 调度器、文件监听、手动触发 | 3-4 天 |
| **P2: 条件分支** | LLM 驱动的条件判断、循环/重试 | 2-3 天 |
| **P3: 管理界面** | 工作流列表、可视化编辑器、执行历史 | 4-5 天 |
| **P4: 模板市场** | 预置模板、用户分享、一键安装 | 2-3 天 |

**MVP 范围（P0 + Cron 触发器）**：能用 YAML 定义工作流，定时自动执行。

---

## 四、整合路线图

```
                           方向一: MCP 运行时
                           ═══════════════
第 1 周  ████ P0: MCP 客户端核心
第 2 周  ██ P1: ToolRegistry 集成
         ███ P2: 管理界面
第 3 周  ███ P3: MCP 市场
         ██ P4: 安全与权限
                           方向二: 工作流自动化
                           ═══════════════
第 3 周                    ████ P0: 工作流 DSL + 执行引擎
第 4 周                    ███ P1: 触发器系统
第 5 周                    ██ P2: 条件分支
                           ███ P3: 管理界面
第 6 周                    ██ P4: 模板市场
```

### 关键依赖

- 工作流的执行引擎依赖 MCP 运行时提供的丰富工具集
- 条件分支 (P2 of 方向二) 可以在 MCP MVP 之后开始
- 两个方向的管理界面可以共享 UI 组件（卡片、列表、开关）

---

## 五、MCP Rust crate 调研结论

### 5.1 Crate 全景对比

对 crates.io 上所有 MCP 相关 crate 做了完整调研：

| Crate | 版本 | 维护方 | GitHub ⭐ | spawn | list_tools | call_tool | 推荐度 |
|-------|------|--------|-----------|-------|------------|-----------|--------|
| **`rmcp`** | 0.16.0 | **官方** modelcontextprotocol | 3,400★ | ✅ `TokioChildProcess::new(Command)` | ✅ `client.list_all_tools()` | ✅ `client.call_tool()` | ⭐⭐⭐ |
| `rust-mcp-sdk` | 0.9.0 | rust-mcp-stack | 176★ | ✅ `StdioTransport::create_with_server_launch` | ✅ `client.request_tool_list()` | ✅ 通过 handler | ⭐⭐ |
| `hanzo-mcp-client` | 0.6.74 | hanzoai | — | ✅ `McpClient::spawn()` | ✅ `client.list_tools()` | ✅ `client.call_tool()` | ⭐⭐⭐ |
| `turbomcp` | 3.1.4 | Epistates | — | ✅ `Client::connect_stdio()` | ✅ | ✅ | ⭐⭐ |
| `mcpr` | 0.2.3 | conikeec | — | ✅ `StdioTransport` | ✅ | ✅ | ⭐ |
| `mcp_client_rs` | 0.1.7 | darinkishore | — | ✅ `ClientBuilder::spawn_and_initialize()` | ✅ | ✅ | ⭐ |

### 5.2 最终选型：`rmcp`（官方 SDK）

**选型理由**：
- MCP 规范作者（Anthropic）官方维护，协议兼容性有保障
- 486 commits、3.4k stars、频繁发版——社区最大
- 整个初始化握手（`initialize` → `initialized`）由 `serve()` 自动完成
- 最小依赖：仅需 `serde_json` + `tokio::process`

```toml
[dependencies]
rmcp = { version = "0.16", default-features = false, features = ["client"] }
```

### 5.3 MCP 协议核心消息

仅需实现 **4 个 request + 1 个 notification**（`rmcp` 自动处理前 2 个）：

| 阶段 | 方向 | 方法 | 说明 | rmcp 处理 |
|------|------|------|------|-----------|
| 初始化 | C→S | `initialize` | 协议版本协商、能力声明 | ✅ 自动 |
| | S→C | → response | server info + capabilities | ✅ 自动 |
| | C→S | `notifications/initialized` | 通知服务器可操作 | ✅ 自动 |
| 工具 | C→S | `tools/list` | 获取工具列表 | `client.list_all_tools()` |
| | C→S | `tools/call` | 调用工具 name + arguments | `client.call_tool()` |
| 健康 | C→S | `ping` | 心跳检测 | `client.ping()` |

**传输层**：stdio — 客户端通过 `ChildStdin` 写 newline-delimited JSON，通过 `ChildStdout` 读取响应。

### 5.4 桥接代码（~60 行）

```rust
use rmcp::{ServiceExt, model::{CallToolRequestParams, Tool as McpToolDef}, transport::TokioChildProcess};
use crate::tools::trait::Tool;

pub struct McpToolBridge {
    client: Arc<rmcp::service::RunningService<...>>,
    def: McpToolDef,
}

impl Tool for McpToolBridge {
    fn name(&self) -> &str { &self.def.name }
    fn description(&self) -> &str { self.def.description.as_deref().unwrap_or("") }
    fn parameters(&self) -> Value { self.def.input_schema.clone() }
    async fn execute(&self, args: Value) -> Result<Value> {
        let result = self.client.call_tool(
            CallToolRequestParams::new(&self.def.name).with_arguments(args)
        ).await?;
        Ok(serde_json::to_value(result.content)?)
    }
}
```

### 5.5 技术选型总结

| 组件 | 技术 | 理由 |
|---|---|---|
| **MCP 客户端** | **`rmcp` 0.16**（官方 SDK） | 协议兼容性最好，社区最大，~60 行桥接代码 |
| JSON-RPC | rmcp 内置 | 无需额外 crate |
| Cron 调度 | tokio-cron-schedule | 轻量，支持标准 cron 表达式 |
| 文件监听 | notify | 跨平台，Rust 原生 |
| Webhook | axum + tokio | 轻量 HTTP，已有 tokio |
| 工作流 DSL | serde_yaml | 已有依赖 |
| 前端工作流编辑器 | React Flow | 可视化 DAG 编辑 |

---

## 六、风险与缓解

| 风险 | 可能性 | 缓解措施 |
|---|---|---|
| MCP 协议变动 | 中 | 封装协议层，便于适配 |
| MCP Server 进程泄漏 | 中 | 超时 + 进程组 kill |
| 工作流执行死循环 | 低 | 最大步骤数 + 总超时限制 |
| Cron 精度不足 | 低 | tokio-cron 支持秒级，桌面端无需毫秒级 |
| 前端编辑器复杂度 | 高 | MVP 用 YAML 文本编辑，P2 加可视化 |

---

## 七、下一步行动

1. **启动 P0 (MCP 客户端核心)**：添加 `rmcp` 依赖 → 实现 `McpToolBridge` 适配器 → 创建 `McpServerManager` → 从配置 spawn 子进程 → 注册工具到 `ToolRegistry`
2. **第一个可验收里程碑**：在 `config.json` 中配置 `@anthropic/mcp-server-filesystem`，启动应用后在对话中说"列出当前目录的文件"，Agent 自动通过 MCP 工具完成操作
3. **P0 代码量预估**：~200 行 Rust（McpToolBridge + McpServerManager + 配置解析）+ 1 个新 crate 依赖

---

> 📁 本文档路径：`.omo/plans/mcp-and-pipeline-roadmap.md`
> 🔬 技术调研结论：MCP 客户端使用 `rmcp`（官方 SDK），非自实现。协议消息由 SDK 自动处理 3/5。
