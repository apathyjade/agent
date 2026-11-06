# 设计：用 Rig 工具链替换项目工具链

> OpenSpec change：`openspec/changes/migrate-toolchain-to-rig/`
> 日期：2026-11-05
> 状态：已核验（2026-11-05），待用户确认

## 1. 背景与问题

### 1.1 现状

项目有两条执行路径，共用一套工具，但工具链是自研的：

```
内置工具 / MCP 工具
      │  实现 crate::agent::tools::base::AgentTool
      │  （name / definition / call(args: String) -> Result<String, ToolError>）
      ▼
crate::agent::tools::registry::ToolRegistry        ← Arc<Mutex<...>>
      │  to_rig_tools(allowed)  手写转换
      ▼
rig_core::tool::DynamicTool
      │
      ├── fast：llm/rig.rs → Rig AgentBuilder → AgentRunner 自动工具循环
      └── deep：orchestrator/runtime/executor.rs → ToolRegistry::execute（自己解析 JSON）
```

Rig 0.46 自带完整的工具体系（`rig_core::tool::Tool`、`DynamicTool`、`ToolSet`、`ToolServerHandle`、
`ToolAnnotations`、`Touches`、`ToolResult`、`ToolExecutionError`、`AgentHook`），项目几乎一个都没用。

### 1.2 缺陷清单

| # | 缺陷 | 证据 | 影响 |
|---|---|---|---|
| D-1 | **工具调用的历史是断的**：模型发起的 `tool_calls` 从不落库，工具结果却以 `role="tool"` 落库 | `app/commands/session.rs:264`、`:339`（`tool_calls: None`）、`:1087`（tool 行） | Rig 在发请求前会把没有对应 assistant 调用的工具结果当孤儿丢弃（`rig-core/src/completion/history.rs:491-494`）。**不报错，但模型下一轮完全看不到自己刚查到的内容**——表现为「忘了刚查过什么、重复查询、凭空作答」 |
| D-2 | **流式出错后前端永久卡在 streaming** | `agent/fast_loop.rs:178-181` 发完错误文本就 `break`，不发 `Done`；前端 `store/index.ts:130` 只在 `chunk.done` 收尾 | 用户看到半截回答 + 一个转不完的圈 |
| D-3 | **最终回答重复一遍** | `agent/fast_loop.rs:185-188` 把 `FinalResponse.output()` 又当文本推一次（此前已推过 Text delta） | 落库与展示的回答是两份 |
| D-4 | **模型参数写错 = 整轮失败** | Rig 默认 `max_invalid_tool_call_retries=0`、`unhandled_invalid_tool_call=Fail`（`rig-agent-0.46.0/src/run/mod.rs:397-399`）；`max_consecutive_malformed_tool_calls` 默认 `None`（无上限） | 一次参数 JSON 笔误就让整轮对话报错，且可能无限循环 |
| D-5 | **MCP 工具的 `is_error` 被丢弃** | `features/mcp/manager.rs:832-852` 只取文本 | MCP 工具失败被当成功回灌给模型，模型据此编造结论 |
| D-6 | **MCP 工具名无前缀，会覆盖并永久删除内置工具** | `manager.rs:441-444`（同名先 `unregister` 再注册）、`:520-523`（断开按名 `unregister`） | 内置工具被 MCP 工具悄悄顶掉，断了 MCP 就永久没了 |
| D-7 | **MCP 的 `confirm_once` 从未实现** | `manager.rs:785` 只判 `Deny`；`ConfirmationMode::ConfirmOnce` 全仓无执行路径 | PRD 承诺的三种确认模式只落地两种 |
| D-8 | **没有工具级超时** | fast 路径无；deep 路径 `timeout_seconds` 明确「目前不生效」（`orchestrator/runtime/executor.rs:190`） | `web_search` / `code_executor` 挂住就一直占着这一轮 |
| D-9 | **并行工具调用被关着** | Rig `tool_concurrency` 默认 1（`rig-agent-0.46.0/src/agent/runner.rs:419`），项目从不设置 | 模型一轮要查 3 个文件时只能串行 |
| D-10 | **调用参数完全不可见** | `ToolCallInfo` 只有 id+name（`agent/fast_loop.rs:36-42`）；前端 `MessageBubble.tsx:144` 读 `parsed.args` 永远是空 | 用户无法审计模型到底传了什么参数 |
| D-11 | **工具清单在前端是空壳** | `registry.rs:177-187` 返回空 `description` + `parameters: null` | 工具管理页看不到任何工具的用途与参数 |
| D-12 | **白名单不解析别名** | `registry.rs:127` 用注册键精确 `contains` | 白名单写 `read_file` 匹配不到 `file_system` |
| D-13 | **deep 与 fast 的工具语义相反** | `executor.rs:358-359`：白名单为 `None`（不限制）时 `tools_enabled = false` | deep 的 AgentTask 在「不限制」时反而拿不到工具 |
| D-14 | **deep 路径把工具返回值当 JSON 解析** | `registry.rs:171` | 返回纯文本的工具在 deep 路径直接报错 |
| D-15 | **`calculator` 描述与实现不符** | `calculator.rs:32` 宣称支持 sqrt/pow/sin/cos/tan/log/abs，实现只有 `+ - * /`（`:144-173`） | 模型照描述调用 `sqrt(4)` 必然失败 |
| D-16 | **全局 `AppConfig::enabled_tools` 不生效** | `app/state.rs:98` 每次 `ToolRegistry::new()` 全量注册；`commands/tool.rs:22` 只用于展示 | 用户在设置里关掉工具，重启后又全开 |
| D-17 | **工具相关代码几乎零测试** | `agent/tools/` 与 `features/mcp/` 无 `#[cfg(test)]`；`fast_loop` 的 `run`/`run_stream` 无测试 | 改工具链没有安全网 |

## 2. 目标与非目标

### 2.1 Goals

- **G1** 工具链换成 Rig 的：`DynamicTool` 是唯一运行时形态，`rig_core::tool::Tool` 是唯一编写形态；
  项目不再自定义工具 trait。
- **G2** fast 与 deep 两条路径**共用同一批工具对象**、同一套参数校验、同一套错误语义。
- **G3** 工具调用可审计：参数、结果、耗时、失败原因都能落到前端与历史里。
- **G4** 高危工具调用有闸门：破坏性 / 开放世界工具执行前要用户点头。
- **G5** 模型参数写错不会整轮崩，且不会无限重试。
- **G6** 一轮里的多个独立工具调用可以并行。

### 2.2 Non-Goals

- 不做「记住授权」——本轮只做**单次确认**，不持久化授权决定。
- 不换掉 Rig 的 agent 运行时：工具循环仍然由 `AgentRunner` 驱动。
- 不引入 `ToolServerHandle` 作为唯一注册表（见 §3.3）。
- 不做工具的版本化、热重载、远程工具市场。
- 不改 `web_search` 的抓取实现、不改 `code_executor` 的沙箱模型（只补超时）。
- 不动记忆、意图路由的分类逻辑。

## 3. 方案选型

### 3.1 候选

| 方案 | 内容 | 代价 |
|---|---|---|
| A | `DynamicTool` 为唯一运行时形态 + 项目保留薄策略层 | 需要写一层转换与策略；MCP 生命周期自己管 |
| B | 完全用 `rig_agent::tool::ToolServerHandle` 做唯一注册表 | 白名单要靠 `AgentHook` 注入 `RequestPatch::active_tools`；deep 路径要碰 Rig 底层 serve API 才能按名调用 |
| C | 只换 MCP 与内置工具的调用协议，注册表与快慢路径都不动 | 等于现状 + 修几个 bug，达不到「全链路统一」 |

### 3.2 选定：A

用户已拍板走 A。理由：

- Rig 的 `ToolServerHandle` 是**注册表**，不负责「这一轮给模型看哪些工具」「工具叫什么别名」「用户关掉了哪个」——
  这三件事项目本来就得自己有一层。A 把这层保留下来但削薄，注册与执行交给 Rig。
- B 的白名单必须绕到 `AgentHook` + `RequestPatch::active_tools`（`rig-agent-0.46.0/src/run/patch.rs:93`），
  而 deep 路径根本没有 Rig agent，两条路又要分叉——与 G2 冲突。
- A 让 fast 与 deep 拿到**完全相同的 `DynamicTool` 对象**：fast 交给 Rig agent 驱动，deep 直接
  `run_with`，语义天然一致。

### 3.3 为什么不用 `ToolServerHandle`

