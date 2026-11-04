# 意图管理 — 设计

日期：2026-11-04
状态：待评审
关联：`docs/design/2026-11-04-src-tauri-restructure.md`（分层前提）

---

## 1. 意图（为什么做）

当前意图是**硬编码在 Rust 里的 5 个常量**（`chat / code / research / auto / deep_think`），配置藏在 `AppConfig.intent_routing` 的 `HashMap` 里。这带来三个实际问题：

1. **不可管理**。想加一个意图、改一段描述、调整工具白名单，都要改代码重新编译；没有任何界面或命令。
2. **分类器与配置脱节**。`CLASSIFIER_PROMPT` 硬编码了 4 个类别，`deep_think` 在配置里存在却永远不可能被选中；分类器还只读 `OPENAI_API_KEY` 环境变量，取不到就整条 LLM 路径跳过、退回中英文关键词匹配。日常使用中意图判断基本等于关键词猜。
3. **无法承载领域**。程序开发、写作、运维的意图集合差别很大，现在只有一套全局常量。

目标是把「意图」变成**可管理的业务数据**：按领域组织、可增删改查、可导入导出、可切换当前工作集。运行时如何消费这些意图，本次只做**最低限度对接**（保持现有路由能力不退化），把它真正用来驱动 workflow 与子 agent 留到后续。

## 2. 目标 / 非目标 / 反目标

**目标**
- 领域（domain）与意图（intent）落 SQLite，可持久化、可编辑。
- 内置一套「程序开发」领域模板，首次启动落库；用户可在此基础上改。
- 同一时刻恰好一个领域处于活跃状态；切换即时生效。
- 现有 `IntentRouter` 改为读新库，`AppConfig.intent_routing` 删除。
- 提供完整 IPC 与前端管理页面。
- 支持导入/导出 §6.3 定义的 JSON 数组格式，便于备份、手工编辑与跨领域搬运。

**非目标**
- 不驱动 `workflow` / `default_agent`：这两个字段本次只存不用，界面可见但标注「后续接入」。
- 不做按项目绑定领域：`scope` 字段只存不用，全局单活跃。
- 不做内置模板的升级/迁移机制：`version` 是整数，仅作将来迁移的抓手。
- 不做基于 embedding 的分类；不引入向量检索。
- 不改 `ToolLoop`、orchestrator、工具注册表的行为。

**反目标**（即使功能"能跑"也算失败）
- 出现第二份真相：任何地方仍然读 `intent_routing`。
- 界面允许出现"零个活跃领域"，导致路由无处可去。
- 编辑意图后，内置领域的路由结果在用户无感的情况下大幅漂移。

## 3. 约束

- **分层**：`features/intent_management/`（业务）→ `core/db`（模型与仓储）；IPC 在 `app/commands/intent.rs`。模型类型放 `core/db/models.rs`，与 `PersonaRecord` / `MemoryRecord` 同处；不得让 `features/` 依赖 `app/`。
- **IPC 契约**：命令名与参数一经落地即契约，前端零散改动的成本由我们承担；不自造新事件。
- **不新增依赖**：用 `serde_json` 处理 JSON 列，用现有 `rusqlite`。
- **TDD**：每个任务先写失败测试。仓储层与回退分类器必须有单测。
- **中文回复、英文代码注释**。

## 4. 方案

三条存储路线，已选定 A。

**A. SQLite 两张表 + 内置种子（选定）**
`intent_domains` + `intents`，首次启动写入内置领域。与项目里 persona / memory / skill 的既有做法一致（都是 SQLite + seeds），领域切换是一条 `UPDATE`，将来运行时接入是一次索引查询。
代价：要写建表、仓储、种子代码，比读一个文件重。

**B. 单个 JSON 配置文件**
`intents.json` 放应用数据目录，启动读、改完写回，`AppConfig` 只留路径。
代价：无查询能力，切换要整份重写，和"用户内容存 SQLite"的惯例不一致，并发写要自己做原子性。放弃。

