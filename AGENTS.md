# AGENTS.md — Agent（Tauri AI 客户端）

## 构建

**真实项目在 `src-tauri/` 中 — 根目录的 `Cargo.toml`/`src/`/`tauri.conf.json` 均已删除。**

```bash
# 终端 1 — 前端开发服务器（Vite，端口 1420）
cd src-ui && npm run dev

# 终端 2 — Tauri 桌面开发（beforeDevCommand 为空，需单独启动）
cd src-tauri && cargo tauri dev

# 生产构建
cd src-ui && npm run build    # 执行 tsc && vite build
cd src-tauri && cargo tauri build
```

### 前置依赖

| 工具     | 版本    | 说明                 |
| -------- | ------- | -------------------- |
| Rust     | 1.77.2+ | edition 2021         |
| Node.js  | 18+     | 用于 src-ui          |
| npm      | —       | 随 Node 捆绑安装     |

---

## 项目结构

```
agent/
├── src-tauri/                   # Tauri Rust 后端（真 · 项目）
│   ├── src/
│   │   ├── lib.rs              # 入口：模块声明 + invoke_handler 注册
│   │   ├── main.rs             # 二进制入口：调用 lib::run()
│   │   ├── commands.rs         # 27 个 IPC 命令（对话、模型、工具、设置、系统提示词）
│   │   ├── commands_provider.rs# 6 个 IPC 命令（Provider CRUD + 状态）
│   │   ├── config.rs           # AppConfig、ModelConfig、ModelProvider 枚举（11 个 provider）
│   │   ├── state.rs            # AppState：通过 Arc<Mutex<>> 共享状态（db, config, providers, tools）
│   │   ├── error.rs            # AppError 枚举 + 类型别名 Result<T>
│   │   ├── api/                # LLM provider 客户端
│   │   │   ├── mod.rs, provider.rs, types.rs
│   │   │   ├── openai.rs       # OpenAI 兼容 API 客户端
│   │   │   └── anthropic.rs    # Anthropic 专用 API 客户端
│   │   ├── agent/
│   │   │   ├── mod.rs, loop.rs # Agent 执行循环（重试×3，4K token 预算，最多 10 个工具调用）
│   │   ├── db/
│   │   │   ├── mod.rs, repository.rs, models.rs  # SQLite，4 张表
│   │   └── tools/
│   │       ├── mod.rs, trait.rs, registry.rs
│   │       ├── calculator.rs   # 默认启用
│   │       ├── file_system.rs  # 默认禁用
│   │       └── web_search.rs   # 默认禁用
│   ├── tests/
│   │   └── tool_calculator.rs  # 计算器测试
│   ├── Cargo.toml              # 真正的清单文件
│   ├── tauri.conf.json         # 真正的配置（带 $schema）
│   ├── Cargo.lock              # 已提交（.gitignore 中无此条目）
│   └── capabilities/
├── src-ui/                     # React + TypeScript 前端
│   ├── src/
│   │   ├── main.tsx, App.tsx
│   │   ├── api/tauri.ts        # 所有 27+ IPC 命令的 invoke() 封装
│   │   ├── store/index.ts      # 单一 Zustand 状态仓库（所有状态 + 异步 IPC 封装）
│   │   ├── types/index.ts      # TypeScript 接口定义
│   │   ├── styles/global.css   # TailwindCSS + 自定义工具类
│   │   └── components/
│   │       ├── ChatArea.tsx, Sidebar.tsx, MessageBubble.tsx
│   │       ├── SettingsModal.tsx, WelcomePage.tsx
│   │       ├── CodeBlock.tsx, ErrorBoundary.tsx
│   └── package.json, vite.config.ts, tsconfig.json, tailwind.config.js, postcss.config.js
├── .gitignore
├── README.md
├── .omo/                       # Agent 计划（已 gitignore）
├── .opencode/                  # OpenCode 项目配置（已 gitignore）
├── openspec/                   # OpenSpec 规约（已 gitignore）
├── docs/
└── gen/
```

---

## 架构

### IPC 命令（已注册 27 个）

路由定义在 `src-tauri/src/lib.rs` 中 — 前端通过 `src-ui/src/api/tauri.ts` 调用 `invoke()`。

| 模块 | 命令 | 用途 |
|------|------|------|
| `commands.rs` | `create/list/get/delete/update*_conversation`、`clear_conversation`、`send_message(_stream)`、`get_messages` | 对话 CRUD + 消息收发 |
| `commands.rs` | `get/add/remove/update/set_default/get_default_model` | 模型管理 |
| `commands.rs` | `update/get_settings` | 设置（键值对） |
| `commands.rs` | `list/toggle_tool` | 工具启用/禁用 |
| `commands.rs` | `create/list/delete/set_default/get_default_system_prompt` | 系统提示词管理 |
| `commands_provider.rs` | `list_providers_cmd`、`setup_provider`、`update_provider_config`、`remove_provider`、`get_provider_models`、`get_available_models` | Provider 配置 |

流式传输：`send_message_stream` 发射 Tauri 事件（`stream_chunk`）— 通过 `listen('stream_chunk', ...)` 监听。

### Provider（11 个硬编码）

定义在 `commands.rs:561` 的 `PROVIDER_OPTIONS` 中：

```
openai, anthropic, google, groq, deepseek, zhipu, moonshot,
siliconflow, ollama, lmstudio, custom
```

`config.rs` 中的 `ModelProvider` 枚举映射上述值；`api/provider.rs` 中的 `ProviderRegistry` 分为两类：
- **OpenAI 兼容**（10 个 provider）→ `OpenAIProvider`
- **Anthropic**（1 个）→ `AnthropicProvider`

