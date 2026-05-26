# Persona 虚拟人系统 — PRD

## 概述

在 AI 编码助手中引入 **虚拟人（Persona）** 概念，让用户能够创建、选择、切换不同的"角色人设"来完成任务。每个 Persona 拥有独立的身份设定、知识记忆和工具配置，实现"用不同的人做不同的事"。

## 核心概念

```
User (真实用户) ── 创建/管理 ── Persona (虚拟人)
                                  ├── 身份: 名称、头衔、系统提示词
                                  ├── 记忆: 该虚拟人知道的专属知识
                                  ├── 角色: 关联的角色标签 (用于继承基础记忆)
                                  ├── 项目绑定: 绑定到特定项目
                                  ├── 工具权限: 可用的工具集
                                  └── 模型配置: 模型、温度等参数
```

## Persona 选择策略（三态决策机）

| 模式 | 触发条件 | 行为 |
|------|----------|------|
| **手动模式** | 用户消息含 Persona 前缀（"Alex, 写个测试"）或命令 `/persona Alex` | 直接切换到指定 Persona |
| **自动模式** | 未指定，但系统能通过检测器推断（检测到 Cargo.toml → Rust 开发者） | 静默激活最匹配的 Persona |
| **选择模式** | 无法确定（多种语言项目、多条消息无上下文） | 弹窗让用户选择 |

### 检测器管线

1. **项目语言检测**：Cargo.toml → Rust 类、package.json → TS/JS 类
2. **任务类型推断**："重构"→ 架构师、"测试 review" → QA
3. **最近使用**：跨会话统计，倾向高频 Persona
4. **项目绑定**：persona_projects 显式绑定的 Persona

### 回退链

```
手动指定 → 会话内已有激活 → 项目绑定 → 自动检测(差距>=20%) → 默认Persona
→ 最近使用 → 选择模式弹窗
```

## V1 范围

### 包含

- [x] Persona 核心 CRUD（创建/列表/查看/编辑/删除）
- [x] Persona 数据表（personas + persona_memories + persona_projects）
- [x] 手动模式（消息前缀解析 `"Alex, xxx"`）
- [x] 自动模式（项目文件检测器）
- [x] 未指定时使用默认 Persona
- [x] Persona system_prompt 注入到 Agent 上下文
- [x] Persona 记忆过滤（只检索该 Persona 关联的记忆）
- [x] 前端 Persona 管理页面
- [x] 默认种子 Persona（开发者、架构师）
- [x] ModuleBar 入口

### 不包含（V2+）

- 多 Persona 对话/辩论模式
- 自动记忆提取到 Persona
- 工具权限粒度控制
- 知识图谱关系
- 向量嵌入语义搜索
- 建议切换弹窗

## 数据模型

```sql
CREATE TABLE personas (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL UNIQUE,     -- "Alex"
    title           TEXT NOT NULL DEFAULT '',  -- "资深Rust开发者"
    emoji           TEXT DEFAULT '🧑‍💻',
    description     TEXT DEFAULT '',
    system_prompt   TEXT NOT NULL,             -- 人设核心注入词
    temperature     REAL DEFAULT 0.3,
    response_style  TEXT DEFAULT 'concise',    -- verbose|concise|academic
    model_provider  TEXT DEFAULT '',
    model_name      TEXT DEFAULT '',
    is_default      INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE TABLE persona_memories (
    persona_id  TEXT NOT NULL REFERENCES personas(id) ON DELETE CASCADE,
    memory_id   TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    PRIMARY KEY (persona_id, memory_id)
);

CREATE TABLE persona_projects (
    persona_id  TEXT NOT NULL REFERENCES personas(id) ON DELETE CASCADE,
    project_path TEXT NOT NULL,
    auto_select INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (persona_id, project_path)
);
```

## 架构集成

```
用户输入 → PersonaSelector.resolve()
  ├─ manual → 指定 Persona
  ├─ auto → 检测器 → 匹配 Persona
  └─ none → 默认 Persona

Persona 激活后:
  ├─ build_context_prompt({persona_id, ...}) → 只检索该 Persona 的记忆
  ├─ system_prompt + persona.system_prompt → 合并为 System 消息
  └─ 发送给 LLM
```

## 体验流程

```
1. 首次启动: 自动创建"开发者"+"架构师"两个默认 Persona
2. 用户可手动创建更多 Persona: /persona create "名称" "头衔"
3. 日常: "Alex, 帮我重构这个" → 自动切换到 Alex
4. 切换: /persona Bob → 切换到 Bob
5. 管理: 前端页面可视化管理所有 Persona
```