**C. 塞进 `AppConfig`**
这正是本次要删掉的东西，加字段只是把债做大。放弃。

## 5. 数据模型

### 5.1 `intent_domains`

| 列 | 类型 | 说明 |
|---|---|---|
| `id` | TEXT PK | 内置用 `builtin.<domain_slug>`（如 `builtin.software_dev`），用户新建用 `uuid` |
| `name` | TEXT NOT NULL UNIQUE | 显示名，如「程序开发」 |
| `description` | TEXT NOT NULL DEFAULT '' | |
| `is_active` | INTEGER NOT NULL DEFAULT 0 | 全局恰好一行为 1 |
| `source` | TEXT NOT NULL DEFAULT 'user' | `builtin` / `user` |
| `version` | INTEGER NOT NULL DEFAULT 1 | |
| `created_at` / `updated_at` | TEXT NOT NULL | RFC3339，沿用 `PersonaRecord` 写法 |

### 5.2 `intents`

| 列 | 类型 | 说明 |
|---|---|---|
| `id` | TEXT PK | 内置用 `builtin.<domain_slug>.<slug>`（如 `builtin.software_dev.plan`），用户新建用 `uuid` |
| `domain_id` | TEXT NOT NULL REFERENCES intent_domains(id) ON DELETE CASCADE | |
| `name` | TEXT NOT NULL | 显示名，如「写计划」 |
| `description` | TEXT NOT NULL DEFAULT '' | 分类器主要依据 |
| `examples` | TEXT NOT NULL DEFAULT '[]' | JSON 字符串数组 |
| `workflow` | TEXT NOT NULL DEFAULT '' | 存而不用 |
| `allowed_tools` | TEXT NOT NULL DEFAULT '[]' | JSON 字符串数组，喂给 `resolve_tools` |
| `default_agent` | TEXT NOT NULL DEFAULT '' | 存而不用 |
| `scope` | TEXT NOT NULL DEFAULT 'global' | 存而不用 |
| `enabled` | INTEGER NOT NULL DEFAULT 1 | 停用的意图不参与分类 |
| `version` | INTEGER NOT NULL DEFAULT 1 | 每次更新 +1 |
| `confidence_threshold` | REAL NOT NULL DEFAULT 0.7 | 低于阈值走 `fallback_intent` |
| `fallback_intent` | TEXT NOT NULL DEFAULT '' | 意图 id；空则落到 `clarify` |
| `path` | TEXT NOT NULL DEFAULT 'fast' | `fast` / `deep`，对应现有 `PathSelection` |
| `auto_escalate` | INTEGER NOT NULL DEFAULT 0 | 对应现有 `IntentConfig.auto_escalate` |
| `system_prompt_appendix` | TEXT NOT NULL DEFAULT '' | 对应现有字段 |
| `model_id` | TEXT NOT NULL DEFAULT '' | 空 = 用会话模型 |
| `max_iterations` | INTEGER NULL | NULL = 用 `ToolLoop` 默认 |
| `sort_order` | INTEGER NOT NULL DEFAULT 0 | 稳定展示顺序 |
| `created_at` / `updated_at` | TEXT NOT NULL | |

约束：`UNIQUE(domain_id, name)`。

**外键不会生效，别去打开它。** `Database::init_tables` 第一句就是 `PRAGMA foreign_keys = OFF`，所以全新库上 `ON DELETE CASCADE` 只是一句装饰。**不要为此全局打开外键**——那会连带改变既有行为：`personas` 没有 CASCADE，`delete_persona` 会从"留下悬挂引用"变成直接报错（`PersonaManager::delete` 没有前置清理）；`delete_session` 会开始级联清理过去被孤立的 `execution_plans`。删除领域时由仓储层显式先 `DELETE FROM intents WHERE domain_id = ?`，再删领域，并在测试里断言子行确实消失。

`path` / `auto_escalate` / `system_prompt_appendix` / `model_id` / `max_iterations` 是**运行时要用的内部字段**，不在用户给的模板 JSON 里，但为了让现有路由能力不退化必须存在。导入时缺省用默认值；导出时一并带上，保证往返无损。

