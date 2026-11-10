# 意图拆解与会话级编排 — 设计

日期：2026-11-09
状态：待批准
关联：
- `docs/design/2026-11-05-intent-routing-redesign-design.md`（现有「信号→决策→策略」路由架构；本设计把「策略层」换成「拆解 + 编排」）
- `docs/design/2026-11-04-intent-management-design.md`（意图管理模块与意图库）
- `docs/design/2026-05-26-session-autonomous-execution.md`（编排器与执行计划的原始设计）

> 本文件是 `/opsx-propose` 的输入。批准后按项目约定走 OpenSpec，不在本文件之外另起 spec。
>
> **本文取代**一份未批准的「领域前置绑定 + 本地小模型意图分类」草案（本地模型方向已放弃）。本文保留其中**与模型选型无关**的两项决定：领域前置绑定、通用意图代码兜底。

---

## 1. 为什么做

### 现状

意图路由链路（`src-tauri/src/llm/intent/router.rs`）每条消息都跑一次，产出**一个**意图，然后由 `build_execution_policy`（`app/commands/session.rs:1586-1598`）取 `intent.path` 决定走哪条执行路径。

四个结构性问题：

| 问题 | 证据 |
|---|---|
| **每条消息都要打一次云端 LLM** | `collect_llm_signal`（`router.rs:405`）是主力信号；embedding 双阈值（floor 0.55 / margin 0.05）同时满足才跳过它。实测本机 `config.json` 无 OpenAI key → **embedding 路径完全是死的**，每条消息必然走一次 LLM |
| **一次判定只能落一个意图** | `RouteDecision.chosen: IntentResult` 是单值；`build_rerank_prompt`（`classifier.rs:94`）要求模型在候选里选**一个**。用户一句话里包含多件事时，模型被迫挑一个，其余被丢弃 |
| **「计划」这条线是断的** | `LlmPlanner`（`orchestrator/planner.rs`）能拆步骤、`ExecutionRuntime`（`runtime.rs`）能顺序执行并落库，但前端 `PlanConfirmDialog` / `PlanTimeline` / `ExecutionStatusBar` 都要求 `sessionMode==='autonomous' && activePlan`，而后端**从不 emit `plan_generated`**、`execute_plan` **没有前端调用点** → 整条线处于休眠 |
| **领域判定与意图判定混在一起** | `auto` 模式下领域只是排序加成（`prior_boost=0.04`），本质是用上一轮猜下一轮 |

### 目标

1. **让一次输入可以落多个意图**：远端大模型理解用户输入后，拆解出**一个或多个**意图，每个意图带自己的具体指令。
2. **用「拆解 + 编排」替代「单意图 + 路由」**：拆解结果直接组装成执行计划，由程序编排执行。
3. **拆解结果先给用户确认**：模型理解错了，在动手之前就能改。
4. **只拆解一次**：会话首条消息拆解，后续轮次沿用——**每会话 1 次云端调用，而不是每消息 1 次**。
5. **领域前置绑定**：建会话即定领域，意图候选范围恒定。

### 非目标

- 不引入本地模型 / 本地推理运行时。分类与拆解都用远端大模型。
- 不训练任何模型、不自研分类头。
- 不做多意图**并行**执行（沿用现有严格顺序执行）。
- 不做意图状态机、意图转移。
- 不把计划演化成会话级的持续执行骨架（见 §2.4 与 §13）。
- 不改 agent 工具系统本身。

### 反目标

- 为了「智能」让每条消息都多花一次云端 LLM 往返。
- 拆解失败时消息发不出去。
- 在用户没确认的情况下执行写文件、跑命令一类的副作用。

---

## 2. 概念模型

**领域（Domain）**：建会话时绑定的命名空间，硬范围——候选意图只来自该领域。

**意图（Intent）**：意图库中的一条记录 = **一份执行配方**。实际生效的五项：`allowed_tools` / `model_id` / `max_iterations` / `system_prompt_appendix` / `path`。（`workflow` 与 `default_agent` 是纯元数据，全库只在意图管理里被读，不参与执行。）

**拆解（Decomposition）**：远端大模型对一条用户输入的理解结果——**有序的意图列表**，每项 = `{intent_id, instruction, confidence}`。`instruction` 是模型为该意图生成的具体指令。

**计划（Plan）**：拆解结果组装成的 `ExecutionPlan`。一个意图 = 一个执行步骤。

**通用意图**：**不是一个实体**，而是「没拆解出任何有效意图」这个状态，由代码兜底路径实现（普通聊天）。

### 2.1 核心链路

