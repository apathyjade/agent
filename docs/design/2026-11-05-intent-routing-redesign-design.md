# 意图路由链路重构 — 设计

日期：2026-11-05
状态：已批准（2026-11-05）；OpenSpec change：`openspec/changes/intent-routing-redesign/`
关联：
- `docs/design/2026-11-04-intent-management-design.md`（意图管理模块）
- `docs/design/2026-11-05-intent-domain-runtime-integration-design.md`（现有级联路由，本设计取代其 §6）

> 本文件是 `/opsx-propose` 的输入。批准后按项目约定走 OpenSpec，不在本文件之外另起 spec。

---

## 1. 为什么做

现有链路已经能用，但结构上是「一次判定 → 直接决定执行配方」：

| 问题 | 证据 |
|---|---|
| 一次判定不可逆 | `IntentRouter::route` 的结果同时决定 path / 工具 / 模型 / 提示词，判错整轮就错 |
| 级联控制流过重 | prior → 扩域 → fallback 链；混用 `fallback_intent`、`.clarify` 后缀、最小 `sort_order` 三种隐式约定 |
| 分类器近乎无上下文 | 只喂当前一句 + 全量意图清单；没有对话历史、没有负例、没有「以上都不是」 |
| 弱信号只能兜底 | examples 子串（命中即 1.0）与中文 bigram Jaccard，短例子容易误命中 |
| 阶段是假的 | `send_message_stream` 先发 `classifying`，真实路由在 `prepare_turn` 里同步跑完 |
| 无法复盘 | 不记录候选分数、信号来源、耗时、prompt/reply；错了只能从 `execution_log` 猜 |

### 目标

1. 把「理解」与「执行策略」拆开：路由器只产出**可解释的决策**，策略层再把决策翻译成执行配方。
2. 让常见消息走**便宜且确定**的路径（embedding 召回），只在边界模糊时花一次 LLM，再模糊才探针。
3. 把「不确定」变成一等公民：不静默兜底，按用户确认的行为**继续执行并明确告知**。
4. 消掉三个隐式约定，降低分支组合与维护成本。
5. 留下可回放的决策证据，错误可定位。

### 非目标

- 运行中 fast → deep 升级（`path` 仍在策略层静态决定）。
- 多意图并行、意图转移/状态机、领域绑定业务流水线（`workflow` / `default_agent`）。
- 从用户纠正中自动学习 examples 或阈值。
- 消息级意图历史落库（回看旧消息显示当时意图）。
- 把意图路由做成带完整工具集的自主 agent。

### 反目标

- 为了「智能」让每条消息都多花一次 LLM/工具往返。
- 分类失败时消息发不出去。
- 用不确定状态静默放宽工具白名单。

---

## 2. 概念模型

**信号（Signal）**：对「这句话像哪个意图」的一次独立判断。每个信号只产出候选与分数，不互相嵌套。
**决策（Decision）**：把多个信号合并成一个结果，包含选中项、置信、是否不确定、候选列表与证据。
**策略（Policy）**：把选中的意图翻译成执行配方（path / 工具 / 模型 / 提示词 / 迭代上限）。

两条语义收窄：

1. **领域是命名空间，不是路由阶段。** `pinned` 是硬范围（只在该领域的意图里选）；`auto` 下上次领域只作**排序加成**，不再有"域内 → 扩域"的级联。
2. **不确定不是失败。** 低于阈值时仍选最高分意图继续执行，但决策带 `uncertain = true`，前端明确展示；只有「完全没有候选」才落到闲聊。

---

## 3. 总体架构