### 5.3 列表为什么用 JSON 列而不是关联表

`examples` 与 `allowed_tools` 是小规模、整体读写的字符串列表，不会出现"按工具反查意图"的需求。用 `TEXT` 存 JSON 减少两张关联表与相应的 CRUD。放弃的：SQL 层无法按元素过滤——当前不需要。

## 6. 运行时对接（本次范围的最小集）

### 6.1 `IntentRouter` 重构

- 构造方式从 `IntentRouter::new(&IntentRouterConfig)` 改为 `IntentRouter::new(db: Arc<Mutex<Database>>, providers: Arc<Mutex<ProviderRegistry>>)`。
- `classify()` 每次从库里读当前活跃领域的启用意图（几十行、本地 SQLite，一次索引查询，不做缓存，避免失效逻辑出错）。
- 分类提示词**由数据生成**：把每个意图的 id、name、description、examples 拼进提示词。这直接修掉「`deep_think` 永远选不中」的缺陷。
- LLM 调用改走 `ProviderRegistry`（优先 `classifier_model_id`，否则默认模型），不再只认 `OPENAI_API_KEY`；取不到 provider 时直接走回退，不发请求。
- `ClassificationResult` 增加 `confidence: f32`。`confidence < 该意图的 confidence_threshold` 时改走 `fallback_intent`（空则该领域的 `clarify`，再空则取 `sort_order` 最小的启用意图）。
- 回退路径（无 provider / LLM 失败）：用 `examples` 做子串与词元重合度打分，归一化后与阈值比较，规则与 LLM 路径一致。修掉现在「中英文关键词表硬编码」的问题——关键词表由数据提供。
- `IntentResult` 的 `name` 改为意图 id（日志与事件里出现的是稳定标识，不再随显示名变化）。
- `resolve_tools` 行为不变。
- **无活跃领域 / 零启用意图时**：`classify()` 不发 LLM 请求，直接返回一个 fast 路径、无工具限制、`max_iterations` 取默认值的结果，并 `log::warn!` 记一次。宁可退化成普通对话，也不能让消息发不出去。
- **种子必须早于 router 构造**：内置领域要在 `AppState::new` 构造 `IntentRouter` 之前就写进库——放在 `Database::new()` 之后同步执行，不能留在 `lib.rs` 的异步启动任务里。否则首条消息可能读到空库，正好落进上面那条退化分支。

### 6.2 删除 `intent_routing`

- 删 `AppConfig.intent_routing` 与 `IntentRouterConfig` / `default_intents()`。
- **保留两个应用级旋钮**，移进 `AppConfig` 的新结构：

  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct IntentAppConfig {
      pub enabled: bool,                       // 默认 true
      pub classifier_model_id: Option<String>, // 默认 None = 用默认模型
  }
  ```

  - `enabled` 是「关闭意图路由、全部按 chat 处理」的总开关。删掉它是功能退化，不是清理。
  - `classifier_model_id` 决定分类用哪个模型。
  - 逐意图的配置全在库里，配置里只剩这两个应用级开关，不构成第二份真相。
- `AppConfig` 每个字段都有 `#[serde(default)]` 且未设 `deny_unknown_fields`，旧配置文件里的 `intent_routing` 键会被静默忽略，不需要迁移代码。
- 旧配置里的用户自定义意图**不迁移**（数量预计为零；若存在，用户可在界面重建或导入）。

### 6.3 内置「程序开发」领域

内置领域 id 为 `builtin.software_dev`，意图 id 为 `builtin.software_dev.<slug>`。用户新建的领域与意图一律用 uuid，前缀只属于内置数据。

下面这 12 条是**我重新设计的种子**，不是照搬示例：`allowed_tools` 只写工具注册表里真实存在的名字（`file_system` / `code_executor` / `web_search` / `calculator` / `run_workflow`），`default_agent` 只写真实 worker kind（`thinker` / `code_explorer` / `code_editor` / `shell` / `web` / `memory` / `mcp_bridge`），这样将来接 `resolve_tools` 与 dispatcher 时不需要再对一遍名字。

