//! 注册表策略层测试：目录元数据、别名归一、白名单语义与调用结果形态。

#![allow(clippy::disallowed_methods)]

use agent_lib::agent::tools::convert::{into_dynamic, ToolEntry};
use agent_lib::agent::tools::registry::ToolRegistry;
use rig::__core::tool::args_schema;
use rig::completion::ToolAnnotations;
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

/// 返回纯文本的工具参数（空对象）。
#[derive(Debug, Clone, Deserialize, JsonSchema)]
struct TextArgs {}

/// 返回纯文本结果的工具，用于验证 `execute()` 不做强制 JSON 解析。
#[derive(Debug, Clone)]
struct TextTool;

impl Tool for TextTool {
    const NAME: &'static str = "text_tool";
    type Args = TextArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Returns plain text".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        args_schema::<TextArgs>()
    }

    fn annotations(&self) -> ToolAnnotations {
        ToolAnnotations::default()
            .with_read_only(true)
            .with_destructive(false)
            .with_open_world(false)
    }

    async fn call(
        &self,
        _context: &mut ToolContext,
        _args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        Ok("plain text".to_string())
    }
}

/// 返回 JSON 文本的工具，用于验证 `execute()` 会解析 JSON。
#[derive(Debug, Clone)]
struct JsonTool;

impl Tool for JsonTool {
    const NAME: &'static str = "json_tool";
    type Args = TextArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Returns JSON text".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        args_schema::<TextArgs>()
    }

    async fn call(
        &self,
        _context: &mut ToolContext,
        _args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        Ok("{\"result\":2}".to_string())
    }
}

#[test]
fn test_catalog_reports_real_metadata() {
    let registry = ToolRegistry::new();

    let read = registry
        .catalog()
        .into_iter()
        .find(|tool| tool.name == "read")
        .expect("应包含 read");

    assert!(
        read.description.to_lowercase().contains("read"),
        "描述应为真实内容: {}",
        read.description
    );
    assert_eq!(read.parameters["type"], "object");
    assert!(read.read_only);
    assert!(!read.destructive);
    assert_eq!(read.source, "builtin");
}

#[test]
fn test_active_normalizes_aliases() {
    let registry = ToolRegistry::new();

    let active: Vec<String> = registry
        .active(Some(&["read_file".to_string()]))
        .iter()
        .map(|entry| entry.name())
        .collect();

    assert_eq!(active, vec!["read".to_string()]);
}

#[test]
fn test_active_empty_allowed_means_none() {
    let registry = ToolRegistry::new();

    assert!(
        registry.active(Some(&[])).is_empty(),
        "空白名单应表示没有工具可用"
    );
    assert_eq!(registry.active(None).len(), 8, "无白名单应返回全部内置工具");
}

#[tokio::test]
async fn test_execute_returns_plain_text_as_is() {
    let mut registry = ToolRegistry::new();
    registry.register("text_tool", into_dynamic(TextTool), true);

    let value = registry
        .execute("text_tool", json!({}))
        .await
        .expect("纯文本工具应成功");

    assert_eq!(value, json!("plain text"));
}

#[tokio::test]
async fn test_execute_parses_json_tool_output() {
    let mut registry = ToolRegistry::new();
    registry.register("json_tool", into_dynamic(JsonTool), true);

    let value = registry
        .execute("json_tool", json!({}))
        .await
        .expect("JSON 工具应成功");

    assert_eq!(value["result"], json!(2));
}

#[test]
fn test_builtin_enabled_follows_config_but_mcp_not() {
    let mut registry = ToolRegistry::new();
    registry.register_mcp("srv", "srv__custom", into_dynamic(TextTool), true);

    registry.apply_builtin_enabled(&["read".to_string()]);

    assert!(registry.is_enabled("read"), "配置里的内置工具应启用");
    assert!(
        !registry.is_enabled("write"),
        "不在配置里的内置工具应禁用"
    );
    assert!(
        registry.is_enabled("srv__custom"),
        "MCP 工具的启用状态不应被内置配置覆盖"
    );
}

/// 构造一个指定注册名的动态工具，用来模拟 MCP 工具。
fn named_text_tool(name: &str) -> ToolEntry {
    ToolEntry::from_dynamic(rig::tool::DynamicTool::new(
        rig::message::ToolName::new(name).expect("合法工具名"),
        "text tool",
        json!({ "type": "object" }),
        |_args| Box::pin(async { Ok(rig::tool::ToolOutput::text("ok")) }),
    ))
}

#[test]
fn test_mcp_tool_is_namespaced_and_coexists_with_builtin() {
    let mut registry = ToolRegistry::new();
    registry.register_mcp("srv", "srv__read", named_text_tool("srv__read"), true);

    assert!(
        registry.is_registered("read"),
        "内置工具不应被 MCP 同名工具覆盖"
    );
    assert!(
        registry.is_registered("srv__read"),
        "MCP 工具应以 server__tool 注册"
    );

    let catalog = registry.catalog();
    let mcp = catalog
        .iter()
        .find(|tool| tool.name == "srv__read")
        .expect("目录应含 MCP 工具");
    assert_eq!(mcp.source, "mcp:srv");
}

#[test]
fn test_unregistering_mcp_keeps_builtin_tools() {
    let mut registry = ToolRegistry::new();
    registry.register_mcp("srv", "srv__custom", named_text_tool("srv__custom"), true);

    registry.unregister("srv__custom");

    assert!(!registry.is_registered("srv__custom"));
    assert!(registry.is_registered("read"));
    assert!(registry.is_registered("write"));
}
