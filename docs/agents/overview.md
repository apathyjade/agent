# 项目结构与架构概览

## 技术栈

| 层 | 技术 |
|----|------|
| 框架 | Tauri 2.x |
| 后端 | Rust（Tokio、reqwest、rusqlite） |
| 前端 | React 18 + TypeScript + Vite |
| 状态管理 | Zustand |
| 样式 | TailwindCSS + 自定义紫色主题 |
| 数据库 | SQLite |

## 目录结构

```
agent/
├── src-tauri/                        # Tauri Rust 后端
│   ├── src/
│   │   ├── lib.rs                   # 入口：模块声明 + invoke_handler
│   │   ├── main.rs                  # 二进制入口
│   │   ├── config.rs                # AppConfig、ModelProvider 枚举
│   │   ├── state.rs                 # AppState（Arc<Mutex<>> 共享状态）
│   │   ├── error.rs                 # AppError + Result<T>
│   │   ├── keychain.rs              # 安全凭据存储
│   │   ├── agent/loop.rs            # Agent 执行循环
│   │   ├── api/                     # LLM API 客户端（openai, anthropic）
│   │   ├── commands/                # IPC 命令，按功能拆分
│   │   ├── db/                      # SQLite 存储（models + repository）
│   │   ├── tools/                   # 工具系统（calculator, file_system 等）
│   │   ├── environment/             # 运行时环境管理
│   │   ├── mcp/                     # Model Context Protocol
│   │   ├── memory/                  # 记忆系统
│   │   ├── pipeline/                # 工作流引擎
│   │   ├── skills/                  # 技能系统
│   │   └── tests/                   # 测试
│   ├── Cargo.toml
│   └── tauri.conf.json              # CSP 白名单配置
├── src-ui/                          # React 前端
│   ├── src/
│   │   ├── api/tauri.ts             # IPC invoke() 封装
│   │   ├── store/                   # Zustand slices
│   │   ├── types/index.ts           # TS 接口定义（严格模式）
│   │   └── components/              # ~30 个组件
│   └── package.json + 构建配置
├── scripts/                         # AI 工作流脚本
└── docs/                            # 项目文档
```

## Provider 系统

支持 11 个 LLM Provider，模型配置按模型独立存储（`api_key`、`base_url`、`context_window`、`max_tokens`），非全局共享：

| 类型 | Provider |
|------|----------|
| OpenAI 兼容（10） | openai, google, groq, deepseek, zhipu, moonshot, siliconflow, ollama, lmstudio, custom |
| Anthropic（1） | anthropic（独立客户端实现） |

## IPC 通信

约 100 个 IPC 命令，通过 Tauri `invoke()` 调用。Rust 端 `snake_case` 命名，前端 `invoke()` 使用驼峰参数。
流式传输通过 Tauri 事件 `stream_chunk` 实现，前端通过 `listen('stream_chunk', callback)` 接收。

## Agent 循环（`agent/loop.rs`）

- 重试 ×3，指数退避，仅重试可恢复错误（网络超时等）
- 上下文窗口优化：保留 system 消息，从后往前裁剪
- 每轮最多 10 次工具迭代
- 流式事件：`Content` / `ToolCall` / `ToolResult` / `Done`
- 记忆注入：system prompt 后自动插入 `<remembered_context>`

## 工具系统

| 工具 | 默认启用 | 说明 |
|------|----------|------|
| `calculator` | ✅ | 算术计算 |
| `file_system` | ❌ | 文件读写 |
| `web_search` | ❌ | 网络搜索 |
| `code_executor` | ✅ | 代码执行沙箱 |
| 动态脚本工具 | 按需 | 由 SkillManager 注册 |

## 数据库

- **位置**：`dirs::data_dir()/agent/agent.db`
- **迁移**：`migrate_tables()` 增量执行（添加列/重命名）
- **表**：conversations, messages, settings, system_prompts, skills, memories, workflow_runs, runtime_version_cache, bound_projects

## 更多

- [开发流程](workflow.md) — 了解如何开始编码
- [OpenSpec 工作流](openspec-workflow.md) — 功能开发的标准化流程
