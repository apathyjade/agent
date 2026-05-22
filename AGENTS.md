# AGENTS.md — Agent（Tauri AI 客户端）

> 详细文档已拆分到 `docs/agents/` 目录：
> - [架构与项目结构](docs/agents/architecture.md)
> - [AI 开发工作流](docs/agents/workflow.md)
> - [常见陷阱与安全规范](docs/agents/traps.md)

---

## 输出语言

- 始终使用中文回复
- 代码注释、变量命名、git commit 使用英文
- /init 命令生成的 AGENTS.md 也使用中文

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

**前置依赖**：Rust 1.77.2+（edition 2021）、Node.js 18+

---

## 速查

| 场景 | 命令或位置 |
|------|-----------|
| 构建检查 | `cd src-tauri && cargo check` |
| 前端类型检查 | `cd src-ui && npx tsc --noEmit` |
| 添加 IPC 命令 | `commands/` 下新建文件，在 `lib.rs` 注册 handler |
| 添加工具 | `tools/` 下实现 `Tool` trait，`ToolRegistry::new()` 注册 |
| 添加 provider | `config.rs` 的 `ModelProvider` 加枚举值，`api/provider.rs` 注册 |
| 添加运行时 | `environment/registry.rs` 注册，实现 detector + installer |
| 添加记忆种子 | `memory/seeds.rs` 的 `default_seed_memories()` 中添加 |
| 修改 CSP 白名单 | 编辑 `src-tauri/tauri.conf.json` 的 `connect-src` |

---

## 项目一览

| 维度 | 数据 |
|------|------|
| 后端模块 | 17 个（agent, api, commands, config, db, environment, error, keychain, mcp, memory, pipeline, skills, state, tools...） |
| IPC 命令 | ~100 个注册命令 |
| Provider | 11 个（10 OpenAI 兼容 + 1 Anthropic） |
| 内置工具 | 5 个（calculator, file_system, web_search, code_executor, script_tool） |
| 运行时支持 | 8 种（Node, Python, Docker, uv, Go, Rust, Java, Deno） |
| 数据表 | 9 张 |
| 前端组件 | ~30 个 |
| 内置记忆 | 17 条种子记忆，关键词检索注入 Agent 上下文 |

---

## AI 工作流

采用 **branch-per-session** 工作流。AI 会话自动：
1. `node scripts/start-session.mjs <desc>` — 创建分支
2. `node scripts/checkpoint.mjs <desc>` — 每次响应后暂存
3. `node scripts/finish-session.mjs --squash` — 归档合并

详细说明见 [docs/agents/workflow.md](docs/agents/workflow.md)。

---

## 安全规则

- 严禁硬编码 API Key，使用 `process.env` 或 keychain
- 所有数据库操作使用参数化查询
- 避免 `as any`、`@ts-ignore`、`@ts-expect-error`
- 详细规范见 [docs/agents/traps.md](docs/agents/traps.md)
