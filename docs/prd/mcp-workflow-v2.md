# Agent 产品需求文档 V2：MCP 运行时 + 工作流自动化

> 基于 V1 路线图执行后的深度升级 | 创建：2026-05-21

---

## 一、现状与目标

### 已完成 (V1 MVP)
- MCP Server 连接/断开、工具自动发现与注册、手动添加/移除
- 工作流 YAML DSL 解析、顺序执行引擎、YAML 文件发现、手动触发

### 核心差距
| 模块 | 已做 | 缺失 |
|------|------|------|
| MCP | 基础连接 + 工具注册 | 安全控制、健康监控、模板库、多传输 |
| 工作流 | YAML + 顺序执行 | 定时触发、真实 LLM 步骤、可视化、错误处理 |
| 产品化 | 功能骨架 | 用户引导、确认流程、监控面板 |

### V2 目标
将两个模块从"能跑的功能骨架"升级为**可交付的产品特性**。

---

## 二、功能规格

### ━━━ 模块 A：MCP 运行时升级 ━━━

#### A1. 工具精细曝光控制
**问题**：MCP Server 的全部工具暴露给 Agent，用户无法选择。
**需求**：每个 MCP Server 连接后，列出工具列表，用户可以逐工具开启/关闭。

```
📁 Filesystem Server (5 tools, 3 enabled)
  ☑ read_file          ✓ 已启用
  ☑ list_directory     ✓ 已启用
  ☑ search_files       ✓ 已启用
  ☐ write_file         ✗ 已禁用
  ☐ delete_file         ✗ 已禁用
  [_设为默认模板_]  [保存我的选择]
```

**实现要点**：
- 每个 McpTool 在 ToolRegistry 中的 enabled 状态独立存储
- 前端 Server 卡片展开显示工具列表，每行一个开关
- 默认安全策略：写操作/删除操作默认关闭

**验收**：连接 Filesystem Server → 展开工具列表 → 关闭 write_file → Agent 对话中不再出现 write_file 工具

---

#### A2. 工具调用确认模式
**问题**：Agent 调用 MCP 工具没有任何用户感知。
**需求**：可配置的确认弹窗，类似 Claude Desktop 的 "Allow / Allow Always / Deny"。

```
┌──────────────────────────────────────────────┐
│ ⚠️ Agent 想要执行工具: write_file              │
│                                              │
│ 📂 文件: /home/user/important.txt             │
│ 📝 内容: "Updated report..."                  │
│ 🔗 来源: Filesystem Server                    │
│                                              │
│ [允许本次]  [总是允许]  [拒绝]                 │
└──────────────────────────────────────────────┘
```

**权限策略**：
| 级别 | 行为 |
|------|------|
| `auto_allow` | 直接执行，不弹窗（仅限安全的只读工具） |
| `confirm_once` | 每次弹窗确认 |
| `confirm_session` | 本次对话内允许（记到对话结束） |
| `deny` | 直接拒绝 |

**实现要点**：
- 新增 `tool_confirmations` 配置表（保存在 AppConfig 中）
- AgentLoop 执行前检查确认策略
- 前端监听 `tool_confirm_request` 事件 → 弹窗 → 返回用户选择
- "总是允许"记录到用户配置，下次不再问

**验收**：write_file 设为 confirm_once → 对话中 Agent 调用 write_file → 弹窗确认 → 用户点允许 → 执行成功

---

#### A3. 连接健康监控
**问题**：MCP Server 进程是否存活、是否正常响应，用户完全不知道。
**需求**：实时显示每个 Server 的健康状态。

```
📁 Filesystem Server          ● 正常  运行 3m  PID: 12456
  最近调用: search_files (12ms)  1 分钟前
  总调用: 47 次  |  失败: 0 次

🗄️ PostgreSQL Server          ● 异常  运行 2m (已崩溃)
  最近调用: query (timeout)  3 分钟前
  总调用: 12 次  |  失败: 3 次
  [重新连接]  [查看日志]
```

**实现要点**：
- 定期 ping（MCP ping 方法）检测存活性
- 记录工具调用延迟和成功率
- 进程崩溃后自动重试（最多 3 次，指数退避）
- 前端显示最近一次错误信息

