//! 类型化工具 → 动态工具的转换层。
//!
//! 项目只保留一种编写形态（[`rig::tool::Tool`]）与一种运行时形态
//! （[`rig::tool::DynamicTool`]）。本模块负责把前者转成后者，并**额外**产出一个
//! footprint 解析器：`DynamicTool` 的 `Serve::footprint` 默认返回 `None`，
//! 策略层（审批、并行冲突判定）拿不到本次调用碰了什么，只能自己保存一份。

use std::sync::Arc;

use rig::__core::tool::Touches;
use rig::completion::ToolAnnotations;
use rig::tool::{tool_definition, DynamicTool, Tool, ToolContext};

/// 由 JSON 参数算出本次调用碰到的东西；参数解析失败时返回 `None`。
///
/// 失败时**不**退回注解推导：调用级信息缺失时交给策略层的 `destructive` 兜底，
/// 比在这里假装「只读」安全。
pub use crate::llm::provider::FootprintFn;

/// 注册表存放的工具条目。
///
/// 交给 Rig 执行的始终是 [`Self::tool`]；[`Self::footprint`] 只服务策略层。
#[derive(Clone)]
pub struct ToolEntry {
    /// 可存储、可跨路径复用的动态工具。
    pub tool: DynamicTool,
    /// 该工具的 footprint 解析器。
    pub footprint: Option<FootprintFn>,
}

impl ToolEntry {
    /// 包装一个已构造好的动态工具，不提供 footprint。
    ///
    /// 适用于运行时才知道结构的工具（MCP）：它们的范围只能从服务器给的注解推。
    pub fn from_dynamic(tool: DynamicTool) -> Self {
        Self {
            tool,
            footprint: None,
        }
    }

    /// 按参数算出本次调用的范围。
    pub fn footprint_for(&self, args: &serde_json::Value) -> Option<Touches> {
        self.footprint.as_ref().and_then(|f| f(args))
    }

    /// 工具注册名。
    pub fn name(&self) -> String {
        self.tool.definition().name.as_str().to_string()
    }
}

/// 把类型化工具转成可存储、可跨路径复用的动态工具。
///
/// `T` 需要 `Clone`：`ToolAdapter::new` 按值消费工具，而 footprint 闭包还要用
/// 同一个工具。内置工具都是无状态的 unit struct，克隆没有成本。
pub fn into_dynamic<T>(tool: T) -> ToolEntry
where
    T: Tool + Clone + 'static,
{
    let annotations = tool.annotations();

    // 先克隆一份留给 footprint，再把原值交给 adapter。
    let footprint_tool = tool.clone();
    let footprint: FootprintFn = Arc::new(move |args: &serde_json::Value| {
        serde_json::from_value::<T::Args>(args.clone())
            .ok()
            .map(|parsed| footprint_tool.footprint(&parsed))
    });

    into_dynamic_with(tool, annotations, Some(footprint))
}

/// 把类型化工具转成动态工具，并显式指定策略元数据。
///
/// `#[rig_tool]` 生成的 `Tool` 实现没有 `annotations` / `footprint` 覆盖，
/// 直接用会退回「destructive = true」的默认值，导致只读工具也弹审批。
/// 工具模块通过 `tool_entry()` 在这里补上真实的注解与 footprint。
pub fn into_dynamic_with<T>(
    tool: T,
    annotations: ToolAnnotations,
    footprint: Option<FootprintFn>,
) -> ToolEntry
where
    T: Tool + 'static,
{
    let mut definition = tool_definition::<T>(&tool);
    definition.annotations = annotations.clone();

    // 没有显式 footprint 时按注解推导：只读 → Reads(Anything)，否则 Writes(Anything)。
    // 参数解析失败返回 `None`，交给策略层用同一份注解兜底。
    let footprint = footprint.or_else(|| {
        let annotations = annotations.clone();
        let derived: FootprintFn = Arc::new(move |args: &serde_json::Value| {
            serde_json::from_value::<T::Args>(args.clone())
                .ok()
                .map(|_| Touches::from_annotations(&annotations))
        });
        Some(derived)
    });

    let adapter = Arc::new(rig::__core::serve::adapters::ToolAdapter::new(tool));
    let dynamic = DynamicTool::new_with_context(
        definition.name.clone(),
        definition.description.clone(),
        definition.parameters.clone(),
        move |context: &mut ToolContext, args: serde_json::Value| {
            let adapter = adapter.clone();
            Box::pin(async move { adapter.run_with(context, args).await })
        },
    )
    // `new_with_context` 只构造 `ToolDefinition::new(...)`，不带注解，
    // 必须显式补上，否则策略层拿不到风险信息。
    .with_annotations(definition.annotations.clone());

    ToolEntry {
        tool: dynamic,
        footprint,
    }
}
