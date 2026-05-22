# AI 开发流程（Branch-Per-Session）

采用 **branch-per-session** 工作流：每个 AI 开发会话在独立分支上工作，每次 AI 响应完成后通过 checkpoint 提交"暂存"变更，最终 squash-merge 到 `master`。

## 流程总览

```
master ──┬── start-session ──┬── AI 修改文件 ──┬── checkpoint ──┬── ... ──┬── finish-session
         │                   │                  │                │          │
         └── ai/feature      └── 响应完成       └── git commit   └── ...    └── squash-merge → master
```

## 三个 Node.js 脚本

| 脚本 | 何时执行 | 作用 |
|------|----------|------|
| `node scripts/start-session.mjs <描述>` | 每次 AI 会话开始时 | 从 `master` 创建/切换到 `ai/<描述>` 分支 |
| `node scripts/checkpoint.mjs <描述>` | 每次 AI 完整响应后 | 自动 `git add -A` + `git commit` 暂存所有变更 |
| `node scripts/finish-session.mjs` | 会话结束准备合并时 | 显示变更统计、提交历史，建议或执行 squash-merge |

## 执行步骤

### 1️⃣ 开始会话 → 自动创建分支

```bash
node scripts/start-session.mjs add-model-sort
# → 创建并切换到分支 ai/add-model-sort（基于最新 master）
```

### 2️⃣ 每次 AI 完整响应后 → 暂存

```bash
# 基本用法
node scripts/checkpoint.mjs "实现模型列表排序 API"
# → git add -A && git commit -m "checkpoint: 实现模型列表排序 API"

# 指定提交类型前缀
node scripts/checkpoint.mjs --type feat "添加排序后端的冒泡算法"
```

支持的类型前缀：`checkpoint`（默认）、`wip`、`feat`、`fix`、`refactor`、`chore`、`docs`

### 3️⃣ 会话结束 → 归档合并

```bash
# 查看汇总
node scripts/finish-session.mjs

# 确认无误后一键归档（squash-merge + 可选删分支）
node scripts/finish-session.mjs --squash
```

或手动分步操作：

```bash
git checkout master
git merge --squash ai/add-model-sort
git commit -m "feat: 添加模型排序功能"
git branch -D ai/add-model-sort   # 可选删除
```

## Sisyphus 自动遵循的工作流

每次 AI 会话（Sisyphus）自动遵循以下规则：

| 阶段 | Sisyphus 行为 |
|------|---------------|
| **会话开始** | 自动执行 `start-session.mjs` 创建/切换到 `ai/<desc>` 分支 |
| **每次完成任务** | 执行 `checkpoint.mjs` 提交所有变更到当前分支 |
| **会话结束** | 执行 `finish-session.mjs --squash` 归档到 master 并清理分支 |

## 为什么这么做

- **每次 AI 响应都 checkpoint** → 每步可追溯、可回退
- **独立分支** → 不影响 master，可并行多个会话
- **Squash merge** → master 保持整洁的提交历史
- **Node.js 脚本** → Windows/macOS/Linux 统一运行