设计原则：描述写「与邻居的区别」而不是泛泛的愿望，因为分类提示词就是用描述和示例生成的；阈值在错分代价高的位置调高（`plan` 与 `implement` 混了最贵），回退目标在本领域内，指向邻居而不是永远指 `clarify`。

```json
[
  { "id": "builtin.software_dev.plan", "name": "规划方案",
    "description": "要方案、拆解或步骤，且明确不是现在改代码。与 spec 的区别：产出是口头/对话形式的计划，不是可存档的文档。",
    "examples": ["先别动代码，帮我把这个需求拆一下", "这个功能大概怎么落地", "给我一个实施计划", "分几步做比较好", "先把思路理一下再动手"],
    "workflow": "planning", "allowed_tools": ["file_system"],
    "default_agent": "thinker", "scope": "global", "enabled": true, "version": 1,
    "path": "deep", "auto_escalate": 1,
    "confidence_threshold": 0.75, "fallback_intent": "builtin.software_dev.spec" },

  { "id": "builtin.software_dev.spec", "name": "编写规格",
    "description": "要一份可存档的需求或技术文档。与 plan 的区别：成果是文档，要落盘。",
    "examples": ["写一份登录模块的规格", "把这个需求整理成设计文档", "补一份技术方案", "生成需求文档存到 docs 下", "把刚才讨论的写成 spec"],
    "workflow": "spec_generation", "allowed_tools": ["file_system"],
    "default_agent": "thinker", "scope": "global", "enabled": true, "version": 1,
    "path": "deep", "auto_escalate": 1,
    "confidence_threshold": 0.75, "fallback_intent": "builtin.software_dev.plan" },

  { "id": "builtin.software_dev.implement", "name": "实现功能",
    "description": "明确要新增或修改代码行为。没有失败现象、也不是纯结构调整时选它。",
    "examples": ["加一个登出接口", "把这段改成异步", "实现一下这个需求", "给设置页加个开关", "按这个设计写代码"],
    "workflow": "code_change", "allowed_tools": ["file_system", "code_executor"],
    "default_agent": "code_editor", "scope": "global", "enabled": true, "version": 1,
    "path": "deep", "auto_escalate": 1,
    "confidence_threshold": 0.7, "fallback_intent": "builtin.software_dev.plan" },

  { "id": "builtin.software_dev.debug", "name": "排查修复",
    "description": "有具体失败现象——报错、测试挂、行为不对——要先定位根因再修。与 implement 的区别：先有症状。",
    "examples": ["这个接口 500 了", "为什么这条测试过不了", "登录之后页面白屏", "这里偶尔 panic", "同样的输入两次结果不一样"],
    "workflow": "bug_fix", "allowed_tools": ["file_system", "code_executor"],
    "default_agent": "code_explorer", "scope": "global", "enabled": true, "version": 1,
    "path": "deep", "auto_escalate": 1,
    "confidence_threshold": 0.75, "fallback_intent": "builtin.software_dev.implement" },

  { "id": "builtin.software_dev.test", "name": "测试相关",
    "description": "围绕测试本身：补用例、修用例、跑用例、看覆盖。改的是测试而不是被测代码时选它。",
    "examples": ["给这个函数补几个单测", "跑一下测试看看", "这个用例为什么挂了", "补一下边界情况", "覆盖率现在多少"],
    "workflow": "testing", "allowed_tools": ["file_system", "code_executor"],
    "default_agent": "code_editor", "scope": "global", "enabled": true, "version": 1,
    "path": "deep", "auto_escalate": 1,
    "confidence_threshold": 0.7, "fallback_intent": "builtin.software_dev.implement" },

  { "id": "builtin.software_dev.review", "name": "审查改动",
    "description": "对已有改动或代码做质量判断，本身不改代码。要有对象——一个 diff、一个 PR、一段实现。",
    "examples": ["帮我 review 一下这次改动", "这段代码有什么问题", "这个 PR 能合吗", "看看有没有隐患", "这么写符合项目规范吗"],
    "workflow": "review", "allowed_tools": ["file_system"],
    "default_agent": "code_explorer", "scope": "global", "enabled": true, "version": 1,
    "path": "deep", "auto_escalate": 1,
    "confidence_threshold": 0.75, "fallback_intent": "builtin.software_dev.explain" },

  { "id": "builtin.software_dev.refactor", "name": "重构",
    "description": "改结构不改外部行为。明确说了「行为不变」「整理结构」「去重」时选它。",
    "examples": ["把这个模块拆一下", "提取公共逻辑", "这段太长了，整理一下", "去掉重复代码", "换个更清楚的组织方式"],
    "workflow": "refactor", "allowed_tools": ["file_system", "code_executor"],
    "default_agent": "code_editor", "scope": "global", "enabled": true, "version": 1,
    "path": "deep", "auto_escalate": 1,
    "confidence_threshold": 0.7, "fallback_intent": "builtin.software_dev.implement" },

  { "id": "builtin.software_dev.explain", "name": "解释代码",
    "description": "要理解现有实现或报错的含义，不要改动。与 lookup 的区别：要的是「为什么」而不是「在哪」。",
    "examples": ["这段代码在干什么", "为什么这里要加锁", "这个报错什么意思", "解释一下这个函数", "这个设计为什么这么做"],
    "workflow": "explain", "allowed_tools": ["file_system"],
    "default_agent": "thinker", "scope": "global", "enabled": true, "version": 1,
    "path": "fast", "auto_escalate": 0,
    "confidence_threshold": 0.65, "fallback_intent": "builtin.software_dev.lookup" },

  { "id": "builtin.software_dev.lookup", "name": "查询项目",
    "description": "查事实：项目约定、文件位置、配置项、历史写法。要的是「在哪」「是什么」，不是「为什么」。",
    "examples": ["项目里怎么跑测试", "AGENTS.md 怎么规定的", "登录相关代码在哪", "之前有没有类似实现", "这个配置项在哪定义"],
    "workflow": "knowledge_query", "allowed_tools": ["file_system", "web_search"],
    "default_agent": "code_explorer", "scope": "global", "enabled": true, "version": 1,
    "path": "fast", "auto_escalate": 0,
    "confidence_threshold": 0.65, "fallback_intent": "builtin.software_dev.explain" },

  { "id": "builtin.software_dev.execute", "name": "执行命令",
    "description": "直接要跑命令、构建、装依赖、起服务。诉求是「执行」而不是「改代码」。",
    "examples": ["跑一下 build", "装一下依赖", "启动开发服务器", "执行这个脚本", "跑一下 lint"],
    "workflow": "tool_execute", "allowed_tools": ["code_executor", "file_system"],
    "default_agent": "shell", "scope": "global", "enabled": true, "version": 1,
    "path": "fast", "auto_escalate": 0,
    "confidence_threshold": 0.7, "fallback_intent": "builtin.software_dev.implement" },

  { "id": "builtin.software_dev.clarify", "name": "需要澄清",
    "description": "信息不足或指代不明，无法安全路由；正确反应是先反问，而不是猜着做。",
    "examples": ["帮我弄一下登录", "改一下", "看看这个", "这个有问题", "帮我处理一下"],
    "workflow": "clarify", "allowed_tools": [],
    "default_agent": "", "scope": "global", "enabled": true, "version": 1,
    "path": "fast", "auto_escalate": 0,
    "confidence_threshold": 0.6, "fallback_intent": "" },

  { "id": "builtin.software_dev.chat", "name": "闲聊对话",
    "description": "寒暄、感谢、与项目无关的日常对话。不需要工具，也不应触发任何工程动作。",
    "examples": ["你好", "谢谢", "今天有点累", "在吗", "随便聊聊"],
    "workflow": "chat", "allowed_tools": [],
    "default_agent": "", "scope": "global", "enabled": true, "version": 1,
    "path": "fast", "auto_escalate": 0,
    "confidence_threshold": 0.6, "fallback_intent": "" }
]
```

