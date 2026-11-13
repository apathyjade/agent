# 计划：把「领域预期管理」升级为请求级资源裁剪（Scope Routing）

> 状态：草案（仅计划，未开工）
> 范围调整：**skills 延期**，本轮只做 tools / MCP / prompts 三个维度
> 关联：`llm/intent/*`、`app/commands/intent_plan.rs`、`app/commands/session.rs`、`agent/tools/*`、`features/mcp/*`

## 1. 现状判定：它确实只是「路由」，而且只裁了一个维度

当前链路（代码定位）：

| 层 | 载体 | 位置 |
|----|------|------|
| L1 领域 | `IntentDomainRecord`（id/name/description/is_active/version） | `core/db/models.rs:292` |
| L2 意图 | `IntentRecord`（examples/workflow/allowed_tools/default_agent/confidence_threshold/fallback_intent/path/auto_escalate/system_prompt_appendix/model_id/max_iterations） | `core/db/models.rs:313` |
| 路由 | embedding 索引 + 规则打分 + LLM 拆解 + 只读探针 → `Decomposition`（0..n 意图） | `llm/intent/router.rs:137`、`classifier.rs`、`probe.rs` |
| 投影 | `IntentPolicy` → `ExecutionPolicy`（多意图聚合，deep 优先） | `llm/intent/mod.rs:226` |
| 落计划 | `assemble_plan`：一意图一步；deep 交 planner 展开 | `app/commands/intent_plan.rs:77` |
| 生效 | 工具白名单 = `session ∩ intent.allowed_tools`（`intersect_tools`） | `router.rs:125`、`session.rs:1086` |
| 执行强制 | `registry.tool_runtime(allowed_tools)` | `agent/fast_loop.rs:108`、`planner.rs:325` |

结论：**「预期管理」= 意图路由 + 一份扁平工具名白名单 + 一段提示词追加文本**。

本轮要补的三个维度：

- **工具（tools）**：有白名单且在运行时强制，但只是扁平名字；无分组、无 server 粒度、无动态可见性、无「模型猜工具名直接调用」的二次校验。
- **MCP**：`McpServerManager` 在启动/连接时把 server 工具全量注册进全局注册表（`app/state.rs:133`），没有 server 级收窄，也没有按需连接。
- **提示词（prompts）**：只有单个 `system_prompt_appendix: String` 拼接（`session.rs:685`），没有片段、顺序、冲突与预算概念。

因此扩展方向不是"再加几个字段"，而是**把领域/意图从「提示词模板」升格为「作用域（Scope）」，并让裁剪结果成为一等可观测对象**。

> 延期项：**skills**。当前 skills 只是外部 CLI 管理的安装产物（`features/skills/*`），应用侧既不注入清单也不读正文；要收窄技能，必须先做「清单注入 + 按需读正文」，留到后续 change。

## 2. 目标模型：一次请求产出一个 `TurnScope`

```
IntentDomain ── 静态默认 scope
     │
     └── Intent ── 静态配方 scope
              │
RouteContext ──┼──> IntentRouter.decompose() ──> Decomposition(0..n intent + scope_draft)
Session ──────┘                                         │
                                                        ▼
                                              ScopeResolver（纯函数）
                                                        │
                                              ResolvedScope（不可变、可序列化）
                                                   ├─ tools   : builtin + mcp:<server> 子集
                                                   ├─ prompts : 有序片段 + 变量
                                                   ├─ memory  : 召回域与上限
                                                   ├─ agent   : model / iterations / path（已有）
                                                   └─ budget  : token / 数量上限
                                                        │
                            ┌───────────────────────────┼──────────────────────┐
                        PromptComposer            ToolRegistry 视图       确认卡 / Trace
```

三条不可谈判的约束：

1. **deny-by-default，但必须可回退**：裁剪是"可见性视图"，不是权限系统；安全红线工具永远在 scope 内（安全策略胜出）。任务因收窄失败时，允许一次性放宽并在 trace 里留痕，不能静默失败。
2. **LLM 只做选择题**：路由模型只能从候选资源 id 里挑，不做权限判定。任何 LLM 建议都要过 `ScopeResolver` 与服务端策略取交集，异常/超时退回静态 scope。
3. **所有裁剪可解释**：`ResolvedScope` 带 decision 来源（session / domain / intent / llm-draft / safety），进 `RouteTrace`，可回放。