`ToolServerHandle` 提供了运行时增删（`add_dynamic_tool` / `remove_tool`）与 `ManagedToolToken` 生命周期，
对 MCP 有价值。但它只能吐出 `ToolSet`/`ToolCatalog`（定义），**按名调用**要落到
`ErasedHandler::handle(EffectKind::ToolCall{..}, Dispatch)` 这层底层 API（`rig-core/src/serve/handler.rs:302`），
比 `DynamicTool::run_with`（`rig-core/src/tool/contextual.rs:259`）绕一层，且拿结果的形状更啰嗦。
本轮 MCP 工具数量与生命周期都不复杂，A 的注册表足够。

> 后续若 MCP 工具数量增长到需要「按 server 批量退役」，再把 `ToolServerHandle` 吸收进注册表即可，
> 届时 `ToolRegistry` 内部换实现，外部接口不变。

## 4. 目标架构

```
┌─ 编写层（rig_core::tool::Tool，类型化） ────────────────────────────┐
│  CalculatorTool  FileSystemTool  WebSearchTool                     │
│  CodeExecutorTool  RunWorkflowTool        MCP 工具（DynamicTool）   │
│      ↑  #[derive(Deserialize, JsonSchema)] 的 Args                  │
│      ↑  annotations() / footprint()                                │
└───────────────┬────────────────────────────────────────────────────┘
                │ into_dynamic::<T>()   ← agent/tools/convert.rs
                ▼
┌─ 运行时层（rig_core::tool::DynamicTool） ──────────────────────────┐
│  definition(): name / description / parameters / annotations       │
│  run_with(&mut ToolContext, args) -> Result<ToolOutput, _>         │
└───────────────┬────────────────────────────────────────────────────┘
                ▼
┌─ 策略层（crate::agent::tools::registry::ToolRegistry） ────────────┐
│  tools: IndexMap<String, DynamicTool>   （有序）                   │
│  enabled: HashMap<String, bool>                                     │
│  aliases: HashMap<String, String>                                  │
│  catalog()          → 前端工具列表（真实描述 / schema / 风险）      │
│  active(allowed)    → fast 路径交给 Rig agent 的 Vec<DynamicTool>  │
│  execute(name,args) → deep 路径直接调用                            │
└───────────────┬────────────────────────────────────────────────────┘
                ├── fast：llm/rig.rs → AgentBuilder::dynamic_tools + AgentHook
                └── deep：orchestrator/runtime/executor.rs → ToolRegistry::execute
                                  └── 两者共用 agent/tools/approval.rs 的闸门判定
```

### 4.1 工具定义层：类型化 `Tool`

每个内置工具改成 `rig_core::tool::Tool` 实现：

```rust
pub struct CalculatorTool;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CalculatorArgs {
    /// 要计算的算术表达式。
    pub expression: String,
}

impl rig_core::tool::Tool for CalculatorTool {
    const NAME: &'static str = "calculator";
    type Args = CalculatorArgs;
    type Output = serde_json::Value;
    type Error = crate::core::error::AppError;

    fn description(&self) -> String {
        "计算算术表达式。支持 + - * /、括号与一元负号。".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        rig_core::tool::args_schema::<CalculatorArgs>()
    }

    fn annotations(&self) -> rig_core::completion::ToolAnnotations {
        // 只读、不破坏、幂等
        ToolAnnotations::default().with_read_only(true)
    }

    fn call(&self, _ctx: &mut ToolContext, args: CalculatorArgs)
        -> impl Future<Output = Result<Self::Output, Self::Error>> + Send
    {
        async move { /* 现有实现，参数从结构体取 */ }
    }
}
```

要点：

- **参数从 `String` 变成结构体**：serde 反序列化失败由 Rig 统一归一成
  `ToolExecutionError::invalid_args`，模型能收到「参数不对」而不是一段 panic 文本。
- **schema 自动生成**：`args_schema::<A>()`（`rig-core/src/tool/schema.rs:34`）从 `JsonSchema` 派生，
  前端工具页直接拿到真实参数表（修 D-11）。
- **描述必须与实现一致**：`calculator` 的描述改成只宣称 `+ - * /`（修 D-15）；
  `run_workflow` 的描述改成实际行为（当前是「不执行、只返回步骤描述」，
  `run_workflow.rs:3-4`，描述里那四个虚构工作流名删掉）。
- **annotations 必须三个标志都显式声明**：`ToolAnnotations::default()` 的 `destructive()` 与
  `open_world()` **默认都是 true**（`rig-core/src/completion/request.rs:165-178`），
  只写 `read_only` 的工具仍会被判为破坏性。每个工具都要显式写
  `with_read_only(...)` / `with_destructive(...)` / `with_open_world(...)`。
- **annotations 表达风险**：`file_system` 的写/删、`code_executor` 标 `destructive`；
  `calculator` 标 `read_only`；`web_search` 标 `read_only` + `open_world`。

### 4.2 转换层：`Tool` → `DynamicTool`

新增 `agent/tools/convert.rs`：

```rust
/// 把类型化工具转成可存储、可跨路径复用的动态工具。
pub fn into_dynamic<T>(tool: T) -> ToolEntry
where
    T: rig_core::tool::Tool + Clone + 'static,
{
    let definition = rig_core::tool::tool_definition::<T>(&tool);

    // ToolAdapter::new 会吃掉 tool，所以先克隆一份留给 footprint。
    // 内置工具都是无状态的 unit struct，Clone 没有成本。
    let footprint_tool = tool.clone();
    let footprint: FootprintFn = Arc::new(move |args: &serde_json::Value| {
        serde_json::from_value::<T::Args>(args.clone())
            .ok()
            .map(|args| footprint_tool.footprint(&args))
    });

    let adapter = std::sync::Arc::new(rig_core::serve::adapters::ToolAdapter::new(tool));
    let dynamic = rig_core::tool::DynamicTool::new_with_context(
        definition.name.clone(),
        definition.description.clone(),
        definition.parameters.clone(),
        move |ctx: &mut ToolContext, args: serde_json::Value| {
            let adapter = adapter.clone();
            Box::pin(async move { adapter.run_with(ctx, args).await })
        },
    )
    .with_annotations(definition.annotations.clone());

    ToolEntry {
        tool: dynamic,
        footprint: Some(footprint),
    }
}
```

> `T: Clone` 是硬约束：`ToolAdapter::new(tool)` 按值消费工具，而 footprint 闭包还要用同一个工具。
> 若将来出现不可 Clone 的工具，改成 `Arc<T>` + 一个委托 `Tool` 的 newtype，而不是放弃 footprint。

`ToolAdapter::run_with` 的签名已核实：`run_with(&self, &mut ToolContext, serde_json::Value)
-> Result<ToolOutput, ToolExecutionError>`（`rig-core/src/serve/adapters.rs:511`）。

注意：`DynamicTool::new_with_context` 内部用 `ToolDefinition::new(...)`，**不会**带上 annotations，
必须显式 `.with_annotations(...)`——否则 §4.7 的风险判定拿不到信息。
`ToolAdapter::run_with` 内部已经把工具的 `Error` 归一成 `ToolExecutionError`（`map_error`）。

#### footprint 会丢，必须自己带着

`DynamicTool` 的 `Serve` 实现**没有**覆盖 `footprint`，默认返回 `None`
（`rig-core/src/tool/contextual.rs:309` + `rig-core/src/serve/handler.rs:103`），
而 `DispatchEvent` 也不带 footprint（`rig-agent-0.46.0/src/agent/hook.rs:623-635`）。
只有 `ToolAdapter` 的 `Serve::footprint` 会调用 `Tool::footprint`
（`rig-core/src/serve/adapters.rs:558`）——转成 `DynamicTool` 后这条路就断了。

因此转换函数要**同时**产出一个 footprint 解析器：

```rust
/// 由参数算出这次调用碰到的东西；解析不出参数时返回 `None`。
pub type FootprintFn = Arc<dyn Fn(&serde_json::Value) -> Option<Touches> + Send + Sync>;

pub struct ToolEntry {
    /// 可存储、可跨路径复用的动态工具。
    pub tool: DynamicTool,
    /// 该工具的 footprint 解析器（由类型化工具的 `footprint(&args)` 捕获）。
    pub footprint: Option<FootprintFn>,
}
```

**footprint 的真实类型（已核实）**：

- `Tool::footprint(&self, args: &Self::Args) -> Touches`——返回 `Touches`，**不是** `Option<Touches>`；
  参数反序列化失败时由 `FootprintFn` 自己返回 `None`。
- `Touches` 的变体是 `Reads(Scope)` / `Writes(Scope)` / `Independent`
  （`rig-core/src/tool/footprint.rs:27`），**没有** `Scope::Read` / `Scope::Write` 这种写法。
- `Scope` 的变体是 `Paths(Vec<PathBuf>)` / `Anything`（`footprint.rs:51`），路径必须是绝对路径；
  相对路径在 `normalized()` 后会退化成 `Anything`（`footprint.rs:77`）。
