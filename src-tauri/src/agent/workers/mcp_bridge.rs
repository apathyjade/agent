//! MCP 桥接 worker：让模型发现并调用已注册的 MCP 工具。
//!
//! 命令语法：
//! - `@@list_tools` 或 `@@list_tools("server?")` —— 列出工具
//! - `@@call("name", {...})` —— 带 JSON 参数调用工具
//!
//! MCP 工具由 `McpServerManager` 以 `McpToolWrapper` 的形式注册进全局
//! `ToolRegistry`，因此本 worker 不直接持有 MCP 连接。

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::llm::provider::ProviderRegistry;
use crate::core::error::Result;
use crate::agent::tools::registry::ToolRegistry;
use crate::agent::workers::{SubTask, WorkerAgent, WorkerKind, WorkerResult};

const MCP_SYSTEM_PROMPT: &str = r#"You are an MCP (Model Context Protocol) bridge agent. Your job is to
help users interact with tools exposed by connected MCP servers.

Use the @@list_tools command to discover available tools, and @@call to
invoke them with JSON parameters. Tool results will be provided below.

Analyze the results and explain them to the user."#;

/// 通过全局 `ToolRegistry` 桥接 MCP 工具的 worker。
///
/// MCP 工具由 `McpServerManager` 以 `McpToolWrapper` 的形式注册；本 worker 提供
/// `@@list_tools` 与 `@@call` 两个命令用于发现和调用它们。
pub struct MCPBridgeWorker {
    providers: Arc<Mutex<ProviderRegistry>>,
    tools: Arc<Mutex<ToolRegistry>>,
}

impl MCPBridgeWorker {
    /// 用共享的 provider 注册表与工具注册表构造 worker。
    pub fn new(
        providers: Arc<Mutex<ProviderRegistry>>,
        tools: Arc<Mutex<ToolRegistry>>,
    ) -> Self {
        Self { providers, tools }
    }

    /// 把已注册工具及其启用状态追加到结果文本。
    async fn append_tool_list(&self, out: &mut String) {
        let registry = self.tools.lock().await;
        let tools = registry.list();
        out.push_str("## Available Tools\n");
        for tool in &tools {
            let status = if tool.enabled { "enabled" } else { "disabled" };
            out.push_str(&format!(
                "- **{}**: {} ({})\n",
                tool.name, tool.description, status
            ));
        }
        if tools.is_empty() {
            out.push_str("(no tools registered)\n");
        }
    }

    /// 处理 `@@call("tool_name", {...})`：解析参数并调用工具。
    async fn handle_call(&self, inner: &str, out: &mut String) {
        if let Some(comma_pos) = inner.find(',') {
            let tool_name = inner[..comma_pos].trim().trim_matches('"');
            let params_str = inner[comma_pos + 1..].trim();
            let params = parse_tool_params(params_str);

            match self.call_tool(tool_name, params).await {
                Ok(output) => {
                    out.push_str(&format!(
                        "## Tool Call: {}\n```\n{}\n```\n",
                        tool_name, output
                    ));
                }
                Err(e) => {
                    out.push_str(&format!(
                        "## Tool Call Error: {}\n```\n{}\n```\n",
                        tool_name, e
                    ));
                }
            }
        } else {
            out.push_str(
                "Invalid @@call syntax: expected @@call(\"tool_name\", {...})\n",
            );
        }
    }

    /// 解析并执行指令中内嵌的 `@@` 命令。
    ///
    /// 支持的命令见模块文档；未识别的行会被忽略。
    async fn execute_tools(&self, instruction: &str) -> String {
        let mut result = String::new();

        for line in instruction.lines() {
            let trimmed = line.trim();

            if trimmed == "@@list_tools" {
                self.append_tool_list(&mut result).await;
            } else if trimmed.starts_with("@@list_tools(") && trimmed.ends_with(')') {
                // 服务器过滤是 best-effort：ToolRegistry 不记录工具来源，先列出全部。
                self.append_tool_list(&mut result).await;
            } else if trimmed.starts_with("@@call(") && trimmed.ends_with(')') {
                self.handle_call(&trimmed[7..trimmed.len() - 1], &mut result)
                    .await;
            }
        }

        result
    }

    /// 按名称调用工具，并把结果格式化为易读的 JSON。
    async fn call_tool(&self, name: &str, params: serde_json::Value) -> Result<String> {
        let registry = self.tools.lock().await;
        let value = registry.execute(name, params).await?;
        Ok(serde_json::to_string_pretty(&value)
            .unwrap_or_else(|_| format!("{:?}", value)))
    }
}