## 3. 分阶段计划

### Phase 0 — 资源目录（前提，没有它一切收窄都无从谈起）

- 新增进程级只读 `ResourceCatalog` 快照，统一暴露可裁剪对象：
  - builtin tools（来自 `ToolRegistry`）
  - MCP tools（按 `server` 分组——注册表当前是扁平的，需要补分组元数据）
  - prompt fragments（内置片段库，见 Phase 4）
  - memory scopes / project roots
- 每个资源统一字段：稳定 id、`source`（`builtin` / `mcp:<server>`）、tags、粗估 token 成本、可用性状态（已连接/未连接）。
- 复用现有 `mark_dirty` 失效模式：目录变更 → 索引重建，scope 缓存同步失效。
- 交付物：`ResourceCatalog` + 单测（分组、去重、缺失元数据降级）。

### Phase 1 — Scope 数据模型

- 给意图增加版本化 scope 列（`scope_json`，`IntentScopeV1`），领域增加 `default_scope_json`；**不拆多表**，避免与现有 intent 读写路径重复事务。
- 维度与语义：
  - `tools`: `mode`(allow/deny) + 选择器（精确名、`mcp:<server>/*`、`builtin:fs.*`）
  - `mcp_servers`: allow + 每 server 的 tool 子集 + 是否允许 on-demand 连接
  - `prompts`: 片段 id 有序列表 + 变量（覆盖旧 `system_prompt_appendix` 单串）
  - `memory`: 召回域、条数上限
  - `budget`: `max_tools` / `max_context_tokens`
- 迁移策略：`allowed_tools` 非空 → 自动映射为 `tools.mode=allow`；scope 为空 = 不限制（与今天行为完全一致）。读取时 scope 优先，写回时双写一段时间保持旧版本可读。
- 预留：`skills` 字段在 schema 中留空位（`resources.skills` 恒为 `None`），避免下轮再加 schema version。
- 交付物：`IntentScopeV1` 定义 + 迁移 + 兼容读取测试（旧库升级后行为不变）。

### Phase 2 — 路由器两级化，输出 scope 而非只有意图

- **两级路由**：先选领域（只给领域摘要，文本量小），再在领域内给"资源组摘要"让模型挑。一次性把上千资源丢给 LLM 既贵又准不了。
- `Decomposition` / `DecomposedIntent` 增加 `scope_draft`：模型可建议收窄（返回 id 列表），不返回自然语言规则。
- 拆解 prompt 的输入从"意图列表"扩为"意图列表 + 该领域可用资源摘要（id + 一行描述 + token 成本）"。
- 缓存：按 `(domain_id, intent_ids, catalog_version)` 缓存解析结果；预算超时直接走静态 scope，**不允许路由阻塞**（沿用 `routing_budget_ms`）。
- `RouteTrace` / `TraceIntent` 增加 scope 摘要（资源数、被裁掉的资源数、decision 来源），保持只读调试语义。
- 交付物：两级路由 + scope_draft + 缓存 + trace 扩展；回放测试保证既有 case 仍命中同一意图。

### Phase 3 — ScopeResolver（纯函数、可测、可回放）

输入：session scope、domain default、多个 intent scope、llm draft、全局安全策略。
规则（明确写死，避免"看情况"）：

1. 同级 `deny` 覆盖 `allow`；
2. 跨级取交集（session ⊂ domain ⊂ intent 可逐级收缩，但 intent 不得突破 session 白名单）；
3. 空 `allow` 列表 = 该类别无资源（不是"不限制"）——沿用 `intersect_tools` 现有语义；
4. 多意图聚合：先并集，再用 session 与安全策略整体裁剪；
5. 安全策略（内置只读/文件系统强制项）不可被任何意图 deny；
6. 结果超 `budget` 时按资源权重/成本裁剪，并记录 `truncated: true`。

交付物：`intersect_tools`（`router.rs:125`）改为 Resolver 的特例调用，行为不变；组合规则单测矩阵（allow/deny/空/多意图/安全覆盖/预算截断/迁移）。

### Phase 4 — 执行侧消费