```
prepare_turn
  │
  ├─ 构造 RouteContext { session_id, content, recent_messages, prior_domain, pinned, project_root }
  │
  ├─ 信号层
  │    ├─ EmbeddingRecall   （默认信号；top-k + 分数）
  │    ├─ LlmRerank         （仅当 embedding 边界不清晰，或 embedding 不可用）
  │    ├─ RuleScorer        （LLM 不可用时的最后信号）
  │    └─ (pinned 作为硬过滤，不作为信号)
  │
  ├─ 决策层 DecisionEngine
  │    ├─ 合并候选 / prior 仅作 tie-break
  │    ├─ margin + floor 判定
  │    ├─ 不确定 → 可选只读探针（Probe）→ 再 rerank
  │    └─ 仍不确定 → 选最高分 + uncertain=true（用户确认的行为）
  │
  ├─ RouteDecision { chosen, confidence, uncertain, source, candidates, evidence }
  │
  └─ 策略层 ExecutionPolicy
       └─ { path, allowed_tools, model_id, system_prompt_appendix, max_iterations }
```

调用方不变：`prepare_turn` 仍是流式/非流式唯一入口；路由完成后发 `intent_resolved`，再进入 fast/deep 执行。

---

## 4. 信号层

### 4.1 显式范围与先验

| 输入 | 行为 |
|---|---|
| `intent.enabled == false` | 直接返回合成闲聊，不跑任何信号（保留现有语义） |
| `pinned` 且领域存在 | 候选集合硬过滤为该领域的启用意图 |
| `pinned` 但领域已删除 | 当作 `auto`，warn 一次（保留现有语义） |
| `auto` + 上次领域 | 该领域内意图加 `prior_boost`（默认 0.04），**只用于候选排序，不参与 floor / margin / 阈值判定**，不改变候选集 |
| 无任何启用意图 | 合成闲聊，`uncertain = false`，不调 LLM |

### 4.2 EmbeddingRecall（默认信号）

- 索引单元：每个意图建 1 个 `description + name` 向量，外加每个 example 各 1 个向量；意图分数取所有单元的最大 cosine。
  - 理由：短 example 与长描述语义密度不同，取最大比拼接更稳；也便于后续定位是哪个例子命中。
- 查询向量：当前用户消息（不拼接历史；历史交给 LLM rerank）。
- 索引构建：进程内 `IntentVectorIndex`，**后台预热，绝不阻塞首条消息**。
  - `AppState::new` 构造完成后 `tokio::spawn` 一次 warmup：读取全部启用意图，批量 embed，写入索引，置 `ready = true`。
  - 意图 CRUD 时置 dirty 并在后台重建；重建期间旧索引继续可用（按旧快照），完成后原子替换。
  - 索引未 ready 时，`available()` 返回 false，embedding 信号跳过，本条消息走 LLM（与改造前延迟相同）；首条消息不会被索引构建卡住。
  - **本版不落库**，避免迁移与同步复杂度；启动成本 = 启用意图数 ×（1 + examples 数）。内置领域约 60 个单元，可接受。若后续意图数量导致启动成本明显，再单独立项持久化。
- Embedding provider：复用记忆模块同一套 Rig OpenAI embedder（`text-embedding-3-small`），key 来源沿用 `AppState::new` 里现有的 OpenAI key 查找逻辑。
  - 抽成共享的 `EmbeddingService`，由 `MemoryManager` 与 `IntentRouter` 共用，避免两处初始化逻辑漂移。
  - 无 key / 初始化失败：该信号不可用，记录一次 warn，不阻塞路由。

### 4.3 LlmRerank

触发条件（满足任一）：

1. top1 分数 `< embedding_floor`；
2. `top1 - top2 < embedding_margin`；
3. embedding 不可用；
4. `pinned` 领域内候选数 > 1 且 embedding 边界不清晰。

输入：

- 当前消息；
- **最近 6 条消息**（role + 内容截断，总字符上限 2000；只用于消解"刚才说的""就按上面那样"这类指代）；
- 候选意图（embedding top-k，默认 k=6；embedding 不可用时按廉价词法召回取 top-k；仍然取不满则取全部启用意图，上限 40）；
- 明确指令：若都不匹配，返回 `intent: "none"`。

