# 意图领域 → Agent 运行时集成 — 设计

日期：2026-11-05
状态：已实施（2026-11-05）
关联：`docs/design/2026-11-04-intent-management-design.md`（前序，本设计承接其 §2「非目标」中推迟的运行时接入）

---

## 1. 为什么做

意图管理模块已经落地：领域/意图入库、12 条内置种子、完整 CRUD、导入导出、LLM + 规则双路分类器。
但它对 Agent 的影响只到「聊天层的三个旋钮」——提示词附录、工具白名单、迭代上限。用户切换领域时，
大部分行为没有变化，分类结果也没有被任何地方复用。

本次要解决的是**聚焦**：用户启用某个领域后，Agent 应该更准确地理解这句话要做什么，并按该领域内
这类事务的既定方式执行。

前序设计 §2 明确把三件事推后，本次正是这三件：

- 不驱动 `workflow` / `default_agent`（字段存而不用）；
- 不做会话级领域绑定（`is_active` 全局单例）；
- 不改 `ToolLoop`、orchestrator、工具注册表的行为。

**落地路径与规模。** 按项目约定走 OpenSpec：本文件作为 `/opsx-propose` 的输入，change 内产出
proposal / specs / tasks，实现按 TDD 与完成前验证执行。规模属于一次中等改动——一次会话表迁移、
路由改造、两条执行路径接线、一个前端选择器，一个 change 能闭环，不需要拆阶段。

---

## 2. 概念模型

**领域 = 一个业务场景下「可以做的事」的分类清单。**
它不是时间顺序上的流程阶段，没有"第几步"的概念，也没有会话级状态机。

**意图 = 清单里的一类事**，附带这一类事的执行配方（工具、提示、模型、迭代上限、快/深执行）。

**选中意图 = 决定这件事怎么干**。所以"领域决定工作流"的准确含义是：领域收窄可选项，意图给出配方，
配方决定执行方式——而不是领域绑定一条 YAML 流水线。

三条推论，直接约束实现：

1. 不需要 `intent` 之间的转移关系，`fallback_intent` 保持现在的语义（低置信度改道目标）。
2. 不需要会话级"当前阶段"。会话只记住**上一次落在哪个领域**，作为下一次判断的先验。
3. 领域判定是**每条消息**做的，不是会话绑定的；通常一个会话一个领域，是结果，不是约束。

---

## 3. 现状与证据

| 事实 | 位置 |
|---|---|
| 分类器只读「当前激活领域」的启用意图，全项目没有"选领域"这一步 | `llm/intent/router.rs:39-118` |
| 领域是全局单例：`is_active` + `set_active_intent_domain` | `core/db/repository.rs`、`app/commands/intent.rs` |
| `Session` 没有领域字段，只有 `mode: chat/autonomous` | `core/db/models.rs:33-60` |
| fast 路径吃到 appendix / tools / max_iterations，但**忽略** `intent.model_id` | `app/commands/session.rs:541-550, 564-569, 615-625` |
| deep 路径只取 `model_id` 和 goal，工具、提示、上限全丢 | `app/commands/session.rs:377-409` |
| `OrchestratorAgent::process_goal` 只造一个 Thinker 节点，不读会话历史 | `agent/orchestrator/agent.rs:389-404` |
| `auto_escalate` 分支实际不可达：`path == Deep` 先 return，而所有 `auto_escalate=1` 的种子都是 deep | `session.rs:377, 412`；种子见 `features/intent_management/seeds.rs` |
| 非流式 `send_message` **完全不做意图路由**，与流式入口行为不一致 | `app/commands/session.rs:176-320` |
| Planner 把**全部**启用工具写进提示词，不看意图白名单 | `agent/orchestrator/planner.rs:34-45` |
| `ExecStep::AgentTask` 已支持 `allowed_tools` / `max_iterations` / `model_id`，运行时确实用上了 | `plan_types.rs:47-57`、`runtime/executor.rs:255-346` |
| 种子里的 `workflow` / `default_agent` / `scope` 仍无消费者 | 全项目 |

最后两条决定了 deep 的落点：`ExecutionRuntime` 已经是 `OrchestratorAgent::process_goal` 的超集——
它按步执行、支持工具白名单、有重试、有进度事件、有取消/暂停、计划可持久化。

---

## 4. 方案

