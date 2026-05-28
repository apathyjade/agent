use serde::{Deserialize, Serialize};
use serde_json::Value;

// Re-export Rig's ToolDyn trait for use by the rest of the codebase
pub use rig::tool::ToolDyn;

#[derive(Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub enabled: bool,
}
