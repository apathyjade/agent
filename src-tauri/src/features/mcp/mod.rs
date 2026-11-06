//! MCP 服务器接入层。
//!
//! `config` 定义持久化到 `config.json` 的服务器配置与运行时状态枚举；
//! `manager` 负责子进程生命周期、MCP 握手、工具注册与健康检查。

pub mod config;
pub mod manager;

pub use config::{
    ConfirmationMode, ConnectionStatus, McpServerConfig, StartupPolicy, ToolConfig,
};
pub use manager::{ConnectionInfo, ConnectionStats, McpLogEntry, McpServerManager, McpToolInfo};