三条把「领域判定」接进消息流的路线，已选定 A。

**A. 级联分类（选定）**
先用会话上次的领域作为候选范围分类；置信度不过阈值，才扩到全部领域再判一次。
常见情况一次 LLM 调用，越界才两次。领域天然稳定，跑题才会换。
改动集中在 `IntentRouter::classify` 的入参与返回，外加 `sessions` 两列。

**B. 联合分类**
一个提示词里同时列领域和意图，输出 `{domain, intent, confidence}`。永远一次调用，但候选集与提示词
随领域数量膨胀，模型还可能给出"领域与意图不匹配"的组合，需要额外校验。领域多了退化。

**C. 两级串行**
先领域分类器（用 `domain.name/description`），再在命中领域内跑现有 `LlmClassifier`。
分层最干净、单层最好测，但常规路径就两次往返，延迟翻倍，领域判错会级联。

取舍：A 在常见路径上既省又稳，B 对规模敏感，C 结构最漂亮但最慢。选 A。

---

## 5. 数据模型变更

### 5.1 `sessions` 两列（迁移 v11）

| 列 | 类型 | 说明 |
|---|---|---|
| `intent_domain_id` | TEXT NULL | 上一次落在的领域，在 `auto` 模式下仅作先验 |
| `intent_domain_mode` | TEXT NOT NULL DEFAULT 'auto' | `auto` / `pinned` |

迁移写在 `core/db/repository.rs::migrate_tables`，沿用 v6–v10 的 `ALTER TABLE ... ADD COLUMN` 写法。
不设外键：本项目 `PRAGMA foreign_keys = OFF`（前序设计 §5.2）。领域删除时由仓储层显式
`UPDATE sessions SET intent_domain_id = NULL WHERE intent_domain_id = ?`，与删除意图子行放在同一事务。

**`pinned` 是「用户为这个会话指定了领域」，不是「消息不能换领域」。**
两种模式的行为差别只有一条：`pinned` 不做扩域，分类器只在该领域的清单里选。

### 5.2 `intents` 不新增列

配方所需的字段全部已存在。本次只做两件事：

- **运行时不再读 `auto_escalate`**，`path` 取 `fast | deep` 两值。列保留不删（SQLite `DROP COLUMN`
  与旧数据回滚都不值得为一次语义收口冒险），在模型层标 `#[deprecated]` 注释。
- `workflow` / `default_agent` / `scope` **本次仍不消费**，前端继续标注"后续接入"。它们要等
  领域真正绑定业务流水线时才启用；本设计不承诺。

### 5.3 `is_active` 语义收窄

`intent_domains.is_active` 从"全局当前领域"改为"**新会话的默认领域**"：

- 新会话创建时若未指定领域，`intent_domain_id` 取当前 `is_active` 的领域。
- `set_active_intent_domain` 的既有守卫（零启用意图的领域不可激活）保留。
- 现有 IPC 名称与签名不变，前端把"活跃领域"文案改成"默认领域"。

---

## 6. 路由设计

### 6.1 接口

```rust
pub struct RouteRequest<'a> {
    pub content: &'a str,
    /// 会话上次的领域。None = 没有先验。
    pub prior_domain: Option<&'a str>,
    /// pinned 时不扩域。
    pub pinned: bool,
}

pub struct RouteOutcome {
    pub intent: IntentResult,     // 现有结构，增加 domain_id / domain_name
    pub source: RouteSource,      // Llm | Scorer | Fallback
    pub widened: bool,            // 是否发生了扩域
}
```

`IntentResult` 增加 `domain_id`、`domain_name`。`classify()` 的旧签名保留为
`classify(content)`（等价于无先验、不 pinned），避免一次性改爆所有调用点。

### 6.2 级联