**验收**：连接 Server → 切换到 MCP 标签 → 看到运行时间和调用统计 → 手动 kill 进程 → 状态变为异常

---

#### A4. 预置 Server 模板库
**问题**：每个 MCP Server 都需要手动输入 npx 命令，门槛高。
**需求**：内置常用 MCP Server 的一键配置模板。

| 模板 | 命令 | 需要配置 |
|------|------|----------|
| 📁 Filesystem | `npx -y @anthropic/mcp-server-filesystem {path}` | 路径 |
| 🐙 GitHub | `npx -y @anthropic/mcp-server-github` | GITHUB_TOKEN |
| 🗄️ PostgreSQL | `npx -y @anthropic/mcp-server-postgres {url}` | 连接串 |
| 💬 Slack | `npx -y @anthropic/mcp-server-slack` | SLACK_TOKEN |
| 🌐 Brave Search | `npx -y @anthropic/mcp-server-brave-search` | BRAVE_API_KEY |
| 🖥️ Puppeteer | `npx -y @anthropic/mcp-server-puppeteer` | 无需 |

**实现要点**：
- 内置模板 JSON 文件（`templates/mcp-servers.json`）
- 添加对话框改为"从模板创建"+"自定义"两个 Tab
- 模板只需填必填字段（API key、路径），其余自动

**验收**：添加 MCP Server → 选择 GitHub 模板 → 填入 Token → 一键连接

---

### ━━━ 模块 B：工作流自动化升级 ━━━

#### B1. 定时触发器 (Cron)
**问题**：工作流只能手动触发，无法自动运行。
**需求**：支持 Cron 表达式定时触发。

```yaml
trigger:
  type: cron
  schedule: "0 9 * * *"       # 每天早上 9 点
  # schedule: "0 */2 * * *"   # 每 2 小时
  # schedule: "30 8 * * 1-5"  # 工作日 8:30
```

**实现要点**：
- 使用 `tokio-cron-schedule` crate 解析 Cron 表达式
- 启动时扫描所有工作流文件，为每个有 cron trigger 的工作流创建调度任务
- 工作流文件变更时重新扫描
- 前端显示"下次执行时间"、"上次执行时间"、"Cron 表达式"
- 支持暂停/恢复调度

**验收**：创建工作流 → 设置 Cron `*/1 * * * *` → 等待 1 分钟 → 看到执行记录新增

---

#### B2. LLM 步骤真实调用
**问题**：工作流中的 LLM 步骤只是模板渲染，没有真正调用 AI 模型。
**需求**：LLM 步骤通过 ProviderRegistry 调用模型，返回真实的 AI 生成内容。

```yaml
steps:
  - id: analyze
    type: llm_call
    prompt: "分析以下内容并提取关键信息：{{ steps.extract.result }}"
    model_id: ""              # 空 = 使用默认模型
    system_prompt: "你是数据分析专家"
    max_tokens: 2000
    temperature: 0.3
```

**实现要点**：
- `PipelineEngine` 持有 `Arc<Mutex<ProviderRegistry>>` 引用
- LLM 步骤构建 ChatRequest → 调用 `provider.chat()` → 提取 `choices[0].message.content`
- 支持 `model_id`（空则用默认）、`system_prompt`、`max_tokens`、`temperature`
- 错误时记录到运行历史

**验收**：工作流含 LLM 步骤 → 运行 → LLM 真正返回生成内容

---

#### B3. 步骤执行时间线
**问题**：工作流运行时是黑盒，用户不知道进度。
**需求**：实时显示每个步骤的执行状态和耗时。

```
┌──────────────────────────────────────────┐
│ 📧 Daily Summary                       │
│                                         │
│ ✅ 1. scan_files      12ms  已完成      │
│    → 找到 3 个 PDF 文件                  │
│                                         │
│ ✅ 2. analyze_content  2.3s  已完成      │
│    → LLM 返回摘要                       │
│                                         │
│ ⏳ 3. send_email       运行中...         │
│                                         │
│ ❌ 4. archive          失败              │
│    → 文件权限不足                        │
└──────────────────────────────────────────┘
```

