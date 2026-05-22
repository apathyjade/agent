# 项目结构与架构

## 目录结构

```
agent/
├── src-tauri/                   # Tauri Rust 后端
│   ├── src/
│   │   ├── lib.rs              # 入口：模块声明 + invoke_handler 注册（~100 个 IPC 命令）
│   │   ├── main.rs             # 二进制入口
│   │   ├── config.rs           # AppConfig、ModelConfig、ModelProvider 枚举（11 个 provider）
│   │   ├── state.rs            # AppState：通过 Arc<Mutex<>> 共享状态
│   │   ├── error.rs            # AppError 枚举 + Result<T> 别名
│   │   ├── keychain.rs         # 安全凭据存储
│   │   ├── agent/
│   │   │   └── loop.rs         # Agent 执行循环
│   │   ├── api/
│   │   │   ├── provider.rs     # ProviderRegistry
│   │   │   ├── types.rs        # ChatRequest/ChatResponse/ToolCall
│   │   │   ├── openai.rs       # OpenAI 兼容 API 客户端
│   │   │   └── anthropic.rs    # Anthropic API 客户端
│   │   ├── commands/           # 按功能拆分的 IPC 命令
│   │   │   ├── conversation.rs # 对话 CRUD + 消息收发
│   │   │   ├── model.rs        # 模型管理
│   │   │   ├── settings.rs     # 设置
│   │   │   ├── tool.rs         # 工具启用/禁用
│   │   │   ├── prompt.rs       # 系统提示词
│   │   │   ├── skill.rs        # 技能 CRUD + 市场
│   │   │   ├── mcp.rs          # MCP 服务器管理
│   │   │   ├── memory.rs       # 记忆系统
│   │   │   ├── pipeline.rs     # 工作流执行
│   │   │   ├── environment.rs  # 运行时环境
│   │   │   └── window.rs       # 窗口控制
│   │   ├── commands_provider.rs# Provider CRUD + 状态
│   │   ├── db/
│   │   │   ├── models.rs       # 所有数据模型
│   │   │   └── repository.rs   # SQLite CRUD
│   │   ├── tools/
│   │   │   ├── trait.rs        # Tool trait
│   │   │   ├── registry.rs     # ToolRegistry
│   │   │   ├── calculator.rs   # 计算器
│   │   │   ├── file_system.rs  # 文件操作
│   │   │   ├── web_search.rs   # 网络搜索
│   │   │   ├── code_executor.rs# 代码执行沙箱
│   │   │   └── script_tool.rs  # 动态脚本工具
│   │   ├── environment/        # 运行时环境管理
│   │   ├── mcp/                # Model Context Protocol
│   │   ├── memory/             # 记忆系统
│   │   ├── pipeline/           # 工作流引擎
│   │   ├── skills/             # 技能系统
│   │   └── tests/              # 4 个测试文件
│   ├── Cargo.toml
│   ├── tauri.conf.json         # CSP 白名单配置
│   └── capabilities/
├── src-ui/                     # React + TypeScript 前端
│   ├── src/
│   │   ├── main.tsx, App.tsx
│   │   ├── api/tauri.ts        # IPC invoke() 封装
│   │   ├── store/              # Zustand 状态仓库 + slices
│   │   ├── types/index.ts      # TS 接口定义
│   │   ├── styles/global.css   # TailwindCSS + 自定义工具类
│   │   └── components/         # ~30 个组件
│   └── package.json, vite.config.ts, tsconfig.json, tailwind.config.js
├── scripts/                    # AI 工作流脚本（start/checkpoint/finish-session）
├── docs/agents/                # AI 辅助文档
└── .gitignore
```

## IPC 命令（约 100 个）

路由在 `src-tauri/src/lib.rs` 中注册，前端通过 `src-ui/src/api/tauri.ts` 的 `invoke()` 调用。

| 模块 | 功能领域 |
|------|----------|
| `conversation.rs` | 对话 CRUD + 流式/非流式消息 |
| `model.rs` | 模型配置管理 |
| `settings.rs` | 键值对设置 |
| `tool.rs` | 工具启用/禁用 |
| `prompt.rs` | 系统提示词预设 |
| `skill.rs` | 技能全生命周期 + 市场搜索/安装 |
| `mcp.rs` | MCP 服务器连接/断开/日志/工具配置 |
| `memory.rs` | 记忆 CRUD + 关键词检索 |
| `pipeline.rs` | 工作流执行/变量/密钥管理 |
| `environment.rs` | 运行时检测/安装/版本切换/项目绑定 |
| `commands_provider.rs` | Provider 配置 CRUD + 可用模型查询 |

**流式传输**：`send_message_stream` 发射 Tauri 事件 `stream_chunk`，前端通过 `listen('stream_chunk', callback)` 接收。

## Provider（11 个）

| 类型 | Provider |
|------|----------|
| OpenAI 兼容（10 个） | openai, google, groq, deepseek, zhipu, moonshot, siliconflow, ollama, lmstudio, custom |
| Anthropic（1 个） | anthropic（独立客户端实现） |

## Agent 循环（`src-tauri/src/agent/loop.rs`）

- 重试 ×3，指数退避，仅重试可恢复错误（网络超时等）
- 上下文窗口优化：保留 system 消息，从后往前裁剪
- 每轮最多 10 次工具迭代
- 流式事件：`Content` / `ToolCall` / `ToolResult` / `Done`
- **记忆注入**：system prompt 后自动插入 `<remembered_context>` 相关记忆

## 工具

| 工具 | 默认 | 说明 |
|------|------|------|
| `calculator` | ✅ | 算术计算 |
| `file_system` | ❌ | 文件读写操作 |
| `web_search` | ❌ | 网络搜索 |
| `code_executor` | ✅ | 代码执行沙箱 |
| 动态脚本工具 | 按需 | 由 SkillManager 通过 `register_dynamic` 注册 |

新增工具：实现 `Tool` trait + `registry.register()` 一行注册。

## 数据库

- **位置**：`dirs::data_dir()/agent/agent.db`
- **表**：conversations, messages, settings, system_prompts, skills, memories, workflow_runs, runtime_version_cache, bound_projects（9 张）
- **迁移**：`migrate_tables()` 增量执行（添加列/重命名）

## 配置

- **位置**：`dirs::config_dir()/agent/config.json`
- **格式**：JSON，按模型独立存储（provider、api_key、base_url、context_window、max_tokens）
- **默认模型**：GPT-5.5（启用）、Llama 3.3 / Ollama（禁用）

## 前端说明

- **TS 严格模式**：`noUnusedLocals` / `noUnusedParameters` 开启 → 未使用参数前加 `_`
- **状态管理**：单一 Zustand 仓库，按领域拆分为 slices
- **样式**：TailwindCSS + 自定义紫色主题，工具类在 `global.css`
- **IPC 命名**：Rust `snake_case` 命令名，前端 `invoke()` 使用对应驼峰参数