```
1. prior 存在且该领域仍有启用意图：
   a. LlmClassifier 返回命中且 score >= 该意图 confidence_threshold
        → 采用（source = Llm, widened = false）
   b. LlmClassifier 命中但分数低于阈值
        → 记一次"域内低置信"，进 2
   c. LlmClassifier 返回 None（无 provider / 请求失败 / 解析失败）
        → 用 score_intents 在域内兜底
             命中过阈值 → 采用（source = Scorer, widened = false）
             未命中 → pinned 进 4，否则进 2
2. 若 pinned → 进 4（不扩域）
3. 扩域：对全部领域的启用意图跑同一条链（LLM → scorer）
     命中且过阈值 → 采用（widened = true）
     命中但低置信 / 未命中 → 进 4
4. 兜底：在"最后一个给出候选的领域"内依次走
     fallback_intent → 该领域 clarify → sort_order 最小的启用意图
   没有候选领域时：prior 存在用 prior；否则用默认领域（is_active）；都没有进 5
5. 全库无启用意图 / 无领域：返回合成的 fast chat 结果（现有 fallback），
   intent_domain_id 写回 NULL——不伪造一个领域
```

**"LLM 失败"与"低置信"必须分开。** 前者是 `None`，后者是一个低于阈值的分数。把两者混在一起，
会在没有 provider 的环境里对每条消息多发一次注定失败的请求；也会在 pinned 模式下把"模型挂了"
误判成"用户跑题了"。

**为什么不把规则打分器提到 LLM 前面。** 现有行为是 LLM 优先、打分器兜底，`router.rs` 的测试也是按
这个顺序写的。例子子串命中（score 1.0）看起来可以省一次调用，但"谢谢"这类短例子在长句里会误命中，
收益不值这个行为变化。留作后续优化。

**扩域的判定条件**是"域内不过阈值"，不是"域内没命中"。分类器永远会给出一个 id，所以必须用阈值
而不是 Option 来判断越界——这正是前序设计里 `apply_confidence` 存在的原因。

### 6.3 先验的写回

一次消息结束后：

- `auto` 模式：`sessions.intent_domain_id = 本次命中的领域`。
- `pinned` 模式：不覆盖。
- 用户在前端选「自动」：`mode = auto`，保留现有 `intent_domain_id` 作为先验。
- 用户选了某个领域：`mode = pinned`，写入该领域。
- 领域被删除：所有引用它的会话置 NULL、模式回落 `auto`。

### 6.4 新增 IPC

| 命令 | 说明 |
|---|---|
| `get_session_intent_state(session_id) -> { domain_id, domain_name, mode }` | 打开会话时渲染选择器 |
| `set_session_intent_domain(session_id, domain_id: Option<String>)` | `None` = 自动；`Some` = pinned |

`None` 表示自动而不是"清空领域"，避免前端出现"无领域"这种不可表达的状态。

### 6.5 可观测

新增一个 Tauri 事件 `intent_resolved`：

```json
{ "session_id": "...", "domain_id": "...", "domain_name": "程序开发",
  "intent_id": "builtin.software_dev.implement", "intent_name": "实现功能",
  "confidence": 0.82, "source": "llm", "widened": false }
```

前端用它渲染消息旁的领域/意图徽标。前序设计 §3 的"不自造新事件"是针对管理模块的约束；
这里不新增事件就只能让前端去解析 `execution_log` 的自由文本，得不偿失。

**徽标是临时的，不落库。** `messages` 表与前后端 `Message` 类型都不加意图字段：事件不带 `message_id`，
前端把它挂到当前正在进行的这一轮上即可。需要跨会话保留的只有"这个会话的领域"，而它在 `sessions` 里。
如果将来要"回看消息时也能看到当时的意图"，那是消息级 schema 变更，另立设计——不在本次范围。

---

## 7. 运行时接线

### 7.1 两条路径，不是三条

`auto_escalate` 分支删除（证据见 §3）。聊天只剩：

| 模式 | 实现 | 用途 |
|---|---|---|
| `fast` | `ToolLoop`（现有） | 单 agent 工具循环 |
| `deep` | `LlmPlanner` → `ExecutionPlan` → `ExecutionRuntime` | 多步执行、进度、取消 |

`deep` 改为同步 await：边执行边把 `plan_progress` 推给前端，结束后把最终文本作为 assistant 消息返回。
非流式 `send_message` 与流式入口共用同一个 `prepare_turn()`，不再各自实现一套。

### 7.1.1 deep 不得把会话变成"自主模式"

`ExecutionRuntime::execute` 现在会无条件写 `sessions.mode = 'autonomous'`、`active_plan_id`、
`execution_status`（`agent/orchestrator/runtime.rs:86-100`）。若原样复用，每一轮 deep 聊天都会在侧栏
亮起"自主模式"徽标、按 Esc 触发取消（`Sidebar.tsx:199`、`ChatArea.tsx:155-167`），把一个聊天轮次
变成一次会话状态迁移。