```
① 拆解（发送前的独立步骤）
     候选 = 绑定领域内的全部启用意图（排除 chat / clarify）
     LLM 返回 { intents: [{ intent_id, instruction, confidence }, ...] }
     校验：intent_id 必须在候选集合内 → 非法项丢弃
     低置信项 → 只读探针 → 项目证据附到 instruction 前面
     结果落 execution_plans（pending）+ execution_plan_steps
     ↓
② 用户确认（PlanConfirmDialog 重建）
     可确认执行 / 删掉某个意图 / 改某个意图的指令 / 重新拆解
     ↓
③ 发送并执行
     send_message_stream(session_id, content, plan_id)
       → 用给定 plan 替代原有的 prepare_turn 路由
       → ExecutionRuntime 顺序执行，结果以 assistant 消息流式产出
     ↓
④ 后续消息
     普通对话 + 会话意图上下文（见 §2.4）
     ——不重新拆解、不做意图变更检测
```

**成本特征**：现在是**每消息 1 次云端调用**；本设计是**每会话 1 次**。这是净改善。

### 2.2 为什么「确认」必须在发送之前

拆解是**发送之前**的一个独立步骤，而不是发送流程内部的一次阻塞等待：

```
用户输入
  → decompose_message(session_id, content)   ← 新命令，返回 pending plan
  → 前端弹确认卡
  → 用户确认 → send_message_stream(session_id, content, plan_id)
  → 后端执行 plan，结果以 assistant 消息流式产出
  → 用户取消 → 丢弃 plan，消息根本不发出去
```

这样做的三个好处：
1. **执行结果能走正常的流式输出通道**——`execute_plan` 目前只发进度、不写会话消息，若把确认放在发送内部，就得另写一套「把 plan 结果落成 assistant 消息」的逻辑。
2. **取消 = 消息没发出去**——不留孤儿用户消息、不留半执行的计划。
3. **「确认前无副作用」天然成立**——计划还没执行。

### 2.3 类型边界

拆解产出**独立类型**，不改造 `RouteDecision`：

```rust
/// 一次拆解的结果。
pub struct Decomposition {
    /// 有序的意图项。空 = 零意图 → 普通聊天兜底。
    pub intents: Vec<DecomposedIntent>,
    /// 整体置信度（取各意图的最小值）。
    pub confidence: f32,
    /// 是否存在不确定项。
    pub uncertain: bool,
    /// 决策来源（供轨迹与复盘）。
    pub source: DecisionSource,
    /// 候选意图与分数。
    pub candidates: Vec<IntentCandidate>,
    /// 证据（llm 调用次数、探针是否使用、耗时）。
    pub evidence: DecisionEvidence,
}

/// 拆解出的单个意图。
pub struct DecomposedIntent {
    /// 意图的投影（`project` 的产物）：含 config 与 path。
    pub intent: IntentResult,
    /// 模型为该意图生成的具体指令。
    pub instruction: String,
    /// 该意图的置信度。
    pub confidence: f32,
    /// 低于该意图 `confidence_threshold` 时为 true。
    pub uncertain: bool,
    /// 只读探针取到的项目证据（可选）。
    pub probe_evidence: Option<String>,
}
```

`RouteDecision` 随之退休——它原本承载的「选一个意图 + 兜底」语义被 `Decomposition` + 普通聊天兜底完全覆盖。这样避免了「一个类型既表示单值路由结果、又表示多意图拆解列表」的混合语义。

**波及面**（`RouteDecision` 是公共类型，改动不止一处）：

| 位置 | 改动 |
|---|---|
| `intent_resolved` 事件 | payload 从单意图改成意图列表 |
| `PreparedTurn` / `ExecutionPolicy` | `session.rs:487/576/825/864/891/1592` 一带的单意图字段 |
| `RouteTrace`（`router.rs:700`）+ `get_recent_route_traces` IPC + 前端类型 | 同样改成列表 |
| router 单测与评测 harness | `router.rs:1657` 起的测试按新口径重写 |
| 前端 `IntentResolvedBadge` | 展示意图列表 |

### 2.4 后续消息

**后续消息走普通对话，但会话携带意图上下文**：

- 工具白名单 = 会话级白名单 ∩ **各已选意图白名单的并集**
- 已选意图的名称与说明注入会话 system prompt
- 已执行的计划产出留在消息历史里（执行结果就是 assistant 消息）

**不实现「自动检测用户想换意图」**——那会让每条消息都回到 1 次 LLM 调用，抵消掉本设计的核心收益。

**重新规划只由显式操作触发**：
1. 用户切换领域（`set_session_intent_domain`）
2. 用户在 UI 上点「重新规划」
3. 新建会话

**计划的生命周期是一次性的**：拆解 → 确认 → 执行 → 结束。计划不会演化成会话级的持续执行骨架（那是一个更大的一步，见 §13）。

---

## 3. 领域前置绑定

