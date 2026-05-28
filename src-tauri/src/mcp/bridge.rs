use std::future::Future;
use std::pin::Pin;
use serde_json::Value;

use rig::completion::ToolDefinition;
use rig::tool::{ToolDyn, ToolError};

/// A tool exposed by an MCP server, wrapped to implement ToolDyn.
pub struct McpToolBridge {
    connection_id: String,
    tool_name: String,
    tool_description: String,
    input_schema: Value,
}

impl McpToolBridge {
    pub fn new(
        connection_id: impl Into<String>,
        tool_name: impl Into<String>,
        tool_description: impl Into<String>,
        input_schema: Value,
    ) -> Self {
        Self {
            connection_id: connection_id.into(),
            tool_name: tool_name.into(),
            tool_description: tool_description.into(),
            input_schema,
        }
    }

    pub fn connection_id(&self) -> &str {
        &self.connection_id
    }
}

impl ToolDyn for McpToolBridge {
    fn name(&self) -> String {
        self.tool_name.clone()
    }

    fn definition<'a>(
        &'a self,
        _prompt: String,
    ) -> Pin<Box<dyn Future<Output = ToolDefinition> + Send + 'a>> {
        Box::pin(async move {
            ToolDefinition {
                name: self.tool_name.clone(),
                description: self.tool_description.clone(),
                parameters: self.input_schema.clone(),
            }
        })
    }

    fn call<'a>(
        &'a self,
        _args: String,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<String, ToolError>> + Send + 'a>>
    {
        Box::pin(async move {
            Err(ToolError::ToolCallError(
                format!(
                    "MCP tool '{}' on connection '{}' executed without connection context. \
                     This bridge is for metadata only.",
                    self.tool_name, self.connection_id
                )
                .into(),
            ))
        })
    }
}