给 `ExecutionRuntime` 增加执行范围参数：

```rust
pub enum PlanRunMode {
    /// 自主模式：写 session.mode / active_plan_id，前端展示计划时间线与确认框
    Autonomous,
    /// 聊天轮次：只执行 + 推 plan_progress，不动 session 状态
    Interactive,
}
```

聊天 deep 一律用 `Interactive`：

- 不写 `session.mode` / `active_plan_id`；轮次结束后会话仍是 `chat`。
- 不发 `plan_generated`——它会打开 `PlanConfirmDialog`（`PlanConfirmDialog.tsx:37-41`），
  聊天里每条 deep 消息弹一次确认框是不能接受的。
- 只发 `plan_progress`，前端用轻量进度条展示，不挂 `activePlan`（`PlanTimeline` 的
  `sessionMode === 'autonomous'` 门槛保持不变）。
- `ExecutionPlan` 仍然落库，作为历史记录；这不需要改运行时。
- 取消：命令层在轮次开始时把 `ExecutionHandle` 放进 `active_executions`，结束后移除，
  沿用 `cancel_execution` 现有的取句柄逻辑（`app/commands/execution.rs:100-112`）。

`OrchestratorAgent` 类型与测试保留不动，只是聊天不再路由到它。
删掉的是"作为聊天 deep 路径的 `process_goal` 调用"，不是类本身——避免连带删除 worker/critic 的测试面。

### 7.2 配方贯穿

| 配方字段 | fast | deep |
|---|---|---|
| `system_prompt_appendix` | 现有 system 消息 | 既进 planner 的 system 提示，也作为每个 `AgentTask` 的前置 system |
| `allowed_tools` | 现有 `resolve_tools` | planner 的**工具目录只列白名单内工具**；生成的 `AgentTask` 默认带上同一份 `allowed_tools` |
| `model_id` | **补上**（当前被忽略） | planner 与 `AgentTask` 的默认模型 |
| `max_iterations` | 现有 | `AgentTask` 的默认上限 |

模型优先级统一为：`intent.model_id` > persona 的 `model_name` > `session.model_id`。
现在 fast 路径用的是 persona/session，deep 用的是 intent/session，两条不一致——这是要修的缺口，
不是新设计。

**模型不存在时逐级回退。** `intent.model_id` 可能指向一个已被删除或没有 API Key 的模型（导入的意图
尤其容易）。解析模型时先查 `ProviderRegistry`，查不到就 warn 一次并落到下一级，而不是让
`ToolLoop::run` 里的 `registry.get()` 直接把这一条消息打回去——这与 §7.4 的"消息必须发得出去"冲突。

**工具权限只有一个来源：`resolve_tools(session.enabled_tools, intent.allowed_tools)`。**
deep 不另开一套规则：

- 白名单 = 会话配置与意图白名单的交集（现有函数语义，空数组表示"一个都不给"）。
- `tools_enabled == false` 时，planner 拿到空工具目录，`ExecutionRuntime` 拒绝任何 `ToolCall` /
  带工具的 `AgentTask`。运行时这一层不能只靠提示词——planner 仍可能产出 `tool_call`。
- 这两条要各写一个测试，断言"白名单外的工具不会被调用"，而不只是断言提示词里没列出来。

`LlmPlanner` 增加 `allowed_tools: Option<&[String]>` 参数；白名单为空数组时 planner 只应产出
`llm_call` / `condition`，不应出现 `tool_call`——落到 `tool_call` 也得在运行时再拒绝一次。
这条双保险要写进测试。

### 7.3 会话上下文与人格

`deep` 现在完全看不到对话历史。本次给 planner 传一段有界上下文：最近 6 条消息的纯文本，
按字符数截断（上限 4000 字符），作为 goal 的前缀。不做逐步骤注入——步骤指令由 planner 自己承担。
这样做是为了让"就按刚才说的做"这类句子在 deep 路径上不至于失忆。

system 侧的组成，按角色分配，避免逐字重复：