**`path` 与旧行为的差异（有意为之）**：`explain` / `lookup` / `execute` 走 fast，而旧的 `research` 走 deep。回答问题和查资料不该付编排成本。`code` 被拆成 `implement` / `debug` / `refactor` / `test` 四条，全部仍是 deep。

**`fallback_intent` 的语义**：存意图 **id**；空字符串表示终结，不再跳转（`clarify` 与 `chat` 用它防止自我循环）。

**`auto_escalate` 与旧配置对齐**：计划类与改动类为 1，问答类为 0，与旧 `chat: false / 其余 true` 的意图一致。

## 7. IPC 命令

全部在 `app/commands/intent.rs`，返回值遵循 `persona.rs` 的既有约定（`Result<T>`，DTO 用 `#[derive(serde::Serialize)]`）。

**领域**
- `list_intent_domains() -> Vec<IntentDomainInfo>`
- `get_active_intent_domain() -> Option<IntentDomainInfo>` — 返回 `Option`：启动瞬间或数据被外部破坏时可能没有活跃领域，前端要能表达这个状态。
- `create_intent_domain(name, description) -> IntentDomainInfo`
- `update_intent_domain(id, name, description) -> IntentDomainInfo` — 重名返回错误。
- `delete_intent_domain(id) -> ()` — 拒绝删除活跃领域；内置领域允许删除。
- `set_active_intent_domain(id) -> ()` — 事务内清零再置一；目标领域若零启用意图，拒绝并返回明确错误。

