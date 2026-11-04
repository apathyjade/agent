use std::future::Future;
use std::pin::Pin;

use rig::tool::ToolExecutionError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Error type for the project's own tool implementations.
///
/// Mirrors the shape Rig used before 0.45 (`JsonError` / `ToolCallError`) so
/// tool bodies stay readable; [`ToolExecutionError`] is produced once, at the
/// boundary where a tool is handed to Rig.
#[derive(Debug)]
pub enum ToolError {
    /// Arguments could not be decoded as JSON.
    JsonError(serde_json::Error),
    /// The tool itself failed while executing.
    ToolCallError(Box<dyn std::error::Error + Send + Sync>),
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::JsonError(e) => write!(f, "invalid tool arguments: {}", e),
            Self::ToolCallError(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for ToolError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::JsonError(e) => Some(e),
            Self::ToolCallError(e) => Some(e.as_ref()),
        }
    }
}

impl From<ToolError> for ToolExecutionError {
    fn from(error: ToolError) -> Self {
        match error {
            ToolError::JsonError(e) => {
                ToolExecutionError::invalid_args(format!("invalid tool arguments: {}", e))
            }
            ToolError::ToolCallError(e) => ToolExecutionError::other(e.to_string()),
        }
    }
}

/// Metadata describing a tool to the model.
#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// The project's tool contract.
///
/// Rig 0.45 removed the object-safe `ToolDyn` trait: tools are either typed
/// [`Tool`](rig::tool::Tool) implementations or erased
/// [`DynamicTool`](rig::tool::DynamicTool) values. The project needs a
/// homogeneous, storable collection (built-in tools plus MCP tools), so it
/// keeps its own object-safe contract and converts to [`DynamicTool`] when a
/// tool set is handed to Rig — see
/// [`ToolRegistry::to_rig_tools`](crate::agent::tools::registry::ToolRegistry::to_rig_tools).
pub trait AgentTool: Send + Sync {
    /// Unique registration and provider-facing name.
    fn name(&self) -> String;

    /// Model-facing description and JSON Schema for the arguments.
    fn definition(&self) -> ToolSpec;

    /// Run the tool on a JSON-encoded argument object, returning the
    /// JSON-encoded result.
    fn call<'a>(
        &'a self,
        args: String,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<String, ToolError>> + Send + 'a>>;
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub enabled: bool,
}