- 不重写 `footprint` 时的默认值是 `Touches::from_annotations`：`read_only = true` → `Reads(Anything)`，
  否则 `Writes(Anything)`（`footprint.rs:138`）。

`into_dynamic` 同时返回 `ToolEntry`；注册表存 `ToolEntry`，但**交给 Rig 执行的仍是 `DynamicTool`**
（`ToolEntry::tool`）——执行形态保持唯一，footprint 只是策略层需要的元数据。

#### 错误信息要留给模型

`ToolExecutionError::from_error` 会把模型可见的反馈**脱敏**成固定文案
（`rig-core/src/tool/result.rs:198` 的 `redact_model_feedback()`），
模型就看不到「文件不存在」这类可操作信息（现在反而能看到原文）。
所以工具的 `map_error` 不走 `from_error`，改用
`ToolExecutionError::other(e.to_string())`；`file_system` 的错误里不回显绝对路径。

#### 工具输出怎么被读出来

`ToolOutput` 的 JSON 形态（`ToolOutput::json`）**没有** `as_text()`，
现有 `agent/fast_loop.rs:203-208` 只 join `as_text()` 会得到空串。
统一改用 `ToolOutput::render()`（`rig-core/src/tool/output.rs:138`）取文本。

### 4.3 策略层：`ToolRegistry`

重写 `agent/tools/registry.rs`，删掉 `to_rig_tools()` 与 `ToolSpec`：

```rust
pub struct ToolRegistry {
    tools: indexmap::IndexMap<String, ToolEntry>,
    enabled: HashMap<String, bool>,
    aliases: HashMap<String, String>,
}

impl ToolRegistry {
    /// 注册（或覆盖）一个工具。
    pub fn register(&mut self, name: &str, entry: ToolEntry, enabled: bool);

    /// 前端工具列表：真实描述、参数 schema、风险注解。
    pub fn catalog(&self) -> Vec<ToolDefinition>;

    /// 本轮给模型看的工具：已启用 ∩ 白名单（白名单先过别名归一）。
    pub fn active(&self, allowed: Option<&[String]>) -> Vec<DynamicTool>;

    /// 按名直接执行（deep 路径）。
    pub async fn execute(&self, name: &str, args: serde_json::Value) -> Result<ToolOutput>;

    /// 查工具条目（含 footprint 解析器）。
    pub fn resolve(&self, name: &str) -> Option<&ToolEntry>;

    /// 开关工具；未注册时返回错误。
    pub fn toggle(&mut self, name: &str, enabled: bool) -> Result<()>;
}
```

变化点：

- **`active()` 里白名单先归一**：`allowed` 里的每个名字先过别名表（`read_file` → `file_system`），
  再和注册键比较（修 D-12）。
- **`execute()` 不再把返回值当 JSON 解析**：直接返回 `ToolOutput`（修 D-14）。
- **`catalog()` 返回 Rig 的 `ToolDefinition`**，前端拿到真实 description / parameters / annotations（修 D-11）。
- **启用状态有两个来源，不能互相覆盖**：
  - 内置工具的启用状态以 `AppConfig::enabled_tools` 为准（启动时写入注册表，`toggle` 时同步回配置）；
  - MCP 工具的启用状态以 `McpToolConfig` 为准，注册时写入注册表，**不受 `AppConfig.enabled_tools` 支配**。
  `list_tools` 直接读注册表的 `enabled`，**不再**用 `config.enabled_tools.contains` 覆盖全部工具
  （现在这么做会把已启用的 MCP 工具显示成关闭）。
  注意 `AppConfig::enabled_tools` 的默认值里没有 `run_workflow`（`app/config.rs:212-217`），
  而注册表实际把它注册成了启用——现状是「实际开着、界面显示关着」。本轮以注册表为准，
  默认值是否补 `run_workflow` 见 §11 的待决项。

### 4.4 fast 路径

**审批服务必须能进到 provider 里。** `RigProvider` 在配置加载时就构造好了
（`llm/rig.rs:225` 的 `create_rig_provider`），拿不到 `AppHandle`、会话与每轮策略；
`ProviderBox` 的方法签名里也没有这些（`llm/provider.rs:41-77`）。
所以「签名不变」这个假设不成立——审批通道必须显式注入：

- 新增 `ToolApprovalService`（`agent/tools/approval.rs`）：持有 `AppHandle`、当前轮策略、
  待确认请求表（按审批 id 索引）。
- `AppState::new(app_handle)`（`app/state.rs:87`）里创建它，
  传给 `ProviderRegistry::new(models, approval)` → `create_rig_provider(model, approval)`。
- `RigProvider` 持有 `Arc<ToolApprovalService>`，每次构造 agent 时挂上 hook。

于是 `ProviderBox` 的工具方法签名要变：

```rust
/// 支持工具的单轮 prompt：返回最终文本与本轮完整 transcript。
async fn prompt_with_tools(
    &self,
    preamble: &str,
    prompt: &str,
    tools: Vec<DynamicTool>,
    max_turns: usize,
) -> Result<TurnOutcome>;

/// 支持工具的多轮 chat。
async fn chat_with_tools(
    &self,
    preamble: &str,
    prompt: &str,
    history: &[Message],
    tools: Vec<DynamicTool>,
    max_turns: usize,
) -> Result<TurnOutcome>;

/// 支持工具的流式 prompt。
async fn stream_with_tools(
    &self,
    preamble: &str,
    prompt: &str,
    history: &[Message],
    tools: Vec<DynamicTool>,
    max_turns: usize,
) -> Result<StreamingResult>;
```

`TurnOutcome` 是项目自己的小结构：

```rust
/// 一轮工具执行的结果：最终文本 + 本轮 transcript + 策略判定记录。
pub struct TurnOutcome {
    /// 模型的最终回答。
    pub output: String,
    /// 本轮产生的消息（assistant 轮次 + 工具结果），供落库用。
    pub messages: Vec<Message>,
    /// 本轮每个工具调用的终态（按 call id），用来区分失败 / 用户拒绝 / 策略拒绝。
    pub decisions: HashMap<String, ToolCallStatus>,
}

/// 一次工具调用的终态。
pub enum ToolCallStatus {
    Completed,
    Failed,
    /// 用户拒绝或等待超时。
    Rejected,
    /// 策略 / 白名单 / 并行冲突阻止，未执行。
    PolicyDenied,
}
```

> `decisions` 不能从 transcript 反推：Rig 的 `ToolResult` 只区分成功 / 错误 / 跳过 / 拒绝，
> 不知道「跳过」是用户拒绝还是并行冲突。这个信息由本轮的 `RunPolicyState` 记录（§4.8）。

> `history: &mut Vec<Message>` 这个形参本来就不生效（`llm/rig.rs:148`、`:179` 两处都 `clone()`），
> 本轮顺手改成 `&[Message]`（只读），把「要不要写回」这件事去掉。

**agent 与 runner 的增补**：

```rust
AgentBuilder::new(model.clone())
    .preamble(preamble)
    .dynamic_tools(tools)
    .add_hook(ToolPolicyHook::new(approval_service, policy))   // 审批 + 冲突判定
    .add_hook(InvalidCallHook)                                   // 参数畸形时带反馈重试
    .build()
```

```rust
agent.prompt(prompt)
    .max_turns(max_turns)
    .tool_concurrency(4)                       // 修 D-9
    .max_invalid_tool_call_retries(2)          // 修 D-4
    .max_consecutive_malformed_tool_calls(Some(3))
```

关于 D-4 要说清楚：**光设 runner 选项不会产生「带反馈重试」**。
`max_invalid_tool_call_retries` 只在某个 hook 返回 `InvalidToolCallAction::Retry` 时才计数
（`rig-agent-0.46.0/src/run/mod.rs:1211-1217`）；没有 hook 时走
`UnhandledInvalidToolCall::Ignore`，而 `Ignore` 是**把该调用从这一轮里删掉**
（`run/mod.rs:1340-1356`），模型既没有结果也没有反馈。

所以本轮要实现一个 `on_invalid_tool_call` hook，对「参数不是 JSON 对象」的调用返回
`InvalidToolCallAction::Retry { feedback }`，反馈里点名是哪个工具、参数该怎么写。
（参数**字段写错**在类型化工具下已经是 `invalid_args` 工具结果，模型看得见并能自纠，
那种情况不走 invalid call 路径。）

### 4.5 deep 路径

`orchestrator/runtime/executor.rs::execute_tool`：

