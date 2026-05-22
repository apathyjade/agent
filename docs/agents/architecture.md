# 项目结构与架构

## 目录结构

```
agent/
├── src-tauri/                   # Tauri Rust 后端（真 · 项目）
│   ├── src/
│   │   ├── lib.rs              # 入口：模块声明 + invoke_handler 注册（103 个 IPC 命令）
│   │   ├── main.rs             # 二进制入口：调用 lib::run()
│   │   ├── config.rs           # AppConfig、ModelConfig、ModelProvider 枚举（11 个 provider）
│   │   ├── state.rs            # AppState：通过 Arc<Mutex<>> 共享状态
│   │   ├── error.rs            # AppError 枚举 + 类型别名 Result<T>
│   │   ├── keychain.rs         # 安全凭据存储
│   │   ├── agent/
│   │   │   └── loop.rs         # Agent 执行循环（重试×3，上下文优化，最多 10 个工具调用）
│   │   ├── api/
│   │   │   ├── provider.rs     # ProviderRegistry（两类：OpenAI 兼容 + Anthropic）
│   │   │   ├── types.rs        # ChatRequest/ChatResponse/ToolCall 等类型
│   │   │   ├── openai.rs       # OpenAI 兼容 API 客户端
│   │   │   └── anthropic.rs    # Anthropic 专用 API 客户端
│   │   ├── commands/           # 按功能拆分的 IPC 命令模块
│   │   │   ├── conversation.rs # 对话 CRUD + 消息收发
│   │   │   ├── model.rs        # 模型管理
│   │   │   ├── settings.rs     # 设置（键值对）
│   │   │   ├── tool.rs         # 工具启用/禁用
│   │   │   ├── prompt.rs       # 系统提示词管理
│   │   │   ├── skill.rs        # 技能 CRUD + 市场
│   │   │   ├── mcp.rs          # MCP 服务器管理
│   │   │   ├── memory.rs       # 记忆系统 CRUD + 检索
│   │   │   ├── pipeline.rs     # 工作流执行
│   │   │   ├── environment.rs  # 运行时环境管理
│   │   │   └── window.rs       # 窗口控制
│   │   ├── commands_provider.rs# Provider CRUD + 状态
│   │   ├── db/
│   │   │   ├── models.rs       # 所有数据模型
│   │   │   └── repository.rs   # SQLite CRUD 操作
│   │   ├── tools/
│   │   │   ├── trait.rs        # Tool trait
│   │   │   ├── registry.rs     # ToolRegistry
│   │   │   ├── calculator.rs   # 计算器（默认启用）
│   │   │   ├── file_system.rs  # 文件操作（默认禁用）
│   │   │   ├── web_search.rs   # 网络搜索（默认禁用）
│   │   │   ├── code_executor.rs# 代码执行沙箱（默认启用）
│   │   │   └── script_tool.rs  # 动态脚本工具
│   │   ├── environment/        # 运行时环境管理（8 种运行时）
│   │   │   ├── detector.rs     # 运行时检测
│   │   │   ├── installer.rs    # 运行时安装
│   │   │   ├── lifecycle.rs    # 版本生命周期
│   │   │   ├── registry.rs     # 运行时注册表
│   │   │   ├── resolver.rs     # 版本解析器
│   │   │   ├── alias.rs        # 版本别名管理
│   │   │   ├── project.rs      # 项目绑定与扫描
│   │   │   ├── node_*.rs       # Node.js 集成
│   │   │   ├── cli.rs          # CLI 工具管理
│   │   │   └── ...             # 其他子模块
│   │   ├── mcp/                # Model Context Protocol
│   │   │   ├── manager.rs      # MCP 服务器管理器
│   │   │   ├── bridge.rs       # MCP 工具桥接
│   │   │   └── config.rs       # MCP 配置类型
│   │   ├── memory/             # 记忆系统
│   │   │   ├── mod.rs          # MemoryManager（CRUD + 检索 + 上下文注入）
│   │   │   └── seeds.rs        # 内置种子记忆（17 条）
│   │   ├── pipeline/           # 工作流引擎
│   │   │   ├── engine.rs       # PipelineEngine（ToolCall/LlmCall/Condition）
│   │   │   ├── scanner.rs      # 工作流文件扫描
│   │   │   └── models.rs       # 工作流定义类型
│   │   ├── skills/             # 技能系统
│   │   │   ├── mod.rs          # SkillManager（安装/卸载/同步）
│   │   │   ├── loader.rs       # skill.yaml 解析
│   │   │   ├── scanner.rs      # 磁盘扫描与 reconcile
│   │   │   └── market.rs       # 技能市场
│   │   └── tests/
│   │       └── tool_calculator.rs
│   ├── Cargo.toml
│   ├── tauri.conf.json         # CSP 白名单在这里
│   └── capabilities/
├── src-ui/                     # React + TypeScript 前端
│   ├── src/
│   │   ├── main.tsx, App.tsx   # 应用入口 + 路由
│   │   ├── api/tauri.ts        # 所有 IPC 命令的 invoke() 封装
│   │   ├── store/index.ts      # 单一 Zustand 状态仓库
│   │   ├── store/*Slice.ts     # 按领域拆分的状态切片
│   │   ├── types/index.ts      # TypeScript 接口定义
│   │   ├── styles/global.css   # TailwindCSS + 自定义工具类
│   │   └── components/         # 29 个组件（见下方）
│   └── package.json, vite.config.ts, tsconfig.json, tailwind.config.js
├── scripts/
│   ├── start-session.mjs       # AI 会话开始脚本
│   ├── checkpoint.mjs          # AI 响应暂存脚本
│   └── finish-session.mjs      # AI 会话归档脚本
├── docs/agents/                # AI 辅助文档（本文件所在目录）
└── .gitignore
```