**实现要点**：
- `PipelineEngine::run()` 通过 channel 发送步骤进度事件
- 工作流运行 ID 推送到前端，前端轮询或通过事件获取最新状态
- `workflow_runs` 表字段：`step_statuses` JSON（每个步骤的状态、耗时、错误）

**验收**：运行工作流 → 设置页面看到步骤逐个更新状态和耗时

---

#### B4. 错误处理策略
**问题**：工作流中任何步骤失败都终止整个工作流。
**需求**：每个步骤可配置错误处理。

```yaml
steps:
  - id: risky_step
    type: tool_call
    tool: web_search
    params:
      query: "latest news"
    retry:
      max: 3
      delay_seconds: 5
    on_error: skip       # skip | fail | fallback_step_id
    timeout_seconds: 30
```

**验收**：步骤失败 → 自动重试 3 次 → 仍然失败 → 按 `on_error` 策略处理

---

### ━━━ 模块 C：产品化 ━━━

#### C1. 可视化工作流编辑器
使用 React Flow 实现节点的拖拽、连线、参数编辑。

#### C2. AI 生成工作流
"我想要每天早上扫描 Downloads 中的 PDF 并发送摘要邮件" → LLM → 自动生成 workflow.yaml。

#### C3. 工作流变量与密钥
`{{ vars.EMAIL_TO }}` 和 `{{ secrets.API_KEY }}` 集中管理，不硬编码。

---

## 三、研发计划

### Phase 1：工作流引擎升级（核心价值）

| 任务 | 说明 | 预估 |
|------|------|------|
| **B1. Cron 触发器** | tokio-cron-schedule 集成 + 调度管理 + 暂停/恢复 | 2-3h |
| **B2. LLM 真实调用** | PipelineEngine 接入 ProviderRegistry | 1-2h |
| **B3. 步骤时间线** | 执行事件流 + 前端实时进度 | 2-3h |
| **B4. 错误处理** | retry + on_error + timeout | 2h |

**Phase 1 总预估**：7-10h（1-2 天）

---

### Phase 2：MCP 安全与控制

| 任务 | 说明 | 预估 |
|------|------|------|
| **A1. 工具精细控制** | 逐工具 enable/disable + 前端 UI | 2-3h |
| **A2. 确认模式** | IPC 事件 + 前端弹窗 + 策略持久化 | 2-3h |
| **A3. 健康监控** | ping + 调用统计 + 自动重连 | 2-3h |
| **A4. 模板库** | 内置 JSON + 一键配置 UI | 1-2h |

**Phase 2 总预估**：7-11h（1-2 天）

---

### Phase 3：可视化与产品化

| 任务 | 说明 | 预估 |
|------|------|------|
| C1. 可视化编辑器 | React Flow 拖拽编辑 | 5-8h |
| C2. AI 生成 | openai chat 生成 YAML | 2-3h |
| C3. 变量密钥 | secrets/vars 管理 | 2-3h |

**Phase 3 总预估**：9-14h（2-3 天）

---

### 里程碑

```
Day 1-2  ████████ Phase 1: Cron + LLM + Timeline + Error
Day 3-4  ████████ Phase 2: MCP Control + Confirm + Health + Templates
Day 5-7  ████████████ Phase 3: Visual Editor + AI Gen + Secrets
```

---

## 四、验收清单

### Phase 1 完成标志
- [ ] 工作流设置 Cron `*/1 * * * *` 后自动每分钟执行
- [ ] 工作流中的 LLM 步骤返回真实 AI 生成内容
- [ ] 运行工作流时前端看到步骤逐个更新
- [ ] 步骤失败按 on_error 策略处理

### Phase 2 完成标志
- [ ] MCP Server 的工具列表中每个工具可独立开关
- [ ] 危险工具执行前弹窗确认
- [ ] MCP 连接断开后自动重连
- [ ] 一键使用模板添加 Server

### Phase 3 完成标志
- [ ] 拖拽节点创建工作流（不需要手写 YAML）
- [ ] 自然语言描述 → 生成工作流 → 运行
- [ ] 密钥集中管理，工作流中引用
