# AI 开发流程

项目使用**双流程**协作模式：branch-per-session 管理 AI 会话生命周期，OpenSpec 管理功能开发内容生命周期。

---

## 1. Branch-Per-Session（AI 会话管理）

每个 AI 开发会话在独立分支上工作，通过 checkpoint 暂存变更，最终 squash-merge 到 `master`。

### 三个脚本

| 脚本 | 作用 |
|------|------|
| `node scripts/start-session.mjs <描述>` | 从 `master` 创建/切换到 `ai/<描述>` 分支 |
| `node scripts/checkpoint.mjs <描述>` | `git add -A` + `git commit` 暂存所有变更 |
| `node scripts/finish-session.mjs [--squash]` | 查看汇总 → squash-merge 到 master |

### 执行步骤

```bash
# 1. 开始会话
node scripts/start-session.mjs add-model-sort
# → 创建/切换到分支 ai/add-model-sort（基于最新 master）

# 2. 每次 AI 完整响应后暂存
node scripts/checkpoint.mjs "实现模型列表排序 API"

# 带类型前缀（可选）
node scripts/checkpoint.mjs --type feat "添加排序后端"

# 3. 会话结束，归档合并
node scripts/finish-session.mjs          # 查看汇总
node scripts/finish-session.mjs --squash # 确认后一键归档
```

### 支持的类型前缀

`checkpoint`（默认）、`wip`、`feat`、`fix`、`refactor`、`chore`、`docs`

### 为什么这么做

- **每次 checkpoint** → 每步可追溯、可回退
- **独立分支** → 不影响 master，可并行多个会话
- **Squash merge** → master 保持整洁提交历史

---

## 2. OpenSpec Change（功能开发管理）

功能开发遵循 openspec 标准化流程。详见 [openspec-workflow.md](openspec-workflow.md)。

```
想法模糊 → 明确 → /opsx-propose → 审阅 → /opsx-apply → 完成 → /opsx-archive
```

### 与 branch-per-session 的关系

两者互补而非替代：
- **branch-per-session**：管理"如何工作"——何时开始、暂存、合并代码
- **openspec**：管理"做什么"——需求、设计、实施步骤、归档
- 一个 AI 会话里可能完成 1 个或多个 change

---

## 3. 流程示意图

```
AI 会话生命周期（branch-per-session）
  start-session → [开发工作] → checkpoint → ... → finish-session
                        │
                        ▼
              OpenSpec change 在其中执行
              propose → apply → archive
```