## IPC 命令（已注册 103 个）

路由定义在 `src-tauri/src/lib.rs` 中 — 前端通过 `src-ui/src/api/tauri.ts` 调用 `invoke()`。

| 模块 | 命令 | 用途 |
|------|------|------|
| `conversation.rs` | create/list/get/delete/update*_conversation, clear_conversation, send_message(_stream), get_messages | 对话 CRUD + 消息收发 |
| `model.rs` | get/add/remove/update/set_default/get_default_model | 模型管理 |
| `settings.rs` | update/get_settings | 设置（键值对） |
| `tool.rs` | list/toggle_tool | 工具启用/禁用 |
| `prompt.rs` | create/list/delete/set_default/get_default_system_prompt | 系统提示词管理 |
| `skill.rs` | list/get_detail/install/uninstall/toggle/configure/reconcile_skill, list_market_top/search/install_market_skill | 技能全生命周期 + 市场 |
| `mcp.rs` | list/add/remove/connect/disconnect/restart/get_logs/update_config/update_startup/get_stats | MCP 服务器管理 |
| `memory.rs` | create/list/get/search/update/delete_memory | 记忆系统 CRUD + 检索 |
| `pipeline.rs` | list/run/list_runs/pause/resume/get_detail/set/delete/list_var/set/delete/list_secret/generate_workflow | 工作流引擎 |
| `environment.rs` | list/get_cached/validate/install/refresh/suggest/...（运行时管理全套） | 运行时环境管理 |
| `commands_provider.rs` | list/setup/update_config/remove/get_provider_models/get_available_models | Provider 配置 |

流式传输：`send_message_stream` 发射 Tauri 事件（`stream_chunk`）— 通过 `listen('stream_chunk', ...)` 监听。

## Provider（11 个）

| Provider | API 类型 | 备注 |
|----------|----------|------|
| openai, google, groq, deepseek, zhipu, moonshot, siliconflow, ollama, lmstudio, custom | OpenAI 兼容 | 统一使用 OpenAIProvider |
| anthropic | Anthropic | 使用 AnthropicProvider（独立实现） |

定义在 `commands/mod.rs` 的 `PROVIDER_OPTIONS` 中。`config.rs` 的 `ModelProvider` 枚举映射上述值。

## Agent 循环（`src-tauri/src/agent/loop.rs`）

- 重试 ×3，指数退避（仅重试可恢复错误，如网络超时）
- 上下文窗口优化（保留 system 消息，从后往前裁剪直到 token 预算内）
- 每轮最多 10 次工具迭代
- 流式事件：`Content`、`ToolCall`、`ToolResult`、`Done`
- **记忆注入**：在 system prompt 后自动插入 `<remembered_context>` 相关记忆

## 工具

| 工具 | 默认状态 | 实现文件 |
|------|----------|----------|
| `calculator` | ✅ 启用 | `tools/calculator.rs` |
| `file_system` | ❌ 禁用 | `tools/file_system.rs` |
| `web_search` | ❌ 禁用 | `tools/web_search.rs` |
| `code_executor` | ✅ 启用 | `tools/code_executor.rs` |
| 动态脚本工具 | 按需 | `tools/script_tool.rs`（由 SkillManager 注册） |

注册方式：`ToolRegistry::new()` — 新增工具 = 新增 `impl Tool` 文件 + `registry.register()` 一行代码。

## 数据库

- **位置**: `dirs::data_dir()/agent/agent.db`
- **表**: conversations, messages, settings, system_prompts, skills, workflow_runs, runtime_version_cache, bound_projects, **memories**
- **迁移**: 通过 `migrate_tables()` 增量执行（添加列、重命名等）

## 配置

- **位置**: `dirs::config_dir()/agent/config.json`
- **格式**: JSON，以 `ModelConfig` 对象按模型存储（provider、API key、base URL）
- **默认模型**: GPT-5.5（启用）、Llama 3.3/Ollama（禁用）

## 前端注意事项

- **TS 严格模式**: `noUnusedLocals`/`noUnusedParameters` 均开启 → 未使用参数前加 `_` 前缀
- **单一 Zustand 仓库**（`src/store/index.ts`）：所有状态 + 异步 IPC 封装，按领域拆分为 slices（ui, conversation, model, tool, prompt, skill, mcp, **memory**, runtime, workflow）
- **TailwindCSS**：自定义紫色 `primary` 调色板；工具类（`btn-primary`、`input-primary` 等）定义在 `global.css` 中
- **IPC 命名约定**：Rust 端的 `snake_case` 命令名会自动转换为前端 `camelCase`
- **29 个组件**：包括 ChatArea, Sidebar, MessageBubble, WelcomePage, SettingsPage, SkillManagerPage, McpManagerPage, **MemoryManagerPage**, RuntimeManagerPage, WorkflowManagerPage, HealthCenter, Toast 等
