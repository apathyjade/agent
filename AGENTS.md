# AGENTS.md — Agent（Tauri AI 客户端）

> 详细文档见 `docs/agents/`：
> - [项目概览](docs/agents/overview.md) — 技术栈、目录结构、**Rig AI 框架**、工具系统
> - [开发流程](docs/agents/workflow.md) — branch-per-session + OpenSpec 双流程
> - [OpenSpec 工作流](docs/agents/openspec-workflow.md) — 功能开发标准化流程
> - [常见陷阱](docs/agents/traps.md) — 项目配置、消息流、MCP 等注意事项
> - [安全编码与依赖管理](docs/agents/safety.md) — 安全红线、第三方依赖规范

---

## 核心依赖

| 依赖 | 用途 | 说明 |
|------|------|------|
| **Rig v0.37** | AI 基础设施 | LLM provider、embeddings、extraction、streaming — 统一 AI 框架 |
| Tauri 2.x | 桌面框架 | Rust 后端 + React 前端 |
| SQLite | 持久化 | 消息、记忆、配置存储 |

> Rig 是项目的 AI 基础框架。所有 LLM 通信（chat、stream、tool calling、embeddings、structured output）统一通过 Rig 驱动。
> 详见 `docs/agents/overview.md` → AI 基础设施 — Rig Framework。

---

## 输出语言

- 始终使用中文回复
- 代码注释、变量命名、git commit 使用英文
- /init 命令生成的 AGENTS.md 也使用中文

---

## 文档约定

所有产出文档统一归入 `docs/` 下的子目录，不分散到多个位置。

| 类型 | 存放位置 | 说明 |
|------|----------|------|
| 产品 PRD / 需求文档 | `docs/prd/` | 做什么、为什么 |
| 规格 / 详细需求 | `docs/specs/` | 每个能力的详细规格 |
| 架构 / 设计文档 | `docs/design/` | 怎么做（技术设计） |
| 实施计划 | `docs/plans/` | 可执行的步骤拆分 |
| 路线图 | `docs/roadmap/` | 长期规划 |

**工具工作目录规则：**
- `.omo/plans/` — OpenCode 内部 plan 缓存，**不**作为文档源。有参考价值的产物复制到 `docs/` 对应目录。
- `openspec/changes/` — OpenSpec change 产物（proposal/design/specs/tasks），在 change 生命周期内以那里为准。archive 后关键内容摘到 `docs/` 对应目录。
- 不要在 `docs/` 根目录直接放文档，使用子目录归类。

**AI agent 规则：**
- 生成文档时写入 `docs/{prd,specs,design,plans,roadmap}/` 对应目录，不写别处。
- `.omo/` 和 `openspec/` 是工具私有空间，不直接写文档到这两处之外。

---

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

### 开发场景

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

### 工作流命令

| 场景 | 命令 |
|------|------|
| 开始 AI 开发会话 | `node scripts/start-session.mjs <desc>` |
| 暂存当前进度 | `node scripts/checkpoint.mjs <描述>` |
| 结束会话并合并 | `node scripts/finish-session.mjs [--squash]` |
| 开始功能开发 | `/opsx-propose` |
| 实施功能开发 | `/opsx-apply` |
| 归档已完成功能 | `/opsx-archive` |

---

## 安全红线

详细规范见 [docs/agents/safety.md](docs/agents/safety.md)，核心红线：

- **API Key / Token** — 严禁硬编码，使用 `process.env` 或 keychain
- **SQL 注入** — 全部使用参数化查询（`rusqlite::params!`）
- **日志脱敏** — 禁止打印密码、API Key 等敏感信息
- **类型安全** — 禁止 `as any`、`@ts-ignore`、`@ts-expect-error`

### Sisyphus 自动遵循的规则

| 阶段 | 行为 |
|------|------|
| **会话开始** | 自动执行 `start-session.mjs` 创建/切换到 `ai/<desc>` 分支 |
| **每次完成任务** | 执行 `checkpoint.mjs` 提交所有变更到当前分支 |
| **功能开发** | 通过 openspec 流程管理（propose → apply → archive） |
| **会话结束** | 执行 `finish-session.mjs --squash` 归档到 master 并清理分支 |