### 3.1 创建会话

- 新建会话**自动绑定**当前 active 领域（`intent_domains.is_active = 1`），**不弹窗、不打断**。
- 这已经是现状：`create_session` 的 SQL 用 `COALESCE(?15, (SELECT id FROM intent_domains WHERE is_active = 1 LIMIT 1))` 补齐（`core/db/session_repo.rs:17-23`）。
- 改动：IPC 命令 `create_session` 现在写死 `intent_domain_id: None` + `intent_domain_mode: MODE_AUTO`（`app/commands/session.rs:251-252`），改成写入解析后的领域 id + **`MODE_PINNED`**。

### 3.2 硬绑定与会话内切换

- 会话与领域是 **pinned**：`select_candidates` 已经实现硬过滤（`router.rs:304-312`），保留不变。
- 用户可在会话内切换领域：现有 `set_session_intent_domain`（`app/commands/intent.rs:641`）保留，前端已有选择器与「可切换领域后重发」文案（`src-ui/src/components/session/IntentResolvedBadge.tsx:63`）。切换领域会**清空拆解结果并触发重新拆解**。
- **`None` 的语义**：删掉 `auto` 之后，`set_session_intent_domain(None)` 改为「**清空领域绑定**」——会话退化为无领域状态，拆解时无候选 → 普通聊天兜底。`SessionIntentState.mode` 的取值从 `'auto' | 'pinned'` 简化为 **`'pinned' | 'none'`**。前端选择器的「自动」项改为「通用（不绑定）」。既有前端 api 测试（`src-ui/src/api/__tests__/intent.test.ts:82`）需相应更新。

### 3.3 领域不可删除

1. **内置领域不可删除**：`intent_domains.source = 'builtin'` 的行拒绝删除。无需改 schema——`source` 列已有 `DEFAULT 'user'`（`core/db/repository.rs:194-203`），内置域由 seeds 写入时就是 `'builtin'`。
2. **当前 active 领域不可删除**：删 active 域会让后续新会话无处绑定。拒绝删除，或删除后把 `is_active` 复位到内置域（实现取其一，见 §14）。

`delete_intent_domain`（`app/commands/intent.rs:597`）目前**没有任何保护**，需要补上。

### 3.4 删除 `auto` 模式

| 移除项 | 位置 | 理由 |
|---|---|---|
| `intent_domain_mode` 的 `'auto'` 取值 | `core/db/models.rs:81-89`、`core/db/session_repo.rs` | 只剩 `pinned` / `none` |
| `prior_domain` 先验入参 | `router.rs:737`（`RouteContext.prior_domain`） | 领域已硬绑定 |
| `prior_boost` 排序加成 | `app/config.rs:79-81`、`llm/intent/mod.rs:160` | 同上 |
| `select_candidates` 的双分支 | `router.rs:292-320` | 只剩 pinned 一条路径 |
| 命中领域后写回先验 | `app/commands/session.rs:743-756` | 无先验可写 |
| `embedding_floor` / `embedding_margin` | `app/config.rs:72-78`、`llm/intent/mod.rs:156-159` | 候选改为全量，不再有 top-1/top-2 比较 |

**旧数据归一化**（读时 + 写时两处，只改 serde default 不生效）：

- **读时**：`sessions.intent_domain_mode` 显式存着 `'auto'` 的历史行映射为 `'pinned'`；`intent_domain_id` 为空的映射为 `'none'`。
- **写时**：`clear_intent_domain_references_on`（`core/db/session_repo.rs:137-144`）删域时把模式回落成 `'auto'`，改为回落成 `'none'`（`intent_domain_id` 同时置 NULL）。

---

## 4. 拆解

### 4.1 候选集合

**候选 = 绑定领域内的全部启用意图**，每项提供 `id` / `name` / `description` / `examples`（≤5 条）。

**排除 `chat` 与 `clarify` 两个内置意图**：
- `chat`（闲聊）的执行配方本来就等价于普通聊天，让它参与拆解只会让「你好」也弹出一张确认卡
- `clarify`（澄清）是给模型用的占位意图，不是用户会主动要的意图
- 两者的语义由「零意图 → 普通聊天兜底」覆盖

不做 embedding 召回。理由：
- **多意图场景下 top-1 召回没有对应物**：一次输入产出 N 个意图，而 embedding 召回只给「最像的那一个」和它的分数——这个分数无法在多意图语义下映射成置信度。
- **领域内通常只有 10–20 个意图，全量进 prompt 完全可行**（数千 token 量级），召回省下的那点 prompt 长度不值得多一次网络往返。
- 拆解本身就要一次 LLM 调用，再加一次召回不划算。
- 附带好处：本机 `config.json` 无 OpenAI key，embedding 路径本就是死的（`llm/embedding.rs:51` 硬编码 `rig::providers::openai::OpenAI::new(key)`）；去掉它也顺手拆掉了对 OpenAI 的硬依赖。