输出：`{ "intent": "<id|none>", "confidence": 0.0-1.0, "reason": "..." }`，沿用现有的容错 JSON 解析（允许 fence / 前后 prose），但**只接受候选集内的 id**。三种结果必须分开：`none` = 明确的「无匹配」（`NoMatch`，可信结论，不再用规则覆盖）；未知 id、非有限分数、调用失败、超时、回复不可解析 = `Unavailable`（允许后续信号兜底）。

超时：`llm_timeout_ms`（默认 8000）。必须覆盖分类模型的真实往返（实测峰值 3.7s）；2.5s 会让每次工程消息都超时，信号丢失后退化到规则打分。超时或失败不再重试，直接使用当前已有候选。

### 4.4 RuleScorer（最后信号）

保留现有规则打分，但修两个明显问题：

- 子串命中要求 example 长度 ≥ 2 且归一化（trim / 小写）；命中分从 `1.0` 降为 `0.9`，避免与「确定命中」混淆。
- 最长 example 优先的规则保留；bigram Jaccard 保留。

它只在 embedding 不可用且 LLM 不可用时作为唯一信号。分数过意图阈值 → `uncertain = false`；否则 → `uncertain = true`。

### 4.5 信号预算

- `routing_budget_ms`（默认 12000）是**整个路由的硬性墙钟预算**，包含 embedding 查询、LLM rerank、探针和探针后的二次 rerank。
- 超时用 `tokio::time::timeout` 停止等待（future 被 drop，等于取消），不重试；用当时已有的最佳候选出决策。
- 进入探针前检查剩余预算：`remaining < probe_timeout_ms + second_rerank_min_budget_ms` 时跳过探针，直接以 `uncertain = true` 返回。默认 `probe_timeout_ms = 1500`、`second_rerank_min_budget_ms = 1200`。
- 索引后台预热不占用该预算（见 §4.2）。
- 单次 LLM rerank 的 timeout 取 `min(llm_timeout_ms, remaining)`，默认 `llm_timeout_ms = 8000`。
- 所有信号失败都返回 `None`/空候选，不允许 panic 或向上抛错。

默认值下的最坏路径：embedding ~0.3s + 第一次 LLM 8s + 探针 1.5s + 第二次 LLM 1.2s ≈ 11s，落在 12s 预算内；成功路径按模型真实耗时返回（实测 1.3–3.7s），不会固定等待 8s。

旧配置里的 2500 / 6000 是上一版默认值，`AppConfig::load()` 会一次性迁移到 8000 / 12000；用户显式设置的其他值不变。

---

## 5. 决策层

### 5.1 合并规则

**「最可能」的定义**：按信号可信度排序取第一个给出有效 id 的信号——探针后的 rerank > LLM rerank > embedding top1 > RuleScorer。不同来源的分数不直接比较，因此不做跨来源的加权平均。

```
candidates = 全部启用意图；pinned 时硬过滤到该领域
if candidates.is_empty(): return 合成闲聊(uncertain = false)

# 1. embedding（阈值判定用原始分数；prior_boost 只用于候选排序）
emb = index.ready ? recall(content, candidates) : None
if emb 且 emb.top1.raw >= floor 且 emb.top1.raw - emb.top2.raw >= margin:
    return emb.top1, source = Embedding, uncertain = false
    # 若原始分数并列（±1e-6），prior 领域优先

# 2. LLM rerank
if llm_rerank_enabled:
    llm = rerank(content, recent_messages, candidates)
else:
    llm = None

if llm 返回有效 id:
    chosen = llm.id
    if llm.confidence >= chosen.confidence_threshold:
        return chosen, source = Llm, uncertain = false

    # 低置信：先探针（若预算与配置允许），否则直接承认不确定
    if probe_enabled 且 project_root 存在 且 剩余预算足够:
        evidence = probe(content, chosen, project_root)
        if evidence:
            llm2 = rerank(content + evidence, recent_messages, candidates)
            if llm2 返回有效 id:
                return llm2.id, source = Probe, uncertain = llm2.confidence < threshold(llm2.id)
    return chosen, source = Llm, uncertain = true

# 3. LLM 不可用 / 关闭 / 失败：embedding 仍是比规则更强的信号
if emb:
    return 按 rank_score（raw + prior_boost）排序的 top1, source = Embedding, uncertain = true

# 4. 无 embedding：RuleScorer
rule = score_intents(content, candidates)
if rule:
    return rule, source = Rule, uncertain = rule.score < threshold(rule.id)

return 合成闲聊(uncertain = true)
```