**意图**
- `list_intents(domain_id) -> Vec<IntentInfo>`
- `get_intent(id) -> IntentInfo`
- `create_intent(domain_id, payload) -> IntentInfo`
- `update_intent(id, payload) -> IntentInfo`
- `delete_intent(id) -> ()` — 拒绝删除领域内最后一个启用的意图。
- `set_intent_enabled(id, enabled) -> ()` — 拒绝停用领域内最后一个启用的意图。

**「至少一个启用意图」的守卫位置**：不是只挂在停用上。下面每条都能把领域变成零启用意图，必须各自拒绝并返回同一种错误：

| 入口 | 条件 |
|---|---|
| `set_intent_enabled(false)` | 该意图是领域内唯一启用的 |
| `delete_intent` | 该意图是领域内唯一启用的 |
| `set_active_intent_domain` | 目标领域零启用意图 |
| `import_intent_domain` | 导入结果零启用意图（或数组为空） |
| `create_intent_domain` | 允许建成空领域；它不活跃，不触发不变量 |

`classify()` 侧仍要按 §6.1 兜底——守卫是契约，兜底是保险。

**导入导出的 id 语义**（往返测试按这个写）
- **导入忽略来件的 `id`**，为领域与每个意图重新生成 uuid。理由：`intents.id` 全局主键，若保留来件 id，把某个领域的导出再导入一次必然撞主键，整批事务失败。
- **重名领域不报错**，自动加后缀 `名称 (2)`、`名称 (3)`，与既有项目的命名习惯一致。
- **导出保留库里的真实 id**，只在库内往返，不做跨库交换。
- **往返测试只比较业务字段**（name / description / examples / allowed_tools / workflow / default_agent / scope / enabled / confidence_threshold / path / system_prompt_appendix / model_id / max_iterations），**不比较 id**；`fallback_intent` 比较时按"指向同一个意图的相对位置"解析，因为 id 变了。

- `import_intent_domain(json: String, name: String) -> IntentDomainInfo` — 接受 §6.3 那种数组格式；整体在一个事务里，任一条不合法则整批失败，错误信息带上第几条。
- `export_intent_domain(domain_id) -> String` — 输出同样的数组格式，包含内部字段，便于备份与手工编辑。
- `reset_intent_domain_to_builtin(id) -> ()` — 仅对 `source = 'builtin'` 的领域可用。**留给实现者决定是否需要**；若实现，须二次确认。

