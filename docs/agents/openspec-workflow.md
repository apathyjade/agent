# OpenSpec 工作流

OpenSpec 是项目的**功能开发工作流**——从需求到实施的标准化流程，通过 `openspec` CLI 管理变更生命周期。与 branch-per-session 的关系见 [workflow.md](workflow.md)。

## 核心概念

一个 **change** 代表一次独立的功能开发或修复。每个 change 包含四个层级：

```
openspec/changes/<change-name>/
├── proposal.md       产品提案（做什么、为什么）
├── design.md         技术设计（架构选择、数据流）
├── specs/            详细规格（每个能力的 spec.md）
└── tasks.md          可执行的实施步骤
```

## 命令速查

| 命令 | 作用 | 何时使用 |
|------|------|----------|
| `/opsx-propose` | 一步生成 proposal + design + specs + tasks | 需求明确，快速出稿 |
| `/opsx-explore` | 探索模式，理清思路再创建 change | 需求模糊，需要先讨论 |
| `/opsx-apply` | 按 tasks.md 逐步实施 | 进入编码阶段 |
| `/opsx-archive` | 完成后归档 change | 实施完毕，清理工作分支 |

## 标准流程

```
  想法模糊                    想法清晰                    实施                    完成
    │                          │                        │                      │
    ▼                          ▼                        ▼                      ▼
┌─────────┐            ┌──────────────┐          ┌──────────┐          ┌──────────────┐
│ explore │ ──→ 明确 ─→ │   propose   │ ──→ 审阅 ─→ │  apply   │ ──→ 完成 ─→ │   archive   │
└─────────┘            └──────────────┘          └──────────┘          └──────────────┘
                              │                        │
                              ▼                        ▼
                         proposal.md              tasks.md 逐条实施
                         design.md
                         specs/**/spec.md
```

## 产物与 docs/ 的关系

change 产物在 `openspec/changes/<name>/` 下，是工具工作目录。有长期参考价值的文档在 archive 后复制到 `docs/`（详见 [AGENTS.md 文档约定](../../AGENTS.md#文档约定)）。

## 项目中的 Change 历史

| Change | 状态 | 说明 |
|--------|------|------|
| `fix-chat-broken` | ✅ complete | 修复 provider 配置导致聊天中断 |
| `add-streaming-abort` | 🔄 in-progress | 流式传输中断能力 |
| `local-skill-management` | 📦 archived | 本地技能管理功能 |
| `intent-routing` | 📦 archived | 意图路由功能 |