补充规则：

- **`llm_rerank_enabled = false`**：跳过第 2/3 步；有 embedding 用 embedding top1（`uncertain = true` 当 margin/floor 不满足），否则用 RuleScorer。没有 LLM 就无法做探针后的 rerank，因此探针也跳过。
- **`probe_enabled = false`**：LLM 低置信时直接 `source = Llm, uncertain = true`。
- **`source` 的语义**：`Probe` 仅表示「探针后的二次 rerank 给出了有效 id」；探针跑了但 rerank 失败时仍是 `Llm`。
- **RuleScorer 不覆盖 embedding**：它只在 embedding 不可用时运行。避免一个 example 子串的 0.9 抢走 embedding 已给出的合理候选。
- **`confidence_threshold` 只作用于 LLM 置信度**；embedding 用全局 `floor` / `margin`，因为两者的分数不可直接比较。
- **`prior_boost` 只影响候选排序**：不参与 `floor` / `margin` / 阈值判定，避免先验把 0.52 抬到 0.56 从而"变成确定"；最终 `confidence` 报告原始分数。

### 5.2 不确定时的行为（用户已确认）

- **继续执行**：使用当前最高分意图的策略，不中断本轮。
- **明确告知**：`intent_resolved` 事件带 `uncertain = true` 与候选列表；前端展示「可能：<意图名>」徽标 + 一行提示（见 §8）。
- 不确定**不放宽**工具白名单。理由：静默扩权是安全回退；用户若不同意，下一轮纠正或 pin 领域。

### 5.3 只读探针（C 分支）

只在「LLM 也低于阈值」且 `probe_enabled` 时触发。

- 输入：当前消息、候选意图、`project_root`（来自 session 绑定的 project；没有则跳过）。
- 能力边界：**不复用 `ToolRegistry`**，只做两类只读探针：
  1. 项目根目录一层列表（含文件名）；
  2. 用候选意图的名称/示例关键词做一次文本搜索（限项目根、限文件数、限结果长度）。
- 不读取 `/`、用户主目录、密钥文件；路径必须 canonicalize 后仍在 `project_root` 内。
- 最多 1 轮，总超时 `probe_timeout_ms`（默认 1500ms）。
- 产出：一段 ≤ 1200 字符的证据文本，作为 LLM rerank 的额外上下文。
- 探针失败 = 无证据，不影响后续决策。

### 5.4 结果类型

```rust
pub struct RouteDecision {
    pub chosen: IntentResult,
    pub confidence: f32,
    pub uncertain: bool,
    pub source: DecisionSource,          // Embedding | Llm | Rule | Probe | Fallback
    pub candidates: Vec<IntentCandidate>, // id, name, score, domain_id
    pub evidence: DecisionEvidence,       // 各信号分数、margin、耗时、llm_calls、probe_used
}

pub struct IntentCandidate {
    pub intent_id: String,
    pub name: String,
    pub domain_id: String,
    pub score: f32,
    pub signal: DecisionSource,
}
```

`IntentResult` 保留现有字段（id/name/domain_id/domain_name/config/path/confidence），`confidence` 填最终得分。

---

## 6. 策略层

`ExecutionPolicy` 由 `chosen` 投影而来，规则与现状一致：

| 字段 | 来源 |
|---|---|
| `path` | `intent.path`（fast / deep） |
| `allowed_tools` | `resolve_tools(session.enabled_tools, intent.allowed_tools)`，空数组 = 无工具 |
| `model_id` | `intent.model_id` > persona > session > registry default，逐级跳过未注册模型 |
| `system_prompt_appendix` | `intent.system_prompt_appendix` |
| `max_iterations` | `intent.max_iterations` |

