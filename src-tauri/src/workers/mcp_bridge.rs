use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::api::provider::ProviderRegistry;
use crate::error::Result;
use crate::tools::registry::ToolRegistry;
use crate::workers::{SubTask, WorkerAgent, WorkerKind, WorkerResult};

const MCP_SYSTEM_PROMPT: &str = r#"You are an MCP (Model Context Protocol) bridge agent. Your job is to
help users interact with tools exposed by connected MCP servers.

Use the @@list_tools command to discover available tools, and @@call to
invoke them with JSON parameters. Tool results will be provided below.

Analyze the results and explain them to the user."#;

/// Worker that bridges to MCP tools via the global ToolRegistry.
///
/// MCP tools are registered in the ToolRegistry by `McpServerManager`
/// as `McpToolWrapper` instances. This worker provides `@@list_tools`
/// and `@@call` commands for discovering and invoking them.
pub struct MCPBridgeWorker {
    providers: Arc<Mutex<ProviderRegistry>>,
    tools: Arc<Mutex<ToolRegistry>>,
}

impl MCPBridgeWorker {
    pub fn new(
        providers: Arc<Mutex<ProviderRegistry>>,
        tools: Arc<Mutex<ToolRegistry>>,
    ) -> Self {
        Self { providers, tools }
    }