- **PromptComposer**：base → 用户自定义 → domain → intent → 运行时补充，按序拼装；同 key 片段重复或互斥时报错并在 trace 标注；超预算按权重裁剪。替换 `session.rs:633-690` 里的字符串拼接。
- **MCP 按需**：注册表不再"连接即全量暴露"；按 `ResolvedScope` 决定连接与可见工具，未命中的 server 延迟连接（扩展 `startup_policy` 增加 `on_demand`）。
- **运行时二次校验**：`tool_runtime(allowed_tools)` 已有强制，补齐 MCP 工具的动态可见性与调用期校验（防模型猜测工具名直接调用）。
- **确认卡**：`intent_plan` 展示"本次可用：N 工具 / K 个 MCP server / 片段列表"，并允许用户逐步开关；开关只写 step 级 scope，不回写意图库。

### Phase 5 — 反馈与运维（可与 Phase 4 并行）

- 采集 `(intent, resolved_scope, 实际调用资源, 失败/纠正)`，离线给出收缩建议（N 次未使用 → 建议移出）。
- 指标：上下文 token 节省、工具误选率、scope 解析耗时、fallback 率、收窄导致的重试率。
- 调试面板展示 `ResolvedScope`，与现有环形 trace 对齐。

## 4. 文件影响面（预估）

| 改动 | 文件 |
|------|------|
| scope 模型与迁移 | `core/db/models.rs`、`core/db/intent_repo.rs` |
| 路由与投影 | `llm/intent/mod.rs`、`router.rs`、`classifier.rs`、`index.rs` |
| Resolver | 新增 `llm/intent/scope.rs`（或 `core/scope/`） |
| 计划与确认卡 | `app/commands/intent_plan.rs`、前端确认卡组件 |
| 提示词组装 | `app/commands/session.rs`（609-770 段）、新增 prompt fragment 库 |
| 工具可见性 | `agent/tools/registry.rs`、`agent/tools/policy.rs`、`agent/fast_loop.rs`、`agent/orchestrator/planner.rs` |
| MCP 懒连接 | `features/mcp/manager.rs`、`features/mcp/config.rs` |

## 5. 风险与对策

| 风险 | 对策 |
|------|------|
| 收窄过度导致任务失败 | 失败即放宽路径：工具未命中 → 提示 + 一次性扩展 scope + trace 留痕 |
| 路由延迟上升 | 两级路由 + 资源摘要限长 + 缓存 + 严格 wall-clock 预算，超时走静态 scope |
| MCP server 未连接就被 prompt 提及 | 目录里标注 `connected` 状态，未连接资源不进入候选摘要 |
| 迁移破坏旧库 | scope 为空 = 不限制；`allowed_tools` 单向映射；升级后行为等价测试 |
| 与安全边界混淆 | 明确写进文档：scope 是性能/准确性问题，安全白名单永远叠加且胜出 |

## 6. 非目标

- 不做多租户/权限系统；scope 不承担授权语义。
- 不重写 agent 框架、不改 Rig 调用方式与 streaming 通路。
- 不允许用户脚本自定义 resolver 逻辑（只允许声明式 scope）。
- **skills 收窄不在本轮**（清单注入 + 按需读正文留待后续 change）。
- 不在本计划内做 skills 市场/安装链路改造。

## 7. 建议的 change 拆分（每个可独立闭环）

1. **change A（前提）**：ResourceCatalog + `IntentScopeV1` + 迁移 + ScopeResolver。纯后端、无 UI，靠组合规则单测与迁移等价测试验收。
2. **change B（路由）**：两级路由 + scope_draft + 缓存 + trace 扩展 + 确认卡只读展示。
3. **change C（消费）**：PromptComposer 片段化 + MCP on-demand + 运行时二次校验 + 确认卡可编辑。
4. **change D（可选，延后）**：反馈采集与收缩建议、指标面板；skills 支持在此之后单独排。

依赖：A → B → C；D 依赖 B/C 的 trace 数据。A 与 D 无 UI 风险，适合先行验证。

## 8. 验收口径

- **Resolver 单测矩阵**：6 条组合规则全覆盖，含安全覆盖与预算截断。
- **路由回放**：用现有 `RouteTrace` 历史样本做 golden test，收窄后意图判定不变。
- **端到端**：构造"单意图 × 大量 builtin 工具 × 3 个 MCP server"的项目，对比注入上下文 token 与任务成功率；要求 token 显著下降且成功率不降。
- **降级**：路由超时、catalog 不可用、LLM 返回非法 id 三种情况均退回静态 scope 且消息仍被正常回答。