```rust
let entry = self.tools.lock().await.resolve(name)?;     // 别名 + 白名单校验
let definition = entry.tool.definition();                // definition() 返回 ToolDefinition（按值）
let footprint = entry.footprint.as_deref().and_then(|f| f(&args));
let decision = approval::decide(&approval::ApprovalInput {
    name,
    annotations: &definition.annotations,
    args: &args,
    footprint: footprint.as_ref(),
    policy: &policy,
}).await?;
match decision {
    ApprovalDecision::Deny(reason) => {
        // 不可重试：直接短路，绝不再次请求审批
        return Err(ExecutionError::ApprovalDenied { reason });
    }
    ApprovalDecision::Allow => {}
}
let output = timeout(
    timeout_seconds,
    entry.tool.run_with(&mut ToolContext::new(), args),
).await??;
```

要点：

- **`timeout_seconds` 这次真的生效**（修 D-8）：用 `tokio::time::timeout` 包住 `run_with`，
  超时按工具失败处理。
- **deep 的 `allowed_tools = None` 不再等于禁用工具**（修 D-13）：
  `executor.rs:358-359` 的 `tools_enabled = self.tools_enabled && (allowed_tools.is_some() || ...)`
  改成 `allowed_tools.is_none()` 时视为「不限制」。
- **「用户拒绝 / 超时 / 策略拒绝」必须是不可重试的分支**：
  `executor.rs:209-237` 现在对任何 `Err` 都按 `retry.max` 重试，
  拒绝会重试并**再次弹审批**（超时同理，会卡满两个超时）。
  新增 `ExecutionError::ApprovalDenied { reason }`，在重试循环里直接短路。
- **deep 没有模型侧的 call id**：审批请求的 id 用 `Uuid::new_v4()` 生成，
  fast 路径用模型给的 call id（这样工具卡片能和审批卡片对上）。
- **deep 的工具步骤也要走审批闸门**（§4.7）。

### 4.6 MCP 接入

`features/mcp/manager.rs`：

- `McpToolWrapper` 从「实现 `AgentTool`」改成「产出一个 `DynamicTool`」。
- **工具名加 `server__tool` 前缀**（修 D-6）：`format!("{}__{}", server_id, tool_name)`。
  注册与注销都用带前缀的名字，内置工具不会被顶掉、不会被误删。
- **`is_error` 落到 `ToolExecutionError`**（修 D-5）：MCP 返回 `is_error = true` 时，
  回调返回 `Err(ToolExecutionError::other(...))`，而不是把错误文本当成功结果。
- **`ConfirmOnce` 真正生效**（修 D-7）：映射到统一闸门的「需要确认」策略（§4.7）。
- `McpToolConfig.confirmation` 的三种取值语义固定为：
  `Deny` = 永不执行；`AutoAllow` = 直接执行；`ConfirmOnce` = 每次都要用户点头。

### 4.7 审批闸门

**判定**（`agent/tools/approval.rs`，fast 与 deep 共用）：

```rust
/// 一次工具调用的审批输入。
pub struct ApprovalInput<'a> {
    pub name: &'a str,
    pub annotations: &'a ToolAnnotations,
    pub args: &'a serde_json::Value,
    /// 本次调用碰到的东西（读 / 写范围），由注册表的 footprint 解析器算出。
    pub footprint: Option<&'a Touches>,
    pub policy: &'a ToolPolicy,
}

pub enum ApprovalDecision {
    Allow,
    Deny(String),
}
```

判定顺序（**顺序即优先级**）：

1. 工具在 `ToolPolicy.deny_list` 里 → `Deny`（不弹窗，直接拒）。
2. 工具在 `ToolPolicy.allow_list` 里 → `Allow`（即使声明了 `destructive` 也不弹窗；
   MCP 的 `AutoAllow` 映射到这里）。
3. 工具在 `ToolPolicy.confirm_list` 里 → 需要确认（MCP 的 `ConfirmOnce` 映射到这里）。
4. **有 footprint 时以 footprint 为准**：`Touches::Writes(_)` → 需要确认；
   `Touches::Reads(_)` / `Touches::Independent` → `Allow`。
5. 没有 footprint 时退回注解：`annotations.destructive()` 为真 → 需要确认。
6. 否则 `Allow`。

> **为什么第 4 步要覆盖第 5 步**：`destructive` 是**工具级**注解，`footprint` 是**每次调用**的事实。
> `file_system` 既会读也会写，只能在注解里声明 `destructive(true)`；如果先看注解，
> 连只读的 `read_file` 都会弹窗。所以「能算出本次调用范围」时，注解让位给 footprint。
>
> 这也是为什么 `into_dynamic` 对**每个**工具都产 `FootprintFn`：默认实现走
> `Touches::from_annotations`，`footprint` 为 `None` 只发生在参数反序列化失败时。

> **判定只看 `destructive()`，不看 `open_world()`。**
> `ToolAnnotations::default()` 的 `destructive()` 与 `open_world()` 默认都是 true
> （`rig-core/src/completion/request.rs:165-178`），若把 `open_world` 也算作触发条件，
> 每个工具都会弹窗（`web_search` 之类只读但开放的工具也要确认，实际不可用）。
> `open_world` 只用于界面标注「访问外部资源」，以及将来放宽策略时用。

**读 / 写分级**：同一个 `file_system` 工具，读和写的风险不同。
`Tool::footprint(&args)` 正好表达「这次调用碰什么」——
`file_system` 重写 `footprint`：读操作返回
`Touches::Reads(Scope::Paths(vec![绝对路径]))`，写 / 删返回
`Touches::Writes(Scope::Paths(vec![绝对路径]))`。

内置工具的初始策略：

| 工具 | 读操作 | 写 / 删操作 |
|---|---|---|
| `calculator` | 自动 | — |
| `web_search` | 自动 | — |
| `file_system` | 自动 | **每次确认** |
| `code_executor` | — | **每次确认**（执行任意代码） |
| `run_workflow` | — | **每次确认**（触发多步副作用） |
| MCP 工具 | 按 `McpToolConfig.confirmation` | 同左 |

> MCP 工具的确认策略由 `McpToolConfig.confirmation` 决定（已有字段），默认 `AutoAllow`；
> 它们的 `annotations` 只用于界面展示，不参与判定。

**策略从哪来**（对应 builder check 第 8 条）：

- 内置工具的确认策略是**代码里的默认表** `ToolPolicy::default()`，工具页**只读展示**；
  本轮**不新增** `AppConfig` 字段、不做持久化编辑（见 §11 待决项）。
- `ToolPolicy` 的三份名单：`deny_list`（直接拒）、`allow_list`（直接放行）、
  `confirm_list`（强制确认）。内置工具默认只用注解 + footprint 判定，三份名单为空；
  工具页的「确认策略」列由 `catalog()` 推导展示：
  deny_list → 拒绝；confirm_list → 每次确认；有 FootprintFn → 写入时确认；否则 自动。
- MCP 的 `ConfirmationMode` 逐一映射：
  `Deny` → `deny_list`，`AutoAllow` → `allow_list`，`ConfirmOnce` → `confirm_list`
  （`McpToolConfig.confirmation` 已有字段）。

**交互**：

```
后端                                        前端
 │  emit tool_approval_request              │
 │   { id, name, args, reason, risk,         │
 │     scope, timeout_ms }                   │
 │                                          │ 入队，按到达顺序逐个弹窗
 │                                          │ 用户点「批准执行」
 │  ◀── invoke respond_tool_approval(id,    │
 │        approved=true, reason=None) ──────│
 │  oneshot 唤醒 → DispatchAction::proceed() │
```

- **fast 路径**：`AgentHook::on_dispatch` 拿到 `DispatchEvent`（`kind` 是 `EffectKind::ToolCall{name,args}`，
  `call_id` 是调用 id），判定后：
  - 需要确认 → `emit` 请求 → `tokio::time::timeout(timeout_ms, oneshot)` 等前端回话；
  - 批准 → `DispatchAction::proceed()`；
  - 拒绝/超时 → `DispatchAction::skip(reason)`。
    `skip` 的语义是「不执行，模型看到一条 skipped 的工具结果」（`rig-agent-0.46.0/src/agent/hook.rs:697-700`），
    所以模型能读到「用户拒绝了这次调用」并换条路，而不是整轮崩掉。
- **deep 路径**：在 `execute_tool` 里调同一个 `approval::request()` 函数（同一个 emit、同一个 IPC 命令），
  拒绝时返回 `ExecutionError::ApprovalDenied`（不可重试）。
- **超时**：默认 120 秒，超时按拒绝处理，原因写「用户未响应，已超时」。
- **并发**：`tool_concurrency(4)` 下 `on_dispatch` 是并发进入的
  （`rig-agent-0.46.0/src/agent/engine.rs:557` 用 `buffer_unordered`），
  模型一轮发 3 个写文件调用会同时产生 3 个审批请求。
  **后端不做串行化**（那会把整批工具串行化，失去并行意义），
  **前端按到达顺序排队弹窗**，逐个处理；每个请求有独立的 id 与超时。