`uncertain` 不参与策略计算。

---

## 7. 数据与配置

### 7.1 数据库：不新增表，不新增列

- `intents.fallback_intent`：**停止读取**，字段保留并在模型层标注 `#[deprecated]`，与 `auto_escalate` 同样处理。前端从编辑表单移除。
- `intents.auto_escalate`：维持已废弃状态。
- `intents.sort_order`：仅用于展示排序，不再参与兜底。
- `.clarify` 后缀：不再有特殊语义；`clarify` 意图就是一个普通候选，只有分数够高才会被选中。

这样避免迁移，也避免"旧数据回滚"问题；代价是旧的自定义 fallback 链行为消失，需要写进发布说明。

### 7.2 `IntentAppConfig` 新字段

全部 `#[serde(default)]`，旧配置可加载：

```rust
pub struct IntentAppConfig {
    pub enabled: bool,                       // 现有
    pub classifier_model_id: Option<String>, // 现有
    pub embedding_model_id: Option<String>,  // 默认 text-embedding-3-small
    pub embedding_floor: f32,                // 默认 0.55（需用 eval 校准）
    pub embedding_margin: f32,               // 默认 0.05（需用 eval 校准）
    pub prior_boost: f32,                    // 默认 0.04
    pub llm_rerank_enabled: bool,            // 默认 true
    pub llm_timeout_ms: u64,                 // 默认 8000
    pub probe_enabled: bool,                 // 默认 true
    pub probe_timeout_ms: u64,               // 默认 1500
    pub second_rerank_min_budget_ms: u64,    // 默认 1200
    pub routing_budget_ms: u64,              // 默认 12000
}
```

`embedding_floor` / `embedding_margin` 的默认值必须在 §10 的 eval 集上校准后写死，而不是凭感觉。

### 7.3 共享 EmbeddingService

- 位置：`llm/embedding.rs` 或 `llm/intent/embedding.rs`（实现者决定；不得让 `core` 依赖 `features`）。
- `MemoryManager` 与 `IntentRouter` 各持有同一个 `Arc<EmbeddingService>`，由 `AppState::new` 构造一次。
- 接口：`async fn embed(&self, text: &str) -> Option<Vec<f32>>`、`fn available(&self) -> bool`。
- 失败只返回 `None`，由调用方降级。

---

## 8. 调用链与可观测

### 8.1 `prepare_turn` 变更

1. 落库用户消息（不变）。
2. 取最近 6 条消息作为 `RouteContext.recent_messages`（不含当前消息；当前消息单独作为 `content` 传入，避免重复）。
3. `state.intent_router.route(context)` → `RouteDecision`。
4. 写回 prior（不变：`auto` 覆盖，`pinned` 不覆盖）。
5. 发 `intent_resolved`（新字段见下）。
6. 由 `RouteDecision.chosen` 生成 `ExecutionPolicy`，替换现有分散的字段计算。
7. 进入 fast / deep。

### 8.2 事件

`intent_resolved` 调整为：

```json
{
  "session_id": "...",
  "domain_id": "...",
  "domain_name": "程序开发",
  "intent_id": "builtin.software_dev.implement",
  "intent_name": "实现功能",
  "confidence": 0.78,
  "uncertain": true,
  "source": "llm",
  "candidates": [
    { "intent_id": "...", "name": "...", "score": 0.78 }
  ],
  "elapsed_ms": 412,
  "llm_calls": 1,
  "probe_used": false
}
```

移除 `widened`（不再有扩域概念）。

### 8.3 真实阶段

- 删除 `send_message_stream` 里进入 `prepare_turn` 之前无条件发出的 `classifying`。
- 路由开始时发 `stream_chunk.phase = "classifying"`（由 `prepare_turn` 内部发，或由调用方在调用前后发），路由结束发 `intent_resolved`。
- 目标：前端看到的阶段与真实工作一致，不再出现"已经显示分类中，但其实还没开始"或反过来的情况。

