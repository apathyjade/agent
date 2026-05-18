use async_trait::async_trait;
use serde_json::Value;

use crate::error::Result;

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> Value;
    async fn execute(&self, input: Value) -> Result<Value>;
}

pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub enabled: bool,
}