因此 **`collect_embedding_signal`（`router.rs:327`）、`collect_embedding_fallback`（`:572`）、`collect_rule_signal`（`:606`）三条信号全部退役**。

`EmbeddingService`（`llm/embedding.rs`）**不被本设计改动**——它还被记忆召回用（`features/memory`），且 `config.intent.embedding_model_id` 字段保留原位不动（`app/state.rs:119` 与记忆召回共用）。

### 4.2 拆解的 LLM 调用

复用 `LlmReranker`（`llm/intent/classifier.rs:58`）的位置，但**改造其 prompt 与解析**：

- `build_rerank_prompt`（`:94`）→ 改为 `build_decompose_prompt`：候选清单 + 输出格式说明 + 「若不属于任何意图，返回空数组」的明确指示
- `build_rerank_input`（`:127`）→ 改为 `build_decompose_input`：用户当前消息 + 最近消息（沿用现有 `ROUTE_HISTORY_LIMIT` 与长度截断）
- `parse_rerank_reply` → 改为 `parse_decompose_reply`

输出契约：

```json
{
  "intents": [
    { "intent_id": "builtin.software_dev.plan", "instruction": "为「导出 CSV」功能写一份实现计划", "confidence": 0.93 },
    { "intent_id": "builtin.software_dev.implement", "instruction": "实现 CSV 导出并补齐单元测试", "confidence": 0.81 }
  ]
}
```

**「零意图」的表达方式：`intents` 为空数组。** 契约里明确写出来，模型才有一条确定的表达路径。

**校验（必须做，防幻觉）**：

| 情况 | 处理 |
|---|---|
| `intent_id` 不在候选集合内 | **丢弃该项**（幻觉） |
| `instruction` 为空 | 丢弃该项 |
| `confidence` 缺失或非数值 | 记 0.0 |
| 某项 `confidence < 该意图的 confidence_threshold` | **不丢弃**，标 `uncertain = true` |
| **所有项都 uncertain** | **整体视同零意图** → 普通聊天兜底 |
| 丢弃后列表为空 | 普通聊天兜底 |

「部分低置信 + 部分高置信」时，低置信项保留（带探针证据）让用户判断；「全部低置信」时兜底——这条规则让验收标准里的「零意图场景 100% 落入兜底」有可达路径。

**模型选择**：沿用 `config.intent.classifier_model_id`（`None` = 默认模型）。**不要求特定模型**——拆解任务对大模型的能力要求远高于分类，但仍低于 agent 主任务，用默认模型即可。

**超时**：沿用 `config.intent.llm_timeout_ms`（默认 8000）与 `routing_budget_ms`（默认 12000）。

### 4.3 不确定项与只读探针

**只读探针**（`collect_probe_signal`，`router.rs:497`）**保留但语义收窄**：对不确定项运行只读探针（限 `project_root`），把读到的项目证据**附加到该意图的 instruction 前面**，让用户在确认时看到「这条意图有项目证据支撑」。

**不重排、不改置信度**——原来的探针语义是「拿证据再选一次意图」，在多意图场景下不成立（多个意图可能都有证据）。探针开关沿用 `config.intent.probe_enabled`。

### 4.4 拆解的时机、缓存与失效

- **触发**：用户在发送前触发 `decompose_message`（§2.2）。
- **缓存**：拆解结果落在 `execution_plans`（`PlanSource::Dynamic`，`status = pending`）+ `execution_plan_steps`（每步一个意图），`sessions.active_plan_id` 指向它。
- **失效机制**：
  - **意图库发生任何增删改** → **清空所有会话的 `active_plan_id`**（一条 UPDATE 即可）。意图改了 examples / 工具白名单 / 阈值后，缓存的拆解结果引用的是旧配方，继续用会有风险；意图库变更本就低频，全量失效代价可接受。索引重建仍走现有 `refresh_intent_index`（`app/commands/intent.rs:366`）。
  - **用户切换领域** → 清空该会话的 `active_plan_id`。
  - **用户点「重新规划」** → 丢弃当前 plan，重新拆解。
  - 清空 `active_plan_id` **不影响 `execution_plans` 的历史记录**，只解除会话指向。

---

## 5. 组装与执行

### 5.1 意图 → 步骤

每个意图 → 一个 `ExecStep::AgentTask`（`plan_types.rs:61`）：