### 8.4 决策追踪

- `IntentRouter` 内维护容量 50 的 `VecDeque<RouteTrace>`（进程内，不落库）：时间、session_id、消息摘要（≤80 字）、候选与信号分数、最终选择、`uncertain`、`source`、耗时、`llm_calls`、`probe_used`。
- 新增只读 IPC `get_recent_route_traces() -> Vec<RouteTrace>`，给设置页/调试面板使用；不在主聊天界面展示。
- 目的：用户反馈"它理解错了"时，能直接看到当时有哪些候选、差多少、走的哪条信号。

---

## 9. 降级阶梯

1. 路由总开关关闭 → 合成闲聊（`source = Fallback`，`uncertain = false`）。
2. 无启用意图 / 无领域 → 合成闲聊，不发起任何网络请求。
3. 索引尚未预热完成 → embedding 信号跳过，本条消息走 LLM（不阻塞等待索引）。
4. 无 embedding provider（无 key / 初始化失败）→ 跳过 embedding；直接 LLM rerank（候选 = 全部启用意图，≤40 条）。
5. LLM 明确回答 `none`（`NoMatch`）→ 有 embedding 用 embedding top1，否则直接合成闲聊；**不得调用 RuleScorer**。
6. LLM 超时/失败/回复不可解析（`Unavailable`）→ 不重试；有 embedding 用 embedding top1，否则用 RuleScorer；低于阈值 → `uncertain = true` 继续。RuleScorer 不覆盖 embedding。
7. 探针超时/失败 → 当作无证据，继续。
7. 全流程任何一步都不得让消息发不出去；路由层不向调用方返回 `Err`，只返回决策。

---

## 10. 测试与验收

### 10.1 单元测试

**决策合并**
- embedding 明确命中（top1 ≥ floor 且 margin 足够）→ `source = Embedding`，不调用 LLM。
- margin 不足 → 调用一次 LLM；LLM 过阈值 → `source = Llm`。
- LLM 低于阈值 + `probe_enabled = false` → 选中 LLM 给出的 id，`source = Llm`，`uncertain = true`，不调用探针。
- LLM 低于阈值 + 探针二次 rerank 过阈值 → `source = Probe`，`uncertain = false`。
- LLM 低于阈值 + 探针二次 rerank 仍低/失败 → 选中第一次 LLM 的 id，`source = Llm`，`uncertain = true`。
- `llm_rerank_enabled = false` + embedding 不 decisive → 选中 embedding top1，`uncertain = true`。
- LLM 返回 `none`（NoMatch）+ embedding 可用 → 选中 embedding top1，**不调用 RuleScorer**。
- LLM 返回 `none`（NoMatch）+ embedding 不可用 → 合成闲聊（`source = Fallback`，`uncertain = false`），保留 `llm_calls` 证据。
- LLM 超时/失败（Unavailable）+ 无 embedding → RuleScorer 兜底；规则也无命中 → 合成闲聊（`uncertain = true`）。
- `prior_boost` 只改变候选排序，不参与 floor / margin 判定，不把低于 floor 的候选抬成 decisive。
- 剩余预算不足以覆盖探针 + 二次 rerank → 跳过探针，`uncertain = true` 返回。
- `pinned`：候选只来自该领域；领域无启用意图 → 合成闲聊。
- `pinned` 指向已删除领域 → 当 auto 处理。
- `auto` prior 加分只改变排序，不把候选限制在 prior 领域。
- 无候选 → 合成闲聊，`uncertain = false`。

**降级**
- 无 embedder、LLM 可用 → 只走 LLM。
- 无 embedder、无 LLM → 只走 RuleScorer，结果带 `uncertain`。
- LLM 超时 → 不重试；返回当前最佳结果。
- 任何信号失败都不 panic、不返回 `Err`。