### Agent 循环（`src-tauri/src/agent/loop.rs`）

- 重试 ×3，指数退避
- 上下文窗口优化（约 4K token 预算，对较早消息做摘要）
- 每轮最多 10 次工具迭代
- 流式事件：`Content`、`ToolCall`、`ToolResult`、`Done`

### 工具

| 工具 | 默认状态 | 实现文件 |
|------|----------|----------|
| `calculator` | ✅ 启用 | `tools/calculator.rs` |
| `file_system` | ❌ 禁用 | `tools/file_system.rs` |
| `web_search` | ❌ 禁用 | `tools/web_search.rs` |

注册方式：`ToolRegistry::new()` — 新增工具 = 新增 `impl Tool` 文件 + `registry.register()` 一行代码。

### 数据库

- **位置**: `dirs::data_dir()/agent/agent.db`
- **表**: `conversations`、`messages`、`settings`、`system_prompts`
- **迁移**: 旧 `provider` 字段重命名为 `model_id`

### 配置

- **位置**: `dirs::config_dir()/agent/config.json`
- **格式**: JSON，以 `ModelConfig` 对象按模型存储（provider、API key、base URL）
- **默认模型**: GPT-4o（启用）、Llama 3/Ollama（禁用）

---

## 前端注意事项

- **TS 严格模式**: `noUnusedLocals`/`noUnusedParameters` 均开启 → 未使用参数前加 `_` 前缀
- **单一 Zustand 仓库**（`src/store/index.ts`）：所有状态 + 异步 IPC 封装
- **TailwindCSS**：自定义紫色 `primary` 调色板；工具类（`btn-primary`、`input-primary` 等）定义在 `global.css` 中
- **IPC 命名约定**：Rust 端的 `snake_case` 命令名会自动转换为前端 `camelCase`

---

## AI 开发流程（Branch-Per-Session）

采用 **branch-per-session** 工作流：每个 AI 开发会话在独立分支上工作，每次 AI 响应完成后通过 checkpoint 提交"暂存"变更，最终 squash-merge 到 `master`。

### 流程总览

```
master ──┬── session-start ──┬── AI 修改文件 ──┬── checkpoint ──┬── AI 修改文件 ──┬── ... ──┬── session-finish
         │                   │                  │                │                  │          │
         └── ai/feature      └── 响应完成       └── git commit   └── 响应完成       └── ...    └── squash-merge → master
```

### 三个脚本

| 脚本 | 何时执行 | 作用 |
|------|----------|------|
| `scripts/session-start.ps1` | 每次 AI 会话开始时 | 从 `master` 创建/切换到 `ai/<描述>` 分支 |
| `scripts/checkpoint.ps1` | 每次 AI 完整响应后 | 自动 stage + commit 所有变更 |
| `scripts/session-finish.ps1` | 会话结束准备合并时 | 显示变更统计、提交历史，建议 squash-merge |

### 执行步骤

#### 1️⃣ 开始会话

```powershell
.\scripts\session-start.ps1 -Description "add-model-sort"
# → 创建并切换到分支 ai/add-model-sort
```

#### 2️⃣ AI 工作 → 每次完整响应后 → 暂存

```powershell
.\scripts\checkpoint.ps1 -Description "实现模型列表排序 API"
# → git add -A && git commit -m "checkpoint: 实现模型列表排序 API"
```

支持的类型前缀：`checkpoint`（默认）、`wip`、`feat`、`fix`、`refactor`、`chore`、`docs`

```powershell
.\scripts\checkpoint.ps1 -Type feat -Description "添加排序后端的冒泡算法"
```

#### 3️⃣ 会话结束 → Review + 合并

```powershell
.\scripts\session-finish.ps1
# 显示变更统计、提交历史、squash-merge 命令
```

确认无误后合并：

```powershell
git checkout master
git merge --squash ai/add-model-sort
git commit -m "feat: 添加模型排序功能"
git branch -D ai/add-model-sort   # 可选删除分支
```

或直接一条命令完成（慎用，建议先 review）：

```powershell
.\scripts\session-finish.ps1 -Squash
```

### 为什么这么做

- **每次 AI 响应都 checkpoint** → 每步可追溯、可回退
- **独立分支** → 不影响 master，可并行多个会话
- **Squash merge** → master 保持整洁的提交历史
- **Conventional commit** → 最终提交符合项目规范

---

## 常见陷阱

- **CSP 限制**：`connect-src` 白名单在 `src-tauri/tauri.conf.json` 中——新增 provider 或 base URL 需同步添加
- **缺少测试**：仅有 `tests/tool_calculator.rs`；`Cargo.toml` 中未配置测试运行器
- **工具注册**：硬编码在 `ToolRegistry::new()` 中——不支持动态加载
- **`Cargo.lock`**：`src-tauri/Cargo.lock` 已提交（`.gitignore` 中无对应条目），而根目录 `.gitignore` 却有 `Cargo.lock`——操作 git 时注意区分
- **`beforeDevCommand`**：Tauri 配置中此项为空——前端开发服务器需手动启动（`cd src-ui && npm run dev`）
- **双 Cargo.lock 风险**：避免在根目录执行 `cargo build`，可能会生成过时的 `Cargo.lock`。始终在 `src-tauri/` 目录下工作
- **`stream_chunk` 事件**：必须在调用 `send_message_stream` 之前完成监听——前端在 `api/tauri.ts` 中处理此逻辑
- **模型配置字段**：`api_key`、`base_url`、`context_window`、`max_tokens` 均为按模型独立存储，非全局