| `ExecStep::AgentTask` 字段 | 来源 |
|---|---|
| `instruction` | LLM 拆解产出的 `instruction`（探针证据已附加） |
| `model_id` | `intent.model_id`（`project` 已有映射，`router.rs:781`） |
| `allowed_tools` | `resolve_tools(会话工具集, intent.allowed_tools)`（`router.rs:187`）——**取交集，不放宽** |
| `max_iterations` | `intent.max_iterations` |
| `temperature` | `None` |
| `system_prompt_appendix` | `intent.system_prompt_appendix`——**新增字段**（见下） |

**`system_prompt_appendix` 的下发**：`ExecutionRuntime` 只有一个运行级 `system_prompt`，所有步骤共用，per-intent 的 appendix 无处安放。**决定：给 `ExecStep::AgentTask` 增加 `system_prompt_appendix: Option<String>`，runtime 执行该步骤时把它合并进该步骤 agent 的 system prompt。**（`ExecStep` 是 `#[serde(tag = "type")]` 的标签枚举，新增带 `#[serde(default)]` 的可选字段向后兼容。）

**deep 意图**（`intent.path == "deep"`）：先经 `LlmPlanner.generate_plan_with_policy`（`planner.rs:54`）把该意图的 instruction 展开成子步骤序列，再展平进总计划。展开失败时退化为单个 `AgentTask`。

### 5.2 步骤的元数据

确认卡片要展示意图名、低置信标记、探针证据、副作用标记，但 `PlanStep` 只有 `label` + `execution`，`ExecStep::AgentTask` 也不带 `intent_id` / `confidence`。

**决定：扩展 `PlanStep`，不塞进 `label`**（`label` 是展示文本，不是结构化字段）：

| 新字段 | 类型 | 用途 |
|---|---|---|
| `intent_id` | `Option<String>` | 卡片显示意图名、追溯到意图库 |
| `confidence` | `Option<f32>` | 显示置信度、标记不确定项 |
| `uncertain` | `bool`（default false） | 卡片上的「置信度低」角标 |

`plan_json` 是整个 `ExecutionPlan` 的 serde 序列化（`runtime.rs:414-434`），新增带 default 的字段对历史记录向后兼容。

**副作用标记不单独存**——从该步骤的 `allowed_tools` 推出（含 `code_executor` / `shell` / `file_system` 即为有副作用）。

### 5.3 编排与执行

- 组装好的 `ExecutionPlan` 交给 `ExecutionRuntime`（`runtime.rs`），**严格顺序执行**（`runtime.rs:166`）。
- 执行沿用现有语义：步骤边界可暂停/取消；任一步失败**立即终止计划**并返回**部分结果**（`runtime.rs:278-343`）；步骤状态 `pending`/`running`/`completed`/`failed`/`skipped`。
- 落库：`execution_plans` + `execution_plan_steps`（`core/db/plan_repo.rs`）。
- 进度：`PlanProgressEvent`（`plan_types.rs:351`）推前端。

**会话状态**：需要一个新的运行模式（如 `PlanRunMode::Planned`）：持久化计划 + 设置 `active_plan_id` + 写 `execution_status`，但**不改** `sessions.mode`（会话仍是 `chat`）。现有 `update_session_execution`（`session_repo.rs:338`）是 mode + status + active_plan_id 三者一起写，需要能只写其中两项。见 §14。

---

## 6. 用户确认

**`PlanConfirmDialog` 需要重建，不是改个条件就行。** 现状是只读的「已自动开始执行」弹窗、只有一个取消按钮——它是为 autonomous 模式写的通知框，不是编辑器。

新建的确认卡片展示：

- 识别到的意图列表（意图名 + instruction + 工具白名单 + 模型）
- 不确定项明确标注「置信度低」
- 有探针证据的意图展示证据摘要
- 有副作用的意图明确标注「将写入文件 / 执行命令」

用户可以：

| 操作 | 行为 |
|---|---|
| 确认执行 | 按当前计划执行 |
| 删掉某个意图 | 从计划里移除该步，重新计算 `step_index` |
| 改某个意图的指令 | 改写该 step 的 `instruction`，其余不变 |
| 重新规划 | 丢弃当前 plan，重新调 `decompose_message` |

**需要的 IPC**（新增）：
- `decompose_message(session_id, content)` → 返回 `{ plan_id, intents: [...] }`，plan 落库为 `pending`
- `update_plan_step(plan_id, step_id, instruction)` → 改指令
- `remove_plan_step(plan_id, step_id)` → 删步骤
- `cancel_plan(plan_id)` → 丢弃 pending plan

**确认前不产生任何副作用**——计划还没执行，这是本设计的一条硬约束。

---

## 7. 前端