| 位置 | 内容 |
|---|---|
| planner 上下文首段 | `session.system_prompt`（空则 `DEFAULT_SYSTEM_PROMPT`）+ persona 段 + 意图 `system_prompt_appendix` |
| `AgentTask` system | `session.system_prompt` + 意图 `system_prompt_appendix`（persona 不重复注入） |
| planner 上下文其后 | 相关记忆（`memory.build_context_prompt`，一次）+ 最近 6 条消息 |

实现时发现 `LlmPlanner` 没有独立的 system 参数，且 planner 的 system 里必须是"只输出 JSON"的硬约束。
把人格与提示词附录塞进 planner system 会冒破坏 JSON 输出的风险，所以上表第一行改为汇入上下文首段；
真正面向用户的那段话由 `AgentTask` 产出，而它拿到的是正确的 system（session prompt + 意图附录）。
这是对原设计的**有意收窄**，不是遗漏。

persona 只进 planner：它是"这个助手是谁"的总体设定，逐步骤重复注入会让子任务跑成角色扮演，
反而稀释了步骤指令。记忆同样只注入一次，避免每一步都把同一段记忆带进上下文。

`AgentTask` 的 system 目前硬编码在 `runtime/executor.rs:309-316`，需要改成可传入；空值时保留现有
文案作为默认。

### 7.4 降级阶梯

1. `pinned` 的领域已被删除 → 当 `auto` 处理，记一次 warn。
2. 会话没有先验、也没有默认领域 → 直接用扩域结果。
3. 分类器不可用（无 provider / 请求失败）→ 域内规则打分 → fallback 链 → 合成 chat。
4. **planner 失败（还没执行任何步骤）** → 回落到 fast 路径，并把失败写进 `execution_log`（现有行为，保留）。
5. **计划已开始执行、第 N 步失败** → **不回落 fast**。此时可能已经写过文件、跑过命令，再走一遍
   fast 会把副作用做第二次。返回"前 N-1 步的结果 + 失败步骤与原因"，作为这一轮的 assistant 消息，
   并把该轮标成失败状态（`plan_progress` 的 `failed`）。部分结果由已完成步骤的 `result` 拼成，
   没有可用文本时只报错误。
6. 任何一步都不得让消息发不出去——这是前序设计的硬约束，本次继承。

第 4 与第 5 的分界是"有没有副作用"，不是"失败发生在哪里"。这条要写进测试：一个第 2 步失败的
计划，断言 fast 路径没有被调用。

---

## 8. 前端

| 位置 | 改动 |
|---|---|
| 会话头部 | 领域选择器：`自动` + 领域列表；显示当前模式 |
| 消息流 | 每条消息旁显示 `intent_resolved` 带来的领域/意图徽标，hover 显示 confidence 与是否扩域 |
| `IntentManagerPage` | "活跃领域" 文案改 "默认领域"，并说明"只影响新会话的初始领域" |
| `IntentManagerPage` | `auto_escalate` 开关移除；`path` 保留 fast/deep |
| `IntentManagerPage` | `workflow` / `default_agent` / `scope` 仍标注"后续接入"并禁用 |

会话创建流程：`create_session` 不新增必填参数，未指定时后端取默认领域。

---

## 9. 测试

**路由**
- 域内命中且过阈值：只调用一次分类器，`widened = false`。
- 域内不过阈值、扩域命中：`widened = true`，返回的是新领域的意图。
- `pinned`：域内不过阈值时**不扩域**，走该域 fallback 链。
- `pinned` 指向已删除领域：回落 auto，不 panic。
- 无领域 / 零启用意图：合成 chat，不发 LLM 请求（沿用现有测试）。
- `fallback_intent` 为空时终结，不再跳转。

**会话状态**
- `auto` 模式：消息后 `intent_domain_id` 被写成本次命中领域。
- `pinned` 模式：不被覆盖。
- 删除领域：引用它的会话置 NULL 且模式回落 `auto`。
- 迁移 v11：旧库升级后两列存在，默认 `auto`，既有会话不报错。

**配方接线**
- fast：`intent.model_id` 优先于 persona 与 session（当前会失败，先写红）。
- fast：`intent.model_id` 指向不存在的模型 → 逐级回退到 persona/session，不报错。
- deep：planner 收到的工具目录只含白名单内工具；白名单为空时不产出 `tool_call`。
- deep：`tools_enabled = false` → planner 拿到空目录，且运行时拒绝任何工具步骤（不是只靠提示词）。
- deep：`AgentTask` 带上 `allowed_tools` / `max_iterations` / `system_prompt_appendix`。
- deep：planner 的 goal 里包含最近 6 条消息的截断上下文与一次记忆注入。

