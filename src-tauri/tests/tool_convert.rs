//! 转换层测试：类型化工具 → `DynamicTool` + footprint 解析器。

#![allow(clippy::disallowed_methods)]

use std::path::PathBuf;

use agent_lib::agent::tools::convert::into_dynamic;
use rig::__core::tool::{args_schema, Scope, Touches};
use rig::completion::ToolAnnotations;
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use rig_tools::{Read, Write};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

/// 回显工具的参数。
#[derive(Debug, Clone, Deserialize, JsonSchema)]
struct EchoArgs {
    #[schemars(description = "Text to echo back")]
    message: String,
}

/// 最小的只读工具，用于验证转换层本身。
#[derive(Debug, Clone)]
struct EchoTool;

impl Tool for EchoTool {
    const NAME: &'static str = "echo";
    type Args = EchoArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Echo the message back".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        args_schema::<EchoArgs>()
    }

    fn annotations(&self) -> ToolAnnotations {
        ToolAnnotations::default()
            .with_read_only(true)
            .with_destructive(false)
            .with_open_world(true)
    }

    async fn call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        Ok(args.message)
    }
}

/// 按参数声明读 / 写范围的工具。
#[derive(Debug, Clone, Deserialize, JsonSchema)]
struct TouchArgs {
    #[schemars(description = "Absolute path the call touches")]
    path: String,
    #[schemars(description = "Whether the call writes the path")]
    write: bool,
}

/// 声明了 footprint 的工具。
#[derive(Debug, Clone)]
struct TouchTool;

impl Tool for TouchTool {
    const NAME: &'static str = "touch";
    type Args = TouchArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Touch a path".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        args_schema::<TouchArgs>()
    }

    fn footprint(&self, args: &Self::Args) -> Touches {
        let scope = Scope::Paths(vec![PathBuf::from(&args.path)]);
        if args.write {
            Touches::Writes(scope)
        } else {
            Touches::Reads(scope)
        }
    }

    async fn call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        Ok(args.path)
    }
}

/// 总是失败的工具，用来确认错误文本不会被脱敏。
#[derive(Debug, Clone)]
struct BrokenTool;

impl Tool for BrokenTool {
    const NAME: &'static str = "broken";
    type Args = serde_json::Value;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Always fails".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({ "type": "object" })
    }

    async fn call(
        &self,
        _context: &mut ToolContext,
        _args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        Err(ToolExecutionError::other("disk exploded"))
    }
}

#[test]
fn test_into_dynamic_keeps_definition() {
    let entry = into_dynamic(EchoTool);

    let definition = entry.tool.definition();
    assert_eq!(definition.name.as_str(), "echo");
    assert_eq!(definition.description, "Echo the message back");

    let parameters = &definition.parameters;
    assert_eq!(parameters["type"], "object");
    assert_eq!(parameters["properties"]["message"]["type"], "string");
    let required = parameters["required"].as_array().expect("required 应为数组");
    assert!(required.iter().any(|v| v == "message"));
}

#[test]
fn test_into_dynamic_keeps_annotations() {
    let entry = into_dynamic(EchoTool);

    let annotations = entry.tool.definition().annotations;
    assert!(annotations.read_only());
    assert!(!annotations.destructive());
    assert!(annotations.open_world());
}

#[test]
fn test_footprint_reads_and_writes_by_args() {
    let entry = into_dynamic(TouchTool);

    let read = entry
        .footprint_for(&json!({ "path": "/work/a.txt", "write": false }))
        .expect("读参数应能解析");
    assert_eq!(
        read,
        Touches::Reads(Scope::Paths(vec![PathBuf::from("/work/a.txt")]))
    );

    let write = entry
        .footprint_for(&json!({ "path": "/work/a.txt", "write": true }))
        .expect("写参数应能解析");
    assert_eq!(
        write,
        Touches::Writes(Scope::Paths(vec![PathBuf::from("/work/a.txt")]))
    );
}

#[test]
fn test_footprint_is_none_when_args_invalid() {
    let entry = into_dynamic(TouchTool);
    assert!(entry.footprint_for(&json!({ "path": 1 })).is_none());
}

#[tokio::test]
async fn test_error_is_not_redacted() {
    let entry = into_dynamic(BrokenTool);

    let error = entry
        .tool
        .run(json!({}))
        .await
        .expect_err("工具应失败");
    assert_eq!(error.model_output().as_text(), Some("disk exploded"));
}

#[test]
fn test_rig_tools_read_keeps_annotations_and_footprint() {
    let entry = into_dynamic(Read::default());
    let definition = entry.tool.definition();

    assert_eq!(definition.name.as_str(), "read");
    assert!(definition.annotations.read_only());
    assert!(!definition.annotations.destructive());

    let footprint = entry
        .footprint_for(&json!({ "path": "Cargo.toml" }))
        .expect("read 应能算出 footprint");
    match footprint {
        Touches::Reads(Scope::Paths(paths)) => {
            assert!(paths[0].ends_with("Cargo.toml"), "路径应为 {:?}", paths);
        }
        other => panic!("read 应为 Reads(Paths)，实际 {:?}", other),
    }
}

#[test]
fn test_rig_tools_write_is_destructive_and_writes_path() {
    let entry = into_dynamic(Write::default());
    let definition = entry.tool.definition();

    assert_eq!(definition.name.as_str(), "write");
    assert!(definition.annotations.destructive());
    assert!(!definition.annotations.read_only());

    let footprint = entry
        .footprint_for(&json!({ "path": "target/footprint-probe.txt", "content": "x" }))
        .expect("write 应能算出 footprint");
    match footprint {
        Touches::Writes(Scope::Paths(paths)) => {
            assert!(
                paths[0].ends_with("footprint-probe.txt"),
                "路径应为 {:?}",
                paths
            );
        }
        other => panic!("write 应为 Writes(Paths)，实际 {:?}", other),
    }
}