**Embedding 索引**
- 预热在后台完成；`ready = false` 时 route 不等待索引，直接走 LLM。
- 意图 CRUD 后置 dirty 并在后台重建；重建期间旧索引继续服务。
- 删除意图后索引不再包含它。
- embedding 返回 `None` 时整个信号被跳过。
- 后台预热失败（无 key）时 `available()` 持续为 false，不反复重试。

**规则打分**
- 长度 1 的 example 不再参与子串命中（修复「改成」类误命中）。
- 子串命中分 0.9；bigram 仍能给出正分。

### 10.2 Eval 集（手动/CI 可选）

- `src-tauri/tests/intent_routing_eval.rs`，用 `INTENT_ROUTING_EVAL=1` 启用；默认 `#[ignore]`。
- 约 40 条标注消息：12 个内置意图各 2-3 条明确句、10 条跨域/上下文句、5 条域外闲聊、5 条模糊句。
- 输出：top-1 accuracy、uncertain 比例、LLM 调用次数分布、p50 耗时。
- 验收门槛：
  1. 明确句 top-1 accuracy **不低于改造前的基线**（先测基线再改）；
  2. 模糊句 100% 标记 `uncertain = true`（允许选错，但必须承认）；
  3. 明确句里能由 embedding 决定的比例 ≥ 60%（即 LLM 调用显著下降）；
  4. 全部用例消息都能得到决策（无 `Err`）。

### 10.3 回归

- `cargo test` 全绿（现有 router 的扩域/fallback 测试按新语义重写或删除，不保留死语义）。
- `cd src-ui && npx tsc --noEmit` 通过。
- 前端 `IntentResolvedBadge` 测试更新：`uncertain` 展示、候选 tooltip、移除 `widened`。

---

## 11. 前端

| 位置 | 改动 |
|---|---|
| `IntentResolvedBadge` | `uncertain = true` 时显示「可能：<意图名>」；hover 列出候选与分数；移除 `widened` |
| 当前轮消息上方 | 新增一行非阻塞提示：「不确定意图，按「X」继续；可切换领域后重发」 |
| `IntentManagerPage` | 移除 `fallback_intent` 编辑项；`confidence_threshold` 说明改为「LLM 置信度阈值」；标注 embedding 索引状态（可用/不可用） |
| 调试面板 | 可选：展示 `get_recent_route_traces` 的最近 50 条决策 |
| 阶段提示 | 与真实路由事件对齐；不再出现假的 `classifying` |

不确定提示的具体样式交由 @designer 决定，但必须满足：不阻塞、不弹窗、用户能一眼看出当前按哪个意图继续。

---

## 12. 方案取舍记录

| 方案 | 做法 | 取舍 |
|---|---|---|
| **A. 信号 → 决策 → 策略（选定）** | 三层解耦，信号并行/分级，决策可解释 | 结构清晰、可单测、可扩展；需要重建索引与决策合并规则 |
| B. 纯 embedding kNN | 只做向量召回 + margin，不调 LLM | 最快最便宜；对细粒度语义（解释 vs 重构）不够稳，无兜底推理 |
| C. 纯 agentic 路由 | 分类器带只读工具，自主查证 | 最擅长上下文句；每次路由都贵且慢，确定性差 |

选定 **A 为骨架、B 为默认信号、C 只服务 ambiguous 分支**：常见消息一次 embedding 决定，薄边界花一次 LLM，真正说不清才探针；三者都有明确降级路径。

---

## 13. 本次拍板的决定

1. **不确定时继续执行最高分意图，并明确告知**（用户确认）。
2. **不新增数据库表/列**；embedding 索引进程内、后台预热、CRUD 后台重建。
3. **移除扩域与 fallback 链**；`pinned` 只作硬过滤，prior 只作排序加成。
4. **`fallback_intent` / `.clarify` 后缀 / 最小 `sort_order` 三个隐式约定全部取消**；`fallback_intent` 字段保留但不再读取。
5. **不确定不放宽工具白名单**。
6. **本版不做 fast → deep 运行时升级**。
7. **探针严格只读、限项目根、最多 1 轮、可关闭**。
8. **决策证据只做进程内环形缓冲 + IPC**，不落消息表。
9. **「最可能」按信号优先级取**：探针后 rerank > LLM rerank > embedding top1 > RuleScorer；不做跨来源分数加权。
10. **`prior_boost` 只影响候选排序**，不参与 `floor` / `margin` / 阈值判定。
11. **RuleScorer 不覆盖 embedding**，只在 embedding 不可用时运行。
12. **路由硬预算 12s**；剩余预算不足以覆盖探针 + 二次 rerank 时跳过探针。
13. **索引后台预热**，首条消息不等索引；未 ready 时走 LLM。