- **没有前端在听时**：非流式 fast 路径（`session.rs:944`）与 deep 的 agent task
  也要能审批——它们同样 emit `tool_approval_request`（独立事件，不依赖 `stream_chunk`），
  前端统一监听并回复。**超时一律按拒绝处理**，宁可不做也不能静默执行破坏性操作。
- **流式状态**：等待确认时 `phase` 置 `waiting_confirmation`。

### 4.8 并行工具调用

- Rig runner `tool_concurrency(4)` 打开（修 D-9）。
- **冲突判定项目自己做**：Rig 不自动判——`DynamicTool` 的 `Serve::footprint` 默认返回 `None`
  （`rig-core/src/serve/handler.rs:103`），`DispatchEvent` 也不带 footprint。
  但注册表里存着 `FootprintFn`（§4.2），审批 / 策略 hook 可以在 `on_dispatch` 里
  自己算出本次调用的 `Touches`：
  1. 从 `DispatchEvent.kind` 取出工具名与参数；
  2. 用注册表的 `FootprintFn` 算出 `Touches`；
  3. 与本轮已放行调用的 `Touches` 比 `Touches::conflicts_with`
     （`rig-core/src/tool/footprint.rs:114`）；
  4. 冲突则 `DispatchAction::skip("与本轮其他工具调用冲突，请串行执行")`，
     让模型下一轮再试。
- **判冲突与登记必须在一个临界区里完成**：`on_dispatch` 是并发进入的
  （`rig-agent-0.46.0/src/agent/engine.rs:557` 的 `buffer_unordered`）。
  若「先查、await 审批、再登记」，两个写同一文件的调用会同时通过。
  正确做法是 `RunPolicyState` 用一把 `std::sync::Mutex`（**不在持锁时 await**）：
  ```
  {
      let mut st = state.lock();
      if st.footprints.iter().any(|(_, t)| t.conflicts_with(&fp)) {
          st.decisions.insert(call_id, PolicyDenied);
          return DispatchAction::skip(...);
      }
      st.footprints.push((call_id.clone(), fp));   // 先占位，防止并发穿透
  }
  // 锁已释放，再做审批 await
  ```
  审批被拒绝时把占位回滚（或直接用决策映射覆盖状态；冲突判定只看「已放行」集合，
  所以拒绝后应移除占位）。
- `RunPolicyState` 是**每次 provider 调用一份**（每个 agent build 一份），
  不挂在共享的 `RigProvider` 上；它同时提供 `decisions` 快照给 `TurnOutcome` / `Transcript` 事件。

- 读写同一文件的两个 `file_system` 调用会被判为冲突；两个不相干的 `web_search` 可以并行
  （`read_only = true` → `Touches::Reads(Anything)`，两个读不冲突）。
- `Touches::Independent` 在 `conflicts_with` 里按 `Reads(Anything)` 处理
  （`footprint.rs:113`）：它不挡任何调用，但会排在已登记的写操作之后。
  普通只读工具仍然用 `Touches::Reads(...)`；`Independent` 只留给「不改变其他调用可见状态」的工具。

### 4.9 可靠性修复

**（1）`tool_calls` 落库（修 D-1）**

**用 Rig 的 transcript 落库，不要自己猜切分点。**
`PromptResponse.messages` 就是这一轮的完整记录（`rig-agent-0.46.0/src/run/response.rs:91-97`）：
「the prompt, every accepted assistant turn, every committed tool result」，
不含 run 之前的输入历史。流式路径的 `FinalResponse(response)` 里也带着它。

于是：

- `ProviderBox::prompt_with_tools` / `chat_with_tools` 返回 `TurnOutcome { output, messages, decisions }`；
  流式路径在 `Done` 之前把 transcript 一并交给上层（§4.4、§5.1）。
  `decisions` 是 `call_id → ToolCallStatus` 的映射，由本轮 `RunPolicyState` 记录（§4.8），
  用来区分「执行失败 / 用户拒绝 / 策略拒绝」——transcript 本身只存内容，不存策略原因。
- 落库（`app/commands/session.rs`）：**按 Rig 的消息结构遍历** transcript，不能简单「跳过 user 消息」：

  Rig 里的工具结果不是顶层 `role="tool"`，而是
  `Message::User { content: [UserContent::ToolResult(..)] }`
  （`rig-core/src/completion/message.rs`，`Message::tool_result` 就是这么构造的）。
  提示词与「重问」反馈也是 `Message::User`。所以判定必须看 **content 变体**：

  1. `Message::Assistant` → 写一条 `role="assistant"` 行：`content` 取文本部分，
     `tool_calls` 存 JSON 数组 `[{"id":…,"name":…,"arguments":{…}}]`；
  2. `Message::User` 里每个 `UserContent::ToolResult` → 写一条 `role="tool"` 行：
     `content` 取结果文本（`ToolOutput::render()`）、`tool_call_id` 取 call id、
     `tool_name` 取真实工具名、状态取 `decisions[call_id]`（缺省按 `is_error` 推）；
  3. 其余 `Message::User`（本轮 prompt、重问反馈）→ 跳过（用户输入已单独落库）。

  这样得到的顺序是正确的：
  `assistant(工具调用前的话 + tool_calls)` → `tool(结果)` → `assistant(最终回答)`。
  > 这比「按 ToolCall 事件切分文本」可靠：现在 `full_content` 把整轮文本（工具调用前的话 +
  > 最终回答）累在一起、回合结束才写成一条 assistant（`session.rs:319-345`），
  > 工具调用前那段话的位置是错的，而且和最终回答混在同一条里。
- 还原（`assemble_fast_messages`）：
  - 带 `tool_calls` 的 assistant 行 → `Message::Assistant { content: vec![AssistantContent::ToolCall(...)] }`
    （`rig-core/src/completion/message.rs:206`），文本部分照旧；
  - `role="tool"` 行 → `Message::tool_result(call, name, content)`
    （`rig-core/src/completion/message.rs:1130`），**用真实的工具名**，
    不再伪造 `ToolName::new("tool")`（`session.rs:37-45`）。
- 数据：`messages` 表新增 `tool_name` 列（迁移 v12）。
- 旧数据：没有 `tool_name` 的历史 tool 行在组装时直接跳过（Rig 本来也会丢弃它们），
  不需要回填迁移。
- **出错轮次没有 transcript**（run 以错误结束）：退回现在的行为——只写累积文本的 assistant 行。

**（2）流式结束语义（修 D-2、D-3）**

`StreamEvent` 增补：

```rust
pub enum StreamEvent {
    Content(String),
    ToolCall(ToolCallInfo),
    ToolResult(ToolResultInfo),
    /// 本轮出错结束：带原因，不再靠文本前缀表达错误。
    Error(String),
    /// 本轮正常结束。**任何路径都必须发一次**。
    Done,
    /// 本轮的完整 transcript（assistant 轮次 + 工具结果），落库用。
    Transcript(Vec<TurnMessage>),
}
```

`TurnMessage` 是项目自己的落库友好结构（与 Rig 的 `Message` 解耦，
避免把 Rig 类型泄到 IPC 层）：

```rust
/// transcript 里的一条消息。
pub struct TurnMessage {
    /// assistant / tool。
    pub role: String,
    /// 文本内容。
    pub text: String,
    /// assistant 的工具调用（id / 工具名 / 参数）。
    pub tool_calls: Vec<TurnToolCall>,
    /// tool 消息对应的调用 id 与工具名。
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    /// `completed` / `failed` / `rejected` / `policy_denied`；
    /// assistant 消息为 `None`。
    pub status: Option<ToolCallStatus>,
    /// 该次工具调用是否失败。
    pub is_error: bool,
}

pub struct TurnToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}
```

- 出错分支发 `Error(原因)` 再发 `Done`。
- `Transcript` 在 `Done` 之前发；出错轮次可能没有 transcript（发空数组）。
- `FinalResponse` 不再当文本追加：`FinalResponse.output()` 是最后一轮 assistant 的完整文本，
  与已推过的 Text delta 重复。流式路径下最终文本以 delta 为准，`FinalResponse` 只用来取 transcript。
- 前端 `store/index.ts` 收到 `done` 时无条件收尾（现在已经是），另外补 `error` 分支展示。

**（3）参数错误预算（修 D-4）**：见 §4.4。

**（4）工具结果结构化落库（配合 §5）**

## 5. 前后端契约

### 5.1 IPC 事件