/// 解析 `@@call` 的参数字符串。
///
/// 空串或 `{}` 视为空对象；不是合法 JSON 时尝试把裸值包装成
/// `{"value": ...}`，仍失败则回落到空对象。
fn parse_tool_params(params_str: &str) -> serde_json::Value {
    serde_json::from_str(params_str).unwrap_or_else(|_| {
        if params_str.is_empty() || params_str == "{}" {
            serde_json::Value::Object(Default::default())
        } else {
            serde_json::from_str(&format!(r#"{{"value": {}}}"#, params_str))
                .unwrap_or(serde_json::Value::Object(Default::default()))
        }
    })
}

#[async_trait]
impl WorkerAgent for MCPBridgeWorker {
    fn kind(&self) -> WorkerKind {
        WorkerKind::McpBridge
    }

    fn description(&self) -> &str {
        "Bridges to MCP (Model Context Protocol) tools for listing and calling tools exposed by connected MCP servers."
    }

    async fn execute(&self, task: SubTask) -> Result<WorkerResult> {
        let start = std::time::Instant::now();

        // 先执行指令里内嵌的 @@ 命令，再把结果作为上下文交给模型。
        let tool_results = self.execute_tools(&task.instruction).await;

        let provider = {
            let registry = self.providers.lock().await;
            let mid = task
                .model_id
                .as_deref()
                .unwrap_or_else(|| registry.default_model_id());
            if mid.is_empty() {
                return Err(crate::core::error::AppError::Worker(
                    "No model configured for MCPBridgeWorker".into(),
                ));
            }
            registry.get(mid)?
        };

        let system_prompt = format!(
            "{}\n\n## Tool Results\n{}",
            MCP_SYSTEM_PROMPT,
            if tool_results.is_empty() {
                "No @@ commands found in instruction.".to_string()
            } else {
                tool_results
            }
        );

        let content = provider
            .prompt(&system_prompt, &task.instruction)
            .await?;

        Ok(WorkerResult {
            worker: WorkerKind::McpBridge,
            task_id: task.id,
            content,
            metadata: None,
            duration_ms: Some(start.elapsed().as_millis() as u64),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_bridge_kind() {
        let providers = Arc::new(Mutex::new(
            crate::llm::provider::ProviderRegistry::new(&[]),
        ));
        let tools = Arc::new(Mutex::new(ToolRegistry::new()));
        let worker = MCPBridgeWorker::new(providers, tools);
        assert_eq!(worker.kind(), WorkerKind::McpBridge);
        assert!(!worker.description().is_empty());
    }

    #[tokio::test]
    async fn test_execute_tools_list() {
        let providers = Arc::new(Mutex::new(
            crate::llm::provider::ProviderRegistry::new(&[]),
        ));
        let tools = Arc::new(Mutex::new(ToolRegistry::new()));
        let worker = MCPBridgeWorker::new(providers, tools);

        let result = worker.execute_tools("@@list_tools").await;
        assert!(result.contains("Available Tools"), "should list tools");
        // 内置工具应已注册。
        assert!(result.contains("read") || result.contains("write"),
            "should contain some registered tools, got: {}", result);
    }

    #[tokio::test]
    async fn test_execute_tools_list_with_server() {
        let providers = Arc::new(Mutex::new(
            crate::llm::provider::ProviderRegistry::new(&[]),
        ));
        let tools = Arc::new(Mutex::new(ToolRegistry::new()));
        let worker = MCPBridgeWorker::new(providers, tools);

        let result = worker.execute_tools("@@list_tools(\"my_server\")").await;
        assert!(result.contains("Available Tools"), "should list tools");
    }

    #[tokio::test]
    async fn test_execute_tools_call_invalid_syntax() {
        let providers = Arc::new(Mutex::new(
            crate::llm::provider::ProviderRegistry::new(&[]),
        ));
        let tools = Arc::new(Mutex::new(ToolRegistry::new()));
        let worker = MCPBridgeWorker::new(providers, tools);

        // 缺少工具名与参数之间的逗号。
        let result = worker.execute_tools("@@call(\"bad_syntax\")").await;
        assert!(result.contains("Invalid @@call syntax"), "should report syntax error");
    }

    #[tokio::test]
    async fn test_execute_tools_no_commands() {
        let providers = Arc::new(Mutex::new(
            crate::llm::provider::ProviderRegistry::new(&[]),
        ));
        let tools = Arc::new(Mutex::new(ToolRegistry::new()));
        let worker = MCPBridgeWorker::new(providers, tools);

        let result = worker.execute_tools("Just a plain instruction.").await;
        assert!(result.is_empty(), "should return empty when no @@ commands");
    }

    #[tokio::test]
    async fn test_execute_tools_call_unknown_tool() {
        let providers = Arc::new(Mutex::new(
            crate::llm::provider::ProviderRegistry::new(&[]),
        ));
        let tools = Arc::new(Mutex::new(ToolRegistry::new()));
        let worker = MCPBridgeWorker::new(providers, tools);

        let result = worker.execute_tools("@@call(\"nonexistent_tool\", {})").await;
        assert!(result.contains("Tool Call Error"), "should report error for unknown tool");
        assert!(result.contains("nonexistent_tool"), "should mention tool name");
    }
}
