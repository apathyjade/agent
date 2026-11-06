//! 工具的前端展示信息。
//!
//! 工具契约已统一到 [`rig::tool::Tool`]；这里只保留给前端 / 规划器看的元数据。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 工具的前端展示信息。
#[derive(Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    /// 注册名。
    pub name: String,
    /// 面向模型的描述。
    pub description: String,
    /// 参数 JSON Schema。
    pub parameters: Value,
    /// 是否对模型可见。
    pub enabled: bool,
    /// 是否只读。
    pub read_only: bool,
    /// 是否可能破坏数据。
    pub destructive: bool,
    /// 是否访问外部资源。
    pub open_world: bool,
    /// 来源：`builtin` 或 `mcp:<server>`。
    pub source: String,
}