```rust
// core/ipc.rs
pub struct StreamChunk {
    pub content: String,
    pub done: bool,
    pub tool_calls: Option<Vec<ToolCallEvent>>,
    pub phase: Option<String>,
    /// 本轮错误原因；`done` 时非空表示出错结束。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub struct ToolCallEvent {
    pub id: String,
    pub name: String,
    /// calling | completed | failed | rejected | policy_denied
    pub status: String,
    /// 模型传入的原始参数 JSON 字符串（新增）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<String>,
    pub result: Option<String>,
    pub is_error: bool,
    /// 执行耗时（新增）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// 工具的一句话用途（新增，来自 ToolDefinition.description）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// 风险等级：read_only | destructive | open_world（新增）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk: Option<String>,
}
```

新增事件（审批请求）：

```rust
pub struct ToolApprovalRequest {
    /// 等于工具调用 id。
    pub id: String,
    pub name: String,
    pub args: Option<String>,
    /// 为什么需要确认（面向用户的中文说明）。
    pub reason: String,
    pub risk: String,
    /// 影响范围（如「当前项目目录 /workspace/a.txt」）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// 后端等待回复的时限。
    pub timeout_ms: u64,
}
```

新增 IPC 命令：

```rust
#[tauri::command]
pub async fn respond_tool_approval(
    state: State<'_, AppState>,
    call_id: String,
    approved: bool,
    reason: Option<String>,
) -> Result<()>;
```

### 5.2 前端类型

```ts
export interface StreamToolCall {
  id: string;
  name: string;
  status: 'calling' | 'completed' | 'failed' | 'rejected' | 'policy_denied';
  args?: string;
  result?: string;
  is_error?: boolean;
  duration_ms?: number;
  description?: string;
  risk?: 'read_only' | 'destructive' | 'open_world';
}
```

前端 store 要改成**按 id 合并** `activeToolCalls`（现在整体替换，并行调用只会显示最后一个）。

### 5.3 工具管理页的 IPC

`list_tools` 改为返回 Rig 的 `ToolDefinition`（name / description / parameters / annotations）+
`enabled` + `source`（`builtin` 或 `mcp:{server_id}`）+ `confirmation`（`auto` / `confirm` / `deny`）。

签名加一个可选的 `session_id`：

- **能算出来的原因**：全局开关关闭、当前会话未启用（`session.config.enabled_tools`）。
- **算不出来的**：「当前意图白名单未允许」——意图是**每轮路由**出来的，
  工具页打开时并不存在「当前意图」这个状态。所以这一条降级为文案说明：
  「本轮意图白名单在每轮路由后生效」，而不是一个看起来精确的断言。

## 6. 前端设计

完整规格见 §附录（由设计专员产出）。要点：

- **工具调用卡片**：头部（状态图标 + 工具名 + 耗时 + 状态文案）→ 描述行 → 参数区（默认折叠、
  JSON 美化、可复制）→ 结果区（成功/失败/拒绝三种配色，长结果截断并提示）。
- **并行调用**：同一轮的卡片用左侧竖线连成时间线。
- **审批弹窗**：沿用 `PlanConfirmDialog` 的 antd `Modal`（`closable=false`、`maskClosable=false`），
  显示工具名、来源、风险等级、影响范围、原因、参数（JSON 美化）、可选拒绝原因输入，
  按钮「批准执行」/「拒绝」。等待期间 `phase = waiting_confirmation`，聊天区顶部挂一条横幅。
- **失败语义三态**：`failed`（红叉，模型可重试）/ `rejected`（琥珀，用户拒绝，不可重试）/
  `policy_denied`（琥珀，策略或白名单阻止，未执行）。
- **工具管理页**：真实描述、参数表（名称/类型/必填/说明）、风险标签、来源徽标、确认策略，
  以及「当前未启用原因」解释区（全局开关 / 会话未启用 / 意图白名单不允许）。

## 7. 决策表

| # | 决定 | 理由 |
|---|---|---|
| D1 | 唯一运行时形态 = `rig_core::tool::DynamicTool` | 对象安全、可克隆、`run_with` 直接调用，fast/deep 共用同一批对象 |
| D2 | 唯一编写形态 = `rig_core::tool::Tool` | 参数类型化 + schema 自动生成 + annotations/footprint |
| D3 | 项目保留薄策略层 `ToolRegistry` | Rig 只管注册与执行，不管「这轮给模型看哪些」 |
| D4 | 白名单先过别名归一再匹配 | 修 D-12；别名表本来就是项目概念 |
| D5 | fast 路径仍走 Rig agent 循环 | 换的是工具链，不是换掉 Rig 的 agent |
| D6 | 用户拒绝 → `DispatchAction::skip` | 模型看到 skipped 结果可换路；`deny`+Cancelled 会终止整轮 |
| D7 | 审批判定函数 fast/deep 共用 | 两条路的风险语义必须一致 |
| D8 | 并行冲突由项目按 `Touches` 判定并 skip | Rig 不自动判冲突（`DynamicTool` 的 `Serve::footprint` 返回 `None`） |
| D9 | 落库以 Rig 的 `PromptResponse.messages` 为准 | 权威 transcript，不用自己猜切分点；顺带修好「工具调用前的话位置错」 |
| D10 | 流式结束：新增 `Error` 事件，`Done` 必发 | 修 D-2；前端不再卡死 |
| D11 | `FinalResponse` 不再当文本追加 | 修 D-3 |
| D12 | 实现 `on_invalid_tool_call` hook 返回 `Retry{feedback}` + 预算 2 | 光设 runner 选项不产生重试；`Ignore` 是静默丢弃 |
| D13 | MCP 工具名加 `server__` 前缀 | 修 D-6 |
| D14 | MCP `is_error` → `ToolExecutionError` | 修 D-5 |
| D15 | MCP `ConfirmOnce` 映射到统一闸门 | 修 D-7，同时兑现 PRD |
| D16 | deep 的 `timeout_seconds` 这次真的生效 | 修 D-8 |
| D17 | 审批只做单次，不做持久化授权 | 本轮边界；避免授权状态散落在多处 |
| D18 | 审批判定只看 `destructive` / 写入 footprint，不看 `open_world` | `ToolAnnotations` 默认 `open_world=true`，算进去等于每个工具都弹窗 |
| D19 | 审批服务由 `AppState::new` 创建并注入 `ProviderRegistry` | provider 在配置加载时构造，那时没有 AppHandle；不改签名就进不去 |
| D20 | 并发审批由前端排队，不在后端串行化 | 后端串行会把整批工具串行化，失去并行意义 |
| D21 | 工具错误不脱敏（`map_error` 不用 `from_error`） | `from_error` 会把模型可见反馈替换成固定文案，模型失去可操作信息 |
| D22 | 工具结果统一用 `ToolOutput::render()` 提取 | JSON 形态的 `ToolOutput` 没有 `as_text()`，只 join 文本会得到空串 |
| D23 | 注册表的 `enabled` 是运行时真相，`AppConfig.enabled_tools` 只管内置工具 | 现在 `list_tools` 用 config 覆盖全部工具，会把已启用的 MCP 工具显示成关闭 |
| D24 | footprint 用 `Touches::{Reads,Writes,Independent}` + `Scope::{Paths,Anything}`；`Tool::footprint` 返回 `Touches` 不是 `Option` | 已核实 `rig-core/src/tool/footprint.rs:27,51,71`；转换层用 `T: Tool + Clone` 保留一份给 footprint 闭包 |
| D25 | 审批优先级：deny → allow → confirm → footprint → destructive → allow；footprint 覆盖注解 | `destructive` 是工具级、footprint 是调用级；否则 `file_system` 的读调用也会弹窗 |
| D26 | 落库按 Rig 消息 **content 变体**遍历：`UserContent::ToolResult` 才是工具结果 | Rig 的工具结果是 `Message::User` 内含 `ToolResult`，简单「跳过 user 消息」会把结果全丢掉 |
| D27 | 每轮一份 `RunPolicyState` 记录 decisions 与已放行 footprint；判冲突 + 登记在同一临界区，不在持锁时 await | 并发 hook 下「先查后登记」会让两个写同一文件的调用同时通过 |

## 8. 影响范围

### 8.1 Rust

