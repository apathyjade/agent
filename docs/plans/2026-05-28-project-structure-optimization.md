# 项目结构优化 — 实施报告

> 日期: 2026-05-28
> 范围: Rust 后端模块组织、React 前端目录结构、API 分层、杂物清理

## 变更清单

### 1. Rust 后端: `commands_provider.rs` 归位

| 改前 | 改后 |
|------|------|
| `src-tauri/src/commands_provider.rs` (孤立文件) | `src-tauri/src/commands/provider.rs` |
| `lib.rs`: `pub mod commands_provider;` | 移除，改为 `commands::provider` 子模块 |

`commands/mod.rs` 新增 `pub mod provider;` + `pub use provider::*;`，所有 6 个 provider 命令通过 `commands::xxx` 统一访问。

### 2. 前端组件目录: 扁平 → 域分组

```
components/                          components/
├── ChatArea.tsx                     ├── session/     (7 文件)
├── ChatInput.tsx                    │   ChatArea, ChatInput, MessageBubble,
├── MessageBubble.tsx                │   SessionConfigPanel, ThinkingPanel,
├── SessionConfigPanel.tsx           │   PlanConfirmDialog, PlanTimeline
├── ThinkingPanel.tsx                ├── management/  (17 文件)
├── PlanConfirmDialog.tsx            │   Skill, Runtime, MCP, Memory,
├── PlanTimeline.tsx                 │   Persona, Workflow, Project, Health,
├── SkillManagerPage.tsx             │   Execution, Lifecycle, Version
├── SkillDetailPanel.tsx             ├── settings/    (3 文件)
├── SkillInstallDialog.tsx           │   SettingsPage, SettingsModal,
├── RuntimeManagerPage.tsx           │   ThemeToggle
├── RuntimeCard.tsx                  └── common/      (9 文件)
├── ... (37 files flat)                  CodeBlock, EmptyState, ErrorBoundary,
                                         Toast, DirectoryPicker, WelcomePage,
                                         ModuleBar, Sidebar, ManagerPageLayout
```

### 3. API 分层: 单文件 → 域文件 (barrel 模式)

```
api/                                 api/
├── tauri.ts         (598 行)        ├── tauri.ts          (barrel, 4 行)
                                     ├── session.ts        (session/message/execution)
                                     ├── config.ts         (provider/model/settings/lifecycle)
                                     └── management.ts     (skill/workflow/mcp/runtime/memory/persona)
```

`tauri.ts` 通过 `export * from './session'` 等转发，**所有现有 `import * as api from '../api/tauri'` 零影响**。

### 4. 杂物清理

| 项目 | 处理 |
|------|------|
| `stdout.log`, `stderr.log` | 已在 `.gitignore`(`*.log`) 覆盖，非跟踪 |
| `__tests__` 组件测试 | 移至对应子目录: `common/__tests__/`, `settings/__tests__/` |
| `EmptyState.tsx` 编码损坏 | 修复已损坏的 UTF-8 中文文本 |

## 构建验证

| 检查项 | 结果 |
|--------|------|
| `cargo check` (Rust) | 0 errors (仅预存警告) |
| `npx tsc --noEmit` (TypeScript) | 0 errors |

## 第二阶段变更（2026-05-28 补完）

### 已解决

| 问题 | 处理方式 | 文件影响 |
|------|---------|---------|
| `execution/orchestrator/pipeline` 重叠 | `execution/` 合并入 `orchestrator/`，`pipeline_adapter` 随之迁移 | 6 files moved, 6 external refs updated |
| `toolSlice`/`promptSlice` 粒度过细 | 合并到 `sessionSlice`，删除 2 个文件 | 3 files changed, 2 deleted |
| `environment/mod.rs` 过大 (625行) | `RuntimeManager` 提取到 `manager.rs` | mod.rs 缩减至 268 行 |

### 当前结构

```diff
 src-tauri/src/
-├── execution/          ← 已删除，合并到 orchestrator/
 ├── orchestrator/
-│   ├── mod.rs, agent.rs, dispatcher.rs, event_bridge.rs, task_graph.rs
+│   ├── mod.rs, agent.rs, dispatcher.rs, event_bridge.rs, task_graph.rs
+│   ├── plan_types.rs   ← from execution/types.rs
+│   ├── planner.rs      ← from execution/planner.rs
+│   ├── runtime.rs      ← from execution/runtime.rs
+│   ├── plan_error.rs   ← from execution/error.rs
+│   └── pipeline_adapter.rs ← from execution/pipeline_adapter.rs
 ├── environment/
+│   ├── manager.rs      ← RuntimeManager 提取

 src-ui/src/store/
-├── toolSlice.ts        ← 已删除
-├── promptSlice.ts      ← 已删除
 └── sessionSlice.ts     ← 合并 tools + systemPrompts 状态
```

### 构建验证

| 检查项 | 结果 |
|--------|------|
| `cargo check` | 0 errors |
| `npx tsc --noEmit` | 0 errors |