---

## 14. 风险

| 风险 | 处理 |
|---|---|
| embedding 阈值/ margin 凭感觉定，导致误判 | 用 eval 集校准后写默认值；暴露到配置；上线后可用 `RouteTrace` 调整 |
| 无 OpenAI key 时退化成"每条消息一次 LLM"，比现在更慢 | 保留 RuleScorer；`source`/`llm_calls` 可观测；文档明确该降级 |
| 意图数量增长导致索引构建慢/贵 | 本版可接受（内置约 60 单元）；超过阈值再单独立项持久化 embedding |
| 探针读文件带来隐私/性能问题 | 只读、限项目根、限 1 轮、限时、可关闭；不改用 `ToolRegistry` |
| 取消 fallback 链改变旧自定义意图行为 | 发布说明点明；`fallback_intent` 字段保留，避免数据丢失 |
| 不确定提示过于频繁，干扰体验 | 只在低于阈值时出现；embedding 明确命中时不会触发；用 eval 观察比例 |
| 事件字段变化导致旧前端崩溃 | 前后端同一次 change 更新；`intent_resolved` 是新增字段而非改类型，旧字段除 `widened` 外保留 |

---

## 15. 留给实现者

- embedding 索引的具体数据结构与并发粒度（`RwLock` vs `Mutex`）。
- LLM rerank 的 prompt 文案与候选列表格式；`none` 的措辞。
- 探针搜索用哪些关键词、文件类型过滤、结果截断策略。
- `RouteTrace` 的字段序列化形状与调试面板是否本版就要做。
- eval 集的 40 条标注样本（应由熟悉内置 12 意图的人写，而不是从 examples 复制）。
- 前端不确定提示的具体文案与视觉层级。

---

## 16. 后续（本次不做）

- fast → deep 运行时升级（例如工具循环发现多步需求后升级）。
- 意图反馈闭环：用户纠正后把消息加入 examples 或调整阈值。
- embedding 持久化与跨机器同步。
- 多意图/并行意图。
- 按项目/文件类型收窄 `scope`。

---

## 17. 发布说明

- **意图路由改为 embedding 优先**：embedding 明确命中时不再调用分类 LLM；边界模糊时才 rerank。没有 OpenAI key 时退化为 LLM + 规则，消息仍可正常发送。
- **`fallback_intent` 不再生效**：字段保留在库中，但路由器不再读取；旧的自定义兜底链会停止改道。
- **`.clarify` 后缀不再有特殊语义**：澄清意图只是一个普通候选，只有分数足够高才会被选中。
- **`sort_order` 只影响展示排序**，不再作为兜底顺序。
- **`intent_resolved` 事件变更**：移除 `widened`；新增 `uncertain`、`candidates`、`elapsed_ms`、`llm_calls`、`probe_used`；`source` 取值变为 `embedding | llm | rule | probe | fallback`。
- **新增只读 IPC** `get_recent_route_traces`，返回最近 50 条路由决策，用于调试面板。
- **配置新增**（全部有默认值，旧配置可直接加载）：embedding 模型、floor/margin、prior boost、LLM/probe 超时、路由总预算。
- **无数据库迁移**：不新增表或列；embedding 索引只在进程内，启动后台预热。
- **不确定提示**：低于阈值时按最可能意图继续执行，消息上方显示「不确定意图，先按「X」继续」；工具白名单不因不确定而放宽。