| 位置 | 改动 |
|---|---|
| 新建会话 | **无改动**（后端已无感绑定默认领域） |
| 会话头部领域选择器 | 已有，改为只列可选领域，「自动」项改为「通用（不绑定）」；切换领域触发重新拆解 |
| 聊天输入框 | 发送前先调 `decompose_message`，有结果则弹确认卡；用户确认后再 `send_message_stream` |
| `PlanConfirmDialog` | **重建**：意图列表编辑器（删步骤 / 改指令 / 重新规划 / 确认执行） |
| `PlanTimeline` | **复活**：展示 N 个步骤的推进状态 |
| `ExecutionStatusBar` | **复活**：展示计划执行状态 |
| `IntentResolvedBadge` | 改为展示意图**列表**（数量 + 名称），不再单值 |
| 意图管理页 | 内置领域显示「内置」标记，删除按钮置灰 |

**现有死代码会被接活**：`plan_generated` 事件（后端从不 emit）、`execute_plan` IPC（无前端调用点）、`sessionSlice.setActivePlan`（无来源）、三个计划展示组件。

---

## 8. 配置

`IntentAppConfig`（`app/config.rs:60-99`）改动：

| 字段 | 变化 | 说明 |
|---|---|---|
| `enabled` | 保留 | 总开关 |
| `classifier_model_id` | 保留 | 拆解用的模型（`None` = 默认模型） |
| `embedding_model_id` | **保留但路由不再读** | 与记忆召回共用（`app/state.rs:119`），本设计不动它 |
| `embedding_floor` / `embedding_margin` | **移除** | 候选改为全量，不再有 top-1/top-2 比较 |
| `prior_boost` | **移除** | `auto` 模式消失 |
| `llm_rerank_enabled` | **保留原名**，语义变为「是否启用意图拆解」 | 减少改动面 |
| `llm_timeout_ms` / `probe_enabled` / `probe_timeout_ms` / `routing_budget_ms` | 保留 | 拆解与探针的超时预算 |
| `second_rerank_min_budget_ms` | 移除 | 探针不再做二次重排 |

**旧配置兼容**：被移除字段读取时忽略即可（serde `#[serde(default)]`），无需迁移脚本。彻底删除还是保留但忽略，见 §14。

### 8.1 推荐配置

拆解模型由 `config.intent.classifier_model_id` 指定，**推荐 `deepseek-v4-flash`**：

| 配置项 | 推荐值 | 理由 |
|---|---|---|
| `intent.classifier_model_id` | `deepseek-v4-flash` | 57 条评测集上与 `deepseek-v4-pro` 的意图集合准确率相同（排除 ambiguous 后均 48/49），但延迟低约 3 倍：p50 1292ms vs 3940ms、p95 2852ms vs 17818ms、max 3054ms vs 20002ms（pro 撞超时）。详见 [评测报告](./intent-decomposition-baseline.md) |
| `intent.llm_timeout_ms` | `20000` | 默认值由 `8000` 上调。旧默认按仓库既有 `migrate_legacy_timeouts` 模式迁移，用户自定义值不动 |
| `intent.routing_budget_ms` | `25000` | 默认值由 `12000` 上调，必须大于 `llm_timeout_ms`，给探针留预算。同上迁移规则 |

本机 `config.json` 已按此配置生效（原文件备份为 `config.json.bak-20261109`）。

---

## 9. 评测与验收

### 9.1 评测集

现有 `src-tauri/tests/fixtures/intent_routing_eval.json`（44 条，`clear` / `ambiguous`）针对的是**单意图分类**，与新的**多意图拆解**任务不匹配。需要的评测集形态：

- 每条标注：一个用户输入 + **期望的意图集合**（有序）+ 每个意图的 instruction 要点
- 需要覆盖：**一个意图 / 多意图 / 零意图（普通聊天）/ 跨领域歧义**

**现有 44 条仍可作为「单意图场景」的子集**，但多意图与零意图场景需要新标注。

评测口径：

| 指标 | 定义 |
|---|---|
| **意图集合准确率** | 拆解出的意图集合与期望集合完全一致的比例 |
| **单意图召回** | 期望的每个意图是否都被拆出来（漏一个即失败） |
| **幻觉率** | 拆出不属于候选集合的意图的比例（应为 0） |
| **零意图准确率** | 应判为零意图的场景正确落入普通聊天兜底的比例 |

### 9.2 可测试性

- 拆解器依赖一个可注入的 LLM provider 边界——现有 `ProviderBox`（`llm/provider.rs:181`）已是这个形态，测试里已有 fake 实现先例。
- 单测与 CI **不需要真实模型调用**：注入固定 reply 的 fake provider。
- 真实模型的评测走手动通道：`INTENT_ROUTING_EVAL=1 cargo test --lib intent_routing_eval -- --ignored --nocapture`（`router.rs:1657`），沿用现有 harness 的形状但换成新口径。

### 9.3 验收标准