| 文件 | 改动 |
|---|---|
| `agent/tools/base.rs` | 删除 `AgentTool` / `ToolSpec` / `ToolError`；保留 `ToolInfo`（前端用）并扩字段 |
| `agent/tools/registry.rs` | 重写为策略层（§4.3）：存 `ToolEntry`，提供 `catalog` / `active` / `execute` / `resolve` |
| `agent/tools/convert.rs` | 新增：`into_dynamic::<T>()` → `ToolEntry`（工具 + footprint 解析器） |
| `agent/tools/approval.rs` | 新增：`ToolApprovalService`（持有 AppHandle、策略、待确认表）+ 判定函数 + 请求/回复通道 |
| `agent/tools/{calculator,file_system,web_search,code_executor,run_workflow}.rs` | 改写为类型化 `Tool`；显式声明三个 annotations；`file_system` 实现 `footprint` 区分读写 |
| `agent/fast_loop.rs` | `run` 返回 `TurnOutcome`（含 transcript + decisions）、`run_stream` 的 `Transcript` 事件携带同一份；事件补 `args`/`duration_ms`；新增 `Error` / `Transcript` 事件；`Done` 必发；去掉 `FinalResponse` 文本重复；结果用 `ToolOutput::render()` 提取 |
| `llm/rig.rs` | 持有审批服务；runner 加 `tool_concurrency` / 重试预算；挂 `ToolPolicyHook` 与 `InvalidCallHook` |
| `llm/provider.rs` | **签名要变**：工具方法返回 `TurnOutcome`（含 transcript）；`history` 改只读 |
| `app/state.rs` | 创建 `ToolApprovalService` 并注入 `ProviderRegistry::new` |
| `app/commands/session.rs` | 按 transcript 落库 assistant/tool 行；写 `tool_name`；组装历史时还原 ToolCall；fast/deep 路径接审批 |
| `app/commands/tool.rs` | `list_tools` 返回真实定义 + 来源 + 确认策略；加可选 `session_id` |
| `app/commands/mcp.rs` | 审批回复命令 `respond_tool_approval` |
| `core/ipc.rs` | `StreamChunk` / `ToolCallEvent` 扩字段；新增 `ToolApprovalRequest` |
| `core/db/{models,repository}.rs` | `messages` 加 `tool_name` 列（迁移 v12）；`Message` 模型扩字段 |
| `agent/orchestrator/runtime/executor.rs` | 工具执行改走 `ToolRegistry::execute`；超时生效；白名单 `None` 不再等于禁用；接审批；拒绝走不可重试分支 |
| `agent/orchestrator/plan_error.rs` | 新增 `ExecutionError::ApprovalDenied { reason }` |
| `agent/orchestrator/planner.rs` | 工具目录改用 `catalog()` |
| `features/mcp/manager.rs` | 产 `DynamicTool`；名字加前缀；`is_error` 落地；`ConfirmOnce` 生效 |

### 8.2 前端

| 文件 | 改动 |
|---|---|
| `types/index.ts` | `StreamToolCall` 扩字段；新增 `ToolApprovalRequest` |
| `store/index.ts` | `activeToolCalls` 改按 id 合并；落库消息内容结构化；处理 `error` 事件 |
| `store/uiSlice.ts` | `activeToolCalls` 类型扩字段 |
| `components/session/ChatArea.tsx` | 工具卡片重做（参数/结果/耗时/风险）；`waiting_confirmation` 阶段提示 |
| `components/session/MessageBubble.tsx` | `ToolMessage` 展示参数与结构化结果 |
| `components/session/ToolApprovalDialog.tsx` | 新增：审批弹窗 |
| `components/management/ToolManagerPage.tsx` | 工具目录页（若已有则改） |
| `api/tauri.ts` | 新增 `respondToolApproval` |

## 9. 迁移顺序

每一步都可独立验证，前一步不破坏后一步：

| 步 | 内容 | 验证判据 |
|---|---|---|
| 0 | 建分支 `ai/migrate-toolchain-to-rig` | `node scripts/start-session.mjs` |
| 1 | 类型化工具 + `into_dynamic` + 注册表重写（**不接线**） | `cargo test` 通过；新增 `agent/tools` 单测覆盖 schema / 别名 / 白名单 |
| 2 | 接线 fast 路径（`DynamicTool` 直接替换旧转换） | fast 路径手工验证：调用 calculator / file_system 正常；`cargo check` 无警告 |
| 3 | 接线 deep 路径 + 超时生效 | deep 计划含 `tool_call` 步骤能跑通；超时用例有测试 |
| 4 | MCP 改造（名字前缀 / is_error / ConfirmOnce） | MCP 工具注册后内置工具仍在；断开后内置工具未被删 |
| 5 | 可靠性修复（tool_calls 落库 / Error 事件 / 去重 / 重试预算） | 新增测试：带工具调用的会话第二轮历史包含 ToolCall；流式出错后前端收尾 |
| 6 | 审批闸门（后端 + 前端） | 新增测试：确认与拒绝两条路径；拒绝后模型收到 skipped 结果 |
| 7 | 并行工具调用 | 新增测试：冲突调用被 skip；独立调用并行 |
| 8 | 工具管理页与前端卡片 | `npx tsc --noEmit` 通过；`npm run build` 通过 |

## 10. 风险

| 风险 | 缓解 |
|---|---|
| 类型化改造触及 5 个工具的核心逻辑，行为可能漂移 | 每个工具先补「旧行为 → 新行为」的等价测试再改写；`calculator` 已有 `tests/tool_calculator.rs` 可作基线 |
| `DynamicTool` 转换时丢 annotations 与 footprint | 转换函数里显式 `.with_annotations(...)` 并一并产出 `FootprintFn`；写单测断言 |
| `ProviderBox` 签名变更波及所有 provider 实现 | 只有 `RigProvider` 一个实现（`llm/provider.rs`），生产调用点只有 `AppState::new` 一处（`app/state.rs:97`）；另有 14 处 `#[cfg(test)]` 构造点（mcp_bridge、workers、orchestrator、intent/router 的测试模块）需要机械补一个 `None` 参数 |
| 审批等待阻塞 agent 回合 | oneshot + 120s 超时；超时按拒绝；拒绝用 `skip` 不终止回合 |
| 并发审批产生多个弹窗 | 前端按到达顺序排队；后端不串行化（见 D20） |
| 并行调用引入竞态（两个写同一文件） | `Touches` 冲突判定 + `skip` 回灌让模型串行重试 |
| 历史格式变更影响旧会话 | 旧 tool 行跳过即可，不需要数据回填；迁移只加列不改数据 |
| deep 路径改动面大 | deep 的工具执行只有一处（`execute_tool`），单点改 + 单测覆盖 |
| deep 把「拒绝」当可重试失败 → 反复弹审批 | 新增不可重试的 `ExecutionError::ApprovalDenied`，在重试循环里短路 |
| `ToolAnnotations` 默认值让每个工具都要确认 | 判定只看 `destructive`；每个工具显式声明三个标志（D18） |

## 11. 已决项

这两项是产品决策，已拍板：

### 11.1 工具的「确认策略」只读展示，不做编辑

内置工具的确认策略写在代码里的默认表（`ToolPolicy::default()`），
工具页**只读展示**「自动 / 每次确认 / 拒绝」与风险标签。

不新增 `AppConfig` 字段、不做持久化编辑、不涉及配置迁移——
「记住授权」这个 Non-Goal 完整保留。MCP 工具的确认策略仍来自
`McpToolConfig.confirmation`（已有字段）。

### 11.2 `run_workflow` 默认启用

现状是「实际开着、界面显示关着」：`AppConfig::enabled_tools` 的默认值里没有它
（`app/config.rs:212-217`），而注册表把它注册成了启用（`registry.rs:69`），
`list_tools` 又用 config 覆盖了显示值。

**决定：默认启用** —— 把 `run_workflow` 补进 `AppConfig.enabled_tools` 的默认值，
与「以注册表为准」的运行时真相对齐。

## 12. 验证方式

```bash
# Rust
cd src-tauri && cargo test            # 含新增的 agent/tools、approval、fast_loop 测试
cd src-tauri && cargo check

# 前端
cd src-ui && npx tsc --noEmit
cd src-ui && npm run build
cd src-ui && npm test

# 手工
# 1. fast 路径调用一次 file_system 读文件 → 第二轮问「刚才读到了什么」→ 上下文不丢
# 2. 让模型调 file_system 写文件 → 弹审批窗 → 拒绝 → 模型收到拒绝原因并换路
# 3. 让模型一轮里读两个不同文件 → 两张卡片都出现
```

## 附录：前端规格（设计专员产出）

> 完整交互细节见本节；实现时以 §6 的要点为准。

### A.1 工具调用卡片

```
┌─────────────────────────────────────────────────────────────┐
│  {状态图标}  {工具名}  ·  {耗时}  ·  {状态文案}   [展开/收起]  │
├─────────────────────────────────────────────────────────────┤
│  {description}                                               │
│  ─────────────────────────────────────────────────────────  │
│  参数 ▾                                                      │
│  ┌─────────────────────────────────────────────────────┐   │
│  │ {pretty JSON}                                       │   │
│  └─────────────────────────────────────────────────────┘   │
│  [复制参数]                                                  │
│  ─────────────────────────────────────────────────────────  │
│  结果 ▾ / 错误 ▾ / 已拒绝 ▾ / 阻止原因 ▾                     │
│  ┌─────────────────────────────────────────────────────┐   │
│  │ {JSON 或纯文本，长则截断}                            │   │
│  └─────────────────────────────────────────────────────┘   │
│  [复制结果]                                                  │
└─────────────────────────────────────────────────────────────┘
```