**deep 的会话状态**
- `Interactive` 执行一轮后：`session.mode` 仍为 `chat`，`active_plan_id` 仍为 NULL。
- 不发出 `plan_generated`（否则会弹确认框）。
- 轮次结束后 `active_executions` 里没有残留句柄。
- 计划执行到第 2 步失败：返回部分结果，且 fast 路径**没有被调用**。

**分类器的失败与低置信**
- provider 不可用（`LlmClassifier` 返回 `None`）：不再发第二次请求，直接走域内 `score_intents`。
- 域内低置信 + pinned：不扩域，走域内 fallback。
- 域内低置信 + auto：扩域一次，`widened = true`。

**入口一致性**
- 非流式 `send_message` 与流式入口对同一条消息给出相同的 domain/intent 判定。

**回归**
- 现有 `cargo test` 全绿；`npx tsc --noEmit` 通过。

---

## 10. 风险

| 风险 | 处理 |
|---|---|
| 越界消息要两次 LLM 调用，首字延迟变高 | 只在"域内不过阈值"时扩域；`intent_resolved` 里暴露 `widened` 便于观测占比 |
| 分类提示词随领域/意图增长 | 全库启用意图超过 40 条时，扩域步骤改为"先选领域再选意图"；本次不做，但把阈值写进注释 |
| 去掉 `auto_escalate` 改变手工配置的意图行为 | 内置种子无一可达该分支；用户自建意图若为 `fast + auto_escalate`，升级后变 fast。发布说明里点明 |
| deep 同步 await 让请求变长 | 执行期间持续推 `plan_progress`；沿用现有 `ExecutionHandle` 取消能力 |
| 会话先验导致"越界越来越难" | 扩域是每次消息都尝试的，不是只在新会话；`pinned` 由用户显式选择 |
| 删除领域时遗漏会话清理 | 清理语句与子行删除同事务，并在测试里断言 |

---

## 11. 本次拍板的决定

评审提出的八个问题里，没有必须由产品方裁决的，都按"不破坏既有语义、副作用只发生一次、消息必须发得出去"
三条原则定下来了：

1. **deep 是聊天轮次，不是自主模式。** 用 `PlanRunMode::Interactive`，不写 `session.mode`、不弹确认框；
   计划仍落库作历史。
2. **工具权限沿用 `resolve_tools`。** 会话配置与意图白名单取交集，`tools_enabled=false` 时 deep 也拿不到
   工具；运行时二次拒绝，不只靠提示词。
3. **执行中失败不回落 fast。** 已发生的副作用不能再做一遍；返回前 N-1 步结果 + 失败原因。
4. **徽标不落库。** 只在当前轮次展示；`messages` 表不动。
5. **deep 复用计划进度，不复用自主模式的时间线与确认框。** 取消句柄在轮次内存放。
6. **分类器的 `None` 与"低置信"分开处理。** 前者不重试第二遍，后者按 pinned/auto 决定是否扩域。
7. **deep 继承 system prompt 与意图附录；persona 只进 planner；记忆只注入一次。**
8. **模型不可用时逐级回退。** `intent.model_id` 查不到就 warn 后降到下一级。

## 12. 留给实现者

- 级联第二次调用的提示词是否需要标注"这是扩域判断"（倾向不加，复用现有 `build_classifier_prompt`）。
- deep 最终 assistant 文本的取法：建议取最后一个产出文本的步骤结果，若无则输出步骤摘要表。
- `intent_resolved` 事件在路由完成后立即发，还是随首条 `stream_chunk` 一起发（倾向立即，避免气泡先出现再变形）。
- 前端徽标与轻量进度条的具体样式。
- 是否把 `path` 改名 `mode`（纯命名，不影响行为；倾向本次不做，减少迁移噪音）。

## 13. 后续（本次不做）

- 领域绑定业务流水线（`workflow`）与执行体（`default_agent`）。
- `intents.scope` 的按项目/按文件类型收窄。
- 基于 embedding 的领域/意图召回。
- 内置领域的版本升级与迁移。
- 消息级的意图历史（回看旧消息时显示当时的领域/意图）。