1. **意图集合准确率 ≥ 90%**（在新建的多意图评测集上）
2. **幻觉率 = 0**：拆出的 `intent_id` 100% 来自候选集合
3. **零意图场景 100% 落入普通聊天兜底**，不误执行领域意图
4. **用户确认前不产生任何副作用**：写文件 / 跑命令类步骤在确认前不执行
5. **每会话只拆解一次**：第二个消息不再发起拆解调用
6. **拆解失败不影响消息发送**：LLM 超时/报错时走普通聊天
7. **`chat` 不触发确认卡**：说「你好」直接走普通聊天，不弹计划卡片

---

## 10. 迁移与兼容

| 变更 | 兼容策略 |
|---|---|
| `sessions.intent_domain_mode` 旧值 `'auto'` | 读时映射为 `'pinned'`；`intent_domain_id` 为空 → `'none'`（§3.4） |
| 删域时的模式回落 | `clear_intent_domain_references_on` 改写 `'none'`（§3.4） |
| `SessionIntentState.mode` 取值变化 | `'auto' \| 'pinned'` → `'pinned' \| 'none'`；前端 api 测试（`src-ui/src/api/__tests__/intent.test.ts:82`）同步更新 |
| 旧 `config.json` 含已移除字段 | serde 忽略未知字段，无需迁移脚本 |
| `RouteDecision` → `Decomposition` | 公共类型改动，波及 `intent_resolved` 事件、`PreparedTurn`/`ExecutionPolicy`、`RouteTrace`、前端徽标与类型、router 单测与评测 harness（§2.3） |
| `PlanStep` 新增字段 | 带 default，历史 `plan_json` 仍可反序列化 |
| `ExecStep::AgentTask` 新增字段 | 带 default，历史记录仍可反序列化 |
| 内置领域不可删除 | 只影响新删除操作，不影响已有数据 |
| `intent.enabled = false` 的意图 | 不进候选集合 |

---

## 11. 方案取舍记录

| 方案 | 做法 | 取舍 |
|---|---|---|
| **A. 改造现有 router 为拆解器（选定）** | 保留候选召回 / 探针 / 轨迹 / 徽标骨架，把 LLM 单选改成多选，产出 `Decomposition` → 组装计划 → 现有 orchestrator 执行 | 复用度最高；`RouteDecision` 是公共类型，改动波及前端；现有单选逻辑退役 |
| B. 另起一套拆解器，router 并存 | 新增独立模块，router 原样保留做「每轮路由」 | 概念清晰但**两套 LLM 调用、首轮成本翻倍，且两套决策可能打架**（router 选 A、拆解选出 A+B） |
| C. router 整体退休，只留拆解 | 删掉候选召回 / 探针 / 规则打分 / 单选逻辑 | 最干净；但要丢掉已经能跑的东西，且现有 deep 路径（`session.rs:1404-1443`）要重写 |
| D. 进程内嵌本地模型做分类 | 见已删除的本地模型草案 | 已放弃：需要模型分发、原生依赖、标注数据，换来的只是省一次云调用 |

**选定 A**。它与既有架构同构：现有 router 本来就在做「用 LLM 理解用户输入并选意图」，新需求只是把输出从 1 个变成 N 个，并接上已经存在但休眠的「计划 → 执行 → 确认」链路。B 的双调用与决策冲突不可接受；C 要重写能跑的东西；D 已放弃。

**为什么最终选了「远端模型 + 确认后执行」而不是本地分类**：本地分类只能「选一个意图」，做不到「拆解出多个意图并为每个生成指令」；而且需要模型分发、跨平台原生依赖、标注数据三样额外投入，而远端大模型一次调用就把三件事全办了。

**为什么确认放在发送之前而不是发送流程内部**：见 §2.2——执行结果能走正常流式通道、取消不留孤儿消息、「确认前无副作用」天然成立。

---

## 12. 本次拍板的决定

