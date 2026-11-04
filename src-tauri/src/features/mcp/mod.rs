// ── Sub-modules ──
pub mod bridge;
pub mod config;
pub mod manager;

// ── Re-exports ──
pub use bridge::McpToolBridge;
pub use config::{
    ConfirmationMode, ConnectionStatus, McpServerConfig, StartupPolicy, ToolConfig,
};
pub use manager::{ConnectionInfo, ConnectionStats, McpLogEntry, McpServerManager, McpToolInfo};