## 8. 前端

**已定：顶层模块视图，与虚拟人管理并列。**

先纠正一处事实：`PersonaManagerPage` 不在 `components/settings/`，它在 `components/management/`，是 `App.tsx` + `ModuleBar` 的顶层模块视图。`SettingsPage.tsx` 只有 `providers / prompts / runtime / conversation` 四个 tab，没有"命令导航"。意图管理跟虚拟人、技能、记忆是同一类东西——管理一套可切换的内容，不是改设置——所以走同一种入口。

要动的文件：新增 `components/management/IntentManagerPage.tsx`；`ModuleBar.tsx` 加一项；`uiSlice` 的 `AppView` 加一个枚举值；`App.tsx` 加渲染分支与顶栏标题映射。

页面结构：

- 左栏领域列表：名称、活跃标记、切换按钮、新建/删除。
- 右栏活跃领域的意图列表：按 `sort_order` 排，显示名称、path 徽标、启用开关；点击进入编辑。
- 编辑面板：id（只读）、名称、描述、示例（多行，一行一条）、工具白名单、path、阈值、兜底意图、`system_prompt_appendix`；`workflow` / `default_agent` / `scope` 显示但标注「后续接入」并禁用。
- 顶部：导入（粘贴 JSON）/ 导出（复制 JSON）。
- 停用最后一个意图、删除活跃领域、激活零启用领域时给出明确的前端提示，同时后端仍然拒绝。

## 9. 测试

- **仓储层**：建表幂等、种子只写一次、领域切换的原子性、版本号递增、**删除领域时子意图被显式清理**（外键不生效，不要假设 CASCADE 生效）。
- **不变量守卫**：按 §7 的表逐条断言——唯一启用意图不可停用、不可删除、零启用领域不可激活、导入零启用结果被拒。
- **回退分类器**：给一组中英文样例，验证命中期望意图、置信度低于阈值时落到 `fallback_intent`、`fallback_intent` 为空时终结不再跳转、无匹配时落到 `clarify`。
- **导入导出**：把 §6.3 的 12 条 JSON 导入 → 导出 → 按 §7 的规则比较业务字段（不含 id）；再导入一次同一份 JSON，断言生成的是另一个新领域而不是主键冲突。
- **路由不退化**：构造内置领域后，断言 `chat` 走 fast、`code_change` 走 deep、`allowed_tools` 能过滤会话工具、`max_iterations` 能传下去、无活跃领域时退化成 fast 且不报错。
- 既有 199 个单测与 6 个集成测试必须保持全绿。

## 10. 风险

| 风险 | 处理 |
|---|---|
| 删 `intent_routing` 时漏改调用点 | `cargo check --all-targets` 会直接报错，编译器兜底 |
| 领域切换与意图编辑并发 | 单连接 `Arc<Mutex<Database>>`，天然串行 |
| 内置领域被用户改坏后无法恢复 | `reset_intent_domain_to_builtin`（可选实现）+ `source` 字段保留判断依据 |
| 分类提示词随意图增多而变长 | 当前 12 个意图，可接受；超过约 40 个时应改为两阶段筛选——记入后续 |
| 新旧 path 分配导致路由行为漂移 | 第 6.3 节显式记录，评审时确认 |

## 11. 留给实现者决定

- `IntentManager` 放在 `features/intent_management/mod.rs` 里作为薄服务层，还是让命令层直接操作 `Database`。倾向后者（与 `persona` 不完全一致，但少一层转发）；若仓储接口膨胀再抽。
- `reset_intent_domain_to_builtin` 做或不做。
- 前端编辑用抽屉还是弹窗，沿用 `PersonaManagerPage` 的做法即可。
- 领域色标、图标等纯展示细节。
- `sort_order` 是否暴露为可拖拽排序。
