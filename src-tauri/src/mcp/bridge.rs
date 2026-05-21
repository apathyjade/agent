use async_trait::async_trait;
use serde_json::Value;

use crate::error::Result;
use crate::tools::r#trait::Tool;

/// A tool exposed by an MCP server, wrapped to implement our Tool trait.
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

#[async_trait]
impl Tool for McpToolBridge {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.tool_description
    }

    fn parameters(&self) -> Value {
        self.input_schema.clone()
    }

    async fn execute(&self, _input: Value) -> Result<Value> {
        Err(crate::error::AppError::Tool(format!(
            "MCP tool '{}' on connection '{}' executed without connection context. \
             This bridge is for metadata only.",
            self.tool_name, self.connection_id
        )))
    }
}