1. **放弃本地模型**，拆解与分类都用远端大模型。
2. **意图从现有意图库选**，LLM 只输出「选哪几个 + 每个的具体指令」；执行配方由 `intents` 表给出，工具白名单不被模型自由发挥。
3. **每个会话的首条消息触发拆解**，后续轮次**沿用**已拆解的意图集合——具体形态是：后续消息走**普通对话**，会话携带意图上下文（工具白名单取各已选意图白名单的并集、意图说明注入 system prompt、计划产出留在消息历史里）。**不做「自动检测用户想换意图」**（那会让每条消息又回到 1 次云端调用）；重新规划只由显式操作触发：切换领域 / 点「重新规划」/ 新建会话。
4. **拆解结果先给用户确认**（`PlanConfirmDialog` 重建），用户可删步骤 / 改指令 / 重新规划；确认前不执行任何副作用。
5. **确认是发送之前的独立步骤**（`decompose_message` → 确认 → `send_message_stream(plan_id)`），不是发送流程内部的阻塞等待。
6. **领域前置绑定**（建会话即绑定、默认无感、可切换、内置域不可删、`auto` 模式删除）——沿用，与模型选型无关。
7. **通用意图不落库**——它是「没拆解出有效意图」这个状态，由代码兜底路径实现（普通聊天）。
8. **候选 = 领域内全量启用意图，排除 `chat` / `clarify`**（`chat` 的执行配方本就等价于普通聊天，留着会让「你好」也弹确认卡；两者语义由零意图兜底覆盖）；不做 embedding 召回；embedding / 规则两条信号退役。
9. **编排沿用现有 `ExecutionRuntime` 顺序执行**，不引入并行。
10. **探针保留但语义收窄**：只为不确定项附加只读证据，不重排、不改置信度。
11. **拆解产出独立类型 `Decomposition`**，`RouteDecision` 退休。
12. **`PlanStep` 扩三个字段**（`intent_id` / `confidence` / `uncertain`）；`ExecStep::AgentTask` 扩 `system_prompt_appendix`。
13. **意图库任何增删改 → 清空所有会话的 `active_plan_id`**；切换领域 → 清空该会话的。
14. **`set_session_intent_domain(None)` = 清空领域绑定**（会话退化为普通聊天），`mode` 取值简化为 `'pinned' | 'none'`。

---

## 13. 风险

| 风险 | 处理 |
|---|---|
| 大模型拆解出多个意图，顺序与依赖出错 | 第一版只做顺序执行，不做依赖图；用户可在确认卡片里删步骤调整顺序 |
| 拆解结果偏离用户真实意图 | 确认环节是最后一道闸；`intent_resolved` 事件照旧携带候选与分数，便于事后复盘 |
| 多意图执行链路过长，中途失败只拿到部分结果 | 沿用现有语义（`runtime.rs:278-343` 返回部分结果），前端展示已完成步骤 |
| 每会话 1 次云端调用的成本 | 相比现状（每消息 1 次）是净改善；模型可用 `classifier_model_id` 指向更便宜的小模型进一步压成本 |
| `RouteDecision` 改动波及前端与公共 API | 事件 payload、`PreparedTurn`/`ExecutionPolicy`、`RouteTrace`、徽标与前端类型同批改；字段形状变更写进 OpenSpec 的 spec delta |
| **后续消息只做普通对话，可能接不住「继续上一个任务」的诉求** | 已知取舍：计划是一次性的，不演化成会话级持续骨架（§2.4）。若后续需要，重新规划是显式操作，成本是再拆解一次 |
| 意图库变更后旧拆解结果失效 | 通过清空 `active_plan_id` 全量失效（§4.4） |
| 用户切换领域后原计划仍在执行 | 切换领域时若有未完成的计划，先提示并取消 |

---

## 14. 留给实现者的事项

1. **`PlanRunMode` 的第三种取值**（持久化计划 + 设 `active_plan_id` + 写 `execution_status`，但不改 `sessions.mode`）的具体形态——现有 `update_session_execution`（`session_repo.rs:338`）三者一起写，需要能只写其中两项。
2. **多意图评测集的标注**：需要新造一批覆盖「一个意图 / 多意图 / 零意图 / 跨领域歧义」的样本，格式沿用 `tests/fixtures/intent_routing_eval.json`。
3. **`PlanConfirmDialog` 的重建规模**：现状是只读通知弹窗，需要重新设计成意图列表编辑器——组件的 props、状态管理与数据来源都要重写。
4. **已移除配置字段的处置**：保留但忽略 vs 彻底删除（涉及 `AppConfig` 的 serde 定义与 `config.json` 的向后兼容）。
5. **active 领域被删除时的行为**：拒绝删除 vs 自动复位到内置域（§3.3 规则 2）。
6. **`ExecStep::AgentTask` 的 `system_prompt_appendix` 如何在 runtime 里合并进该步骤 agent 的 system prompt**——runtime 现有的是运行级单一 `system_prompt`，需要下沉到步骤级。

---

## 15. 实现遗留

实现与验收已完成，但以下两项未收口（完整清单与评测口径见 [意图拆解评测报告](./intent-decomposition-baseline.md) §6）：

1. **`decompose_message` 命中已有缓存时仍会新落一条等价计划**：正常前端流程不会触发（前端在 `active_plan_id` 存在时跳过调用），只会在前后端判断不一致时产生孤儿计划行。未修。
2. **`check-comment-language.mjs` 报 6 处英文注释**：全部位于 `src-tauri/src/app/mod.rs`，本次改动未触及该文件，属既有问题。