状态表：

| status | 图标 | 头部文案 | 强调色 | 结果区标题 |
|---|---|---|---|---|
| `calling` | 旋转 Loader | 正在调用: {name} | orange-500 | — |
| `completed` | CheckCircle | 已完成: {name} | green-500 | 结果 |
| `failed` | XCircle | 执行失败: {name} | red-500 | 错误 |
| `rejected` | ShieldX | 已拒绝: {name} | amber-600 | 拒绝原因 |
| `policy_denied` | Lock | 策略已阻止: {name} | amber-500 | 阻止原因 |

- 参数区默认折叠；`args` 尝试 `JSON.parse` 后美化，失败原样显示；空或 `{}` 显示「无参数」。
- 结果区默认截断 500 字符并提示「共 N 字符，点击展开」；展开后 `max-h-96 overflow-y-auto`。
- `is_error = true` 时即使 `status = completed` 也用失败样式。
- `failed` 卡片底部可加一行「模型可能会根据错误信息重试」；拒绝类不加。

### A.2 审批弹窗

```
┌─────────────────────────────────────────────────────────────┐
│  ⚠️ 需要您的确认                                             │
├─────────────────────────────────────────────────────────────┤
│  模型请求执行以下操作：                                       │
│  ┌─────────────────────────────────────────────────────┐   │
│  │ 工具名    file_system                               │   │
│  │ 来源      内置 / MCP: filesystem                   │   │
│  │ 风险      破坏性操作（会修改文件）                   │   │
│  │ 影响范围  当前项目目录 /workspace/note.txt         │   │
│  └─────────────────────────────────────────────────────┘   │
│  需要确认的原因：该工具会修改文件、数据或状态。               │
│  调用参数： {pretty JSON}                                    │
│  [可选] 拒绝原因：（会回传给模型）                            │
│                                     [ 拒绝 ]  [ 批准执行 ]   │
└─────────────────────────────────────────────────────────────┘
```

- antd `Modal`，宽 520，`closable={false}`、`maskClosable={false}`，禁止 ESC 关闭。
- 风险文案：`destructive` → 「该工具会修改文件、数据或状态，需要您确认。」；
  `open_world` → 「该工具会访问外部网络或执行不受限代码，需要您确认。」
- 等待期间聊天区顶部显示横幅：「模型正在等待您对一次 {风险} 操作进行确认，请先处理弹窗。」，
  `phase = waiting_confirmation`。
- 超时按拒绝，原因「用户未响应，已超时」。
- **不做**「以后都允许」这类持久化授权。

### A.3 工具管理页

单条工具卡片：名称（mono）+ 来源徽标（内置 / MCP: {server}）+ 启用开关；描述；风险标签
（破坏性 red / 只读 green / 网络 blue）+ 确认策略（自动 green / 每次确认 amber / 拒绝 red）；
参数表（名称 / 类型 / 必填 / 说明）；未启用时列出原因：

- 全局开关关闭
- 当前会话未启用
- 当前意图白名单未允许

### A.4 消息落库格式（前端侧）

`role='tool'` 的消息 `content` 存结构化 JSON，便于 `ToolMessage` 统一读取：

```json
{
  "tool_call_id": "call_xxx",
  "name": "file_system",
  "status": "completed",
  "args": "{\"path\":\"a.txt\"}",
  "result": "{\"ok\":true}",
  "is_error": false,
  "duration_ms": 123
}
```

---

## 13. 假设核验记录（2026-11-05）

> 原计划由子代理复核，但 oracle / explorer 均因 `Go usage limit exceeded` 不可用；
> 以下由主会话直接读 Rig 0.46 源码与本仓库核对。结论只列「成立」与「已修正」。

| # | 断言 | 结论 | 证据 / 处置 |
|---|---|---|---|
| 1 | `PromptResponse.messages` 是 prompt + 本轮 assistant 轮次 + 工具结果，不含输入历史 | 成立 | `rig-agent-0.46.0/src/run/mod.rs:628-634`（`messages()` 文档）与 `:1105-1116`（`finish()` 从 `CommittedLog` 取） |
| 2 | 流式 `FinalResponse` 同样带 `messages` | 成立 | 阻塞与流式共用 `finish()`；`rig-agent-0.46.0/src/agent/engine.rs:1122` 产出 `Surfaced::Final` |
| 3 | `on_dispatch` 是 async，能拿到 call id / 工具名 / 参数 | 成立 | `agent/hook.rs:623-635`（`DispatchEvent`）、`:1492`（`async fn on_dispatch`） |
| 4 | `tool_concurrency` 真的并发，默认 1 | 成立 | `agent/engine.rs:557`（`buffer_unordered`）、`agent/runner.rs:427` |
| 5 | `DispatchAction::skip` = 不执行且模型看到 skipped 结果 | 成立 | `agent/hook.rs:697-700`；非 `Cancelled` 的 `Deny` 变成 skipped（`:647-651`） |
| 6 | `tool_definition` / `args_schema` / `ToolAdapter` / `DynamicTool::new_with_context` / `with_annotations` 公开可用 | 成立 | `rig-core/src/tool/mod.rs:25,34`；`serve/mod.rs:17`；`tool/contextual.rs:182,199` |
| 7 | `DynamicTool: Clone` | 成立 | `tool/contextual.rs:150` |
| 8 | `DynamicTool::definition()` 可按引用取 annotations | **已修正** | 它返回 `ToolDefinition` **按值**（`contextual.rs:219`），先绑定局部变量再取引用（§4.5） |
| 9 | `footprint` 返回 `Option<Touches>`，变体叫 `Scope::Read/Write` | **已修正** | 实际 `Tool::footprint(&self, args) -> Touches`（`contextual.rs:71`），变体 `Touches::Reads/Writes/Independent` + `Scope::Paths/Anything`（`footprint.rs:27,51`）；相对路径 normalize 后退化为 `Anything` |
| 10 | 转换层可以「同时」拿工具本体与 ToolAdapter | **已修正** | `ToolAdapter::new` 按值消费工具（`adapters.rs:415`），因此加 `T: Clone` 约束，克隆一份给 footprint 闭包（§4.2） |
| 11 | `ProviderRegistry::new` 只在 `AppState::new` 一处调用 | **已修正** | 生产调用点确实只有 `app/state.rs:97`；另有 14 处 `#[cfg(test)]` 调用点需要机械补 `None`（§10 风险表） |
| 12 | `ToolExecutionError::from_error` 会脱敏模型可见反馈；`other()` 公开 | 成立 | `tool/result.rs:198`（`redact_model_feedback`）、`:117`（`other` 构造器由 `kind_ctors!` 生成） |
| 13 | `messages` 表迁移版本应为 v12 | 成立 | 当前最高为 v11（`core/db/repository.rs:474`），迁移风格是 `pragma_table_info` 检查 |
| 14 | `AppConfig.enabled_tools` 默认值缺 `run_workflow` | 成立 | `app/config.rs:212-217` |
| 15 | 工具结果是 `role="tool"` 顶层消息 | **已修正** | Rig 用 `Message::User { content: [UserContent::ToolResult] }`；落库必须按 content 变体遍历（§4.9） |
| 16 | 从 transcript 能区分用户拒绝 / 策略拒绝 | **已修正** | `ToolResult` 只区分成功/错误/跳过/拒绝；新增每轮 `RunPolicyState.decisions` 记录终态（§4.4、§4.8） |

**未验证（留给实现阶段）**：MCP 服务器实际返回的 `is_error` 形态、前端 `activeToolCalls` 的现有合并逻辑、
`code_executor` 的沙箱超时行为。这些都不影响架构选择，按任务里的测试逐条落地。

## 后续变更（2026-11-06）：改用官方 rig-tools

- 内置工具不再由项目实现：`read` / `write` / `edit` / `search` / `list` / `shell` / `delete` / `move`
  全部来自官方 `rig-tools = "0.46"` crate。
- 已移除本地 `calculator` / `file_system` / `web_search` / `code_executor` / `run_workflow`。
- `rig-tools` 自带 `annotations` 与按参数计算的 `footprint`，审批与并行冲突判定保持不变。
- 旧工具名通过注册表 alias 与 `AppConfig` / 意图白名单的归一化映射到新工具：
  `file_system` → 7 个文件工具，`code_executor` → `shell`，其余无替代。
- `shell` 使用 `sh -c`；Windows 需要 Git Bash / WSL 提供 `sh`。