    /// Parse and execute @@ commands embedded in the instruction.
    ///
    /// Supported commands:
    /// - `@@list_tools` or `@@list_tools("server?")` — list tools
    /// - `@@call("name", {...})` — call a tool with JSON params
    async fn execute_tools(&self, instruction: &str) -> String {
        let mut result = String::new();

        for line in instruction.lines() {
            let trimmed = line.trim();

            if trimmed == "@@list_tools" {
                // List all tools (no server filter)
                let registry = self.tools.lock().await;
                let tools = registry.list();
                result.push_str("## Available Tools\n");
                for tool in &tools {
                    let status = if tool.enabled { "enabled" } else { "disabled" };
                    result.push_str(&format!(
                        "- **{}**: {} ({})\n",
                        tool.name, tool.description, status
                    ));
                }
                if tools.is_empty() {
                    result.push_str("(no tools registered)\n");
                }
            } else if trimmed.starts_with("@@list_tools(") && trimmed.ends_with(')') {
                // @@list_tools("server_name") — server filter is best-effort
                // Since ToolRegistry doesn't track server origin, list all tools
                let _server_filter = &trimmed[14..trimmed.len() - 1]
                    .trim()
                    .trim_matches('"');
                let registry = self.tools.lock().await;
                let tools = registry.list();
                result.push_str("## Available Tools\n");
                for tool in &tools {
                    let status = if tool.enabled { "enabled" } else { "disabled" };
                    result.push_str(&format!(
                        "- **{}**: {} ({})\n",
                        tool.name, tool.description, status
                    ));
                }
                if tools.is_empty() {
                    result.push_str("(no tools registered)\n");
                }
            } else if trimmed.starts_with("@@call(") && trimmed.ends_with(')') {
                // @@call("tool_name", {"key": "value", ...})
                let inner = &trimmed[7..trimmed.len() - 1];
                if let Some(comma_pos) = inner.find(',') {
                    let tool_name = inner[..comma_pos].trim().trim_matches('"');
                    let params_str = inner[comma_pos + 1..].trim();

                    let params: serde_json::Value = serde_json::from_str(params_str)
                        .unwrap_or_else(|_| {
                            // If params_str is empty or not valid JSON, use empty object
                            if params_str.is_empty() || params_str == "{}" {
                                serde_json::Value::Object(Default::default())
                            } else {
                                // Try wrapping bare string as value
                                serde_json::from_str(&format!(r#"{{"value": {}}}"#, params_str))
                                    .unwrap_or(serde_json::Value::Object(Default::default()))
                            }
                        });

                    match self.call_tool(tool_name, params).await {
                        Ok(output) => {
                            result.push_str(&format!(
                                "## Tool Call: {}\n```\n{}\n```\n",
                                tool_name, output
                            ));
                        }
                        Err(e) => {
                            result.push_str(&format!(
                                "## Tool Call Error: {}\n```\n{}\n```\n",
                                tool_name, e
                            ));
                        }
                    }
                } else {
                    result.push_str(&format!(
                        "Invalid @@call syntax: expected @@call(\"tool_name\", {{...}})\n"
                    ));
                }
            }
        }

        result
    }

    /// Call a tool by name with the given JSON parameters.
    async fn call_tool(&self, name: &str, params: serde_json::Value) -> Result<String> {
        let registry = self.tools.lock().await;
        let value = registry.execute(name, params).await?;
        Ok(serde_json::to_string_pretty(&value)
            .unwrap_or_else(|_| format!("{:?}", value)))
    }
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

        // Execute any @@ commands embedded in the instruction
        let tool_results = self.execute_tools(&task.instruction).await;

        let provider = {
            let registry = self.providers.lock().await;
            let mid = task
                .model_id
                .as_deref()
                .unwrap_or_else(|| registry.default_model_id());
            if mid.is_empty() {
                return Err(crate::error::AppError::Worker(
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
            crate::api::provider::ProviderRegistry::new(
                &crate::config::AppConfig::default(),
            ),
        ));
        let tools = Arc::new(Mutex::new(ToolRegistry::new()));
        let worker = MCPBridgeWorker::new(providers, tools);
        assert_eq!(worker.kind(), WorkerKind::McpBridge);
        assert!(!worker.description().is_empty());
    }

    #[tokio::test]
    async fn test_execute_tools_list() {
        let providers = Arc::new(Mutex::new(
            crate::api::provider::ProviderRegistry::new(
                &crate::config::AppConfig::default(),
            ),
        ));
        let tools = Arc::new(Mutex::new(ToolRegistry::new()));
        let worker = MCPBridgeWorker::new(providers, tools);

        let result = worker.execute_tools("@@list_tools").await;
        assert!(result.contains("Available Tools"), "should list tools");
        // Standard tools should be registered
        assert!(result.contains("calculator") || result.contains("web_search"),
            "should contain some registered tools, got: {}", result);
    }

    #[tokio::test]
    async fn test_execute_tools_list_with_server() {
        let providers = Arc::new(Mutex::new(
            crate::api::provider::ProviderRegistry::new(
                &crate::config::AppConfig::default(),
            ),
        ));
        let tools = Arc::new(Mutex::new(ToolRegistry::new()));
        let worker = MCPBridgeWorker::new(providers, tools);

        let result = worker.execute_tools("@@list_tools(\"my_server\")").await;
        assert!(result.contains("Available Tools"), "should list tools");
    }

    #[tokio::test]
    async fn test_execute_tools_call_invalid_syntax() {
        let providers = Arc::new(Mutex::new(
            crate::api::provider::ProviderRegistry::new(
                &crate::config::AppConfig::default(),
            ),
        ));
        let tools = Arc::new(Mutex::new(ToolRegistry::new()));
        let worker = MCPBridgeWorker::new(providers, tools);

        // Missing comma between name and params
        let result = worker.execute_tools("@@call(\"bad_syntax\")").await;
        assert!(result.contains("Invalid @@call syntax"), "should report syntax error");
    }

    #[tokio::test]
    async fn test_execute_tools_no_commands() {
        let providers = Arc::new(Mutex::new(
            crate::api::provider::ProviderRegistry::new(
                &crate::config::AppConfig::default(),
            ),
        ));
        let tools = Arc::new(Mutex::new(ToolRegistry::new()));
        let worker = MCPBridgeWorker::new(providers, tools);

        let result = worker.execute_tools("Just a plain instruction.").await;
        assert!(result.is_empty(), "should return empty when no @@ commands");
    }

    #[tokio::test]
    async fn test_execute_tools_call_unknown_tool() {
        let providers = Arc::new(Mutex::new(
            crate::api::provider::ProviderRegistry::new(
                &crate::config::AppConfig::default(),
            ),
        ));
        let tools = Arc::new(Mutex::new(ToolRegistry::new()));
        let worker = MCPBridgeWorker::new(providers, tools);

        let result = worker.execute_tools("@@call(\"nonexistent_tool\", {})").await;
        assert!(result.contains("Tool Call Error"), "should report error for unknown tool");
        assert!(result.contains("nonexistent_tool"), "should mention tool name");
    }
}
