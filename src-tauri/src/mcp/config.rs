use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize};

// ── Confirmation Mode ──

/// Confirmation mode for a specific MCP tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationMode {
    /// Execute without asking (default for safe tools)
    AutoAllow,
    /// Ask each time
    ConfirmOnce,
    /// Always reject
    Deny,
}

impl Default for ConfirmationMode {
    fn default() -> Self {
        ConfirmationMode::AutoAllow
    }
}

impl ConfirmationMode {
    /// Returns the string representation for frontend usage.
    pub fn as_str(&self) -> &'static str {
        match self {
            ConfirmationMode::AutoAllow => "auto_allow",
            ConfirmationMode::ConfirmOnce => "confirm_once",
            ConfirmationMode::Deny => "deny",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "confirm_once" => ConfirmationMode::ConfirmOnce,
            "deny" => ConfirmationMode::Deny,
            _ => ConfirmationMode::AutoAllow,
        }
    }
}

/// Per-tool configuration for an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolConfig {
    /// Whether the tool is enabled (registered in ToolRegistry)
    #[serde(default = "default_tool_enabled")]
    pub enabled: bool,
    /// Confirmation mode for this tool
    #[serde(default)]
    pub confirmation: ConfirmationMode,
}

fn default_tool_enabled() -> bool {
    true
}

// ── Startup Policy ──

/// Startup strategy for an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartupPolicy {
    /// Startup priority (lower = earlier). None means manual-only.
    #[serde(default)]
    pub priority: Option<u32>,
    /// Extra delay before start in milliseconds (after priority group)
    #[serde(default)]
    pub delay_ms: u64,
    /// Auto-start when the application launches
    #[serde(default = "default_launch_on_startup")]
    pub launch_on_startup: bool,
    /// Start on demand (first tool call triggers connection)
    #[serde(default)]
    pub launch_on_demand: bool,
    /// Max retry attempts on connection failure
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    /// Delay between retries in milliseconds
    #[serde(default = "default_retry_delay_ms")]
    pub retry_delay_ms: u64,
    /// Health check interval in milliseconds (0 = disabled)
    #[serde(default = "default_health_check_interval_ms")]
    pub health_check_interval_ms: u64,
}

fn default_launch_on_startup() -> bool { true }
fn default_max_retries() -> u32 { 3 }
fn default_retry_delay_ms() -> u64 { 2000 }
fn default_health_check_interval_ms() -> u64 { 30000 }

impl Default for StartupPolicy {
    fn default() -> Self {
        Self {
            priority: Some(10),
            delay_ms: 0,
            launch_on_startup: true,
            launch_on_demand: false,
            max_retries: default_max_retries(),
            retry_delay_ms: default_retry_delay_ms(),
            health_check_interval_ms: default_health_check_interval_ms(),
        }
    }
}

impl StartupPolicy {
    /// Whether this server should be auto-connected at startup.
    pub fn should_launch_on_startup(&self) -> bool {
        self.launch_on_startup && self.priority.is_some()
    }
}

// ── Connection Status ──

/// Runtime status of an MCP server connection.
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionStatus {
    /// User disabled this server
    Disabled,
    /// Waiting in startup queue (delay timer)
    Waiting,
    /// Spawning child process + MCP handshake
    Starting,
    /// Connected and tools registered
    Ready,
    /// Connection degraded (health check failing but still alive)
    Degraded,
    /// Gracefully stopping
    Stopping,
    /// Fully stopped/disconnected
    Stopped,
    /// Error state with message
    Error(String),
}

impl ConnectionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConnectionStatus::Disabled => "disabled",
            ConnectionStatus::Waiting => "waiting",
            ConnectionStatus::Starting => "starting",
            ConnectionStatus::Ready => "ready",
            ConnectionStatus::Degraded => "degraded",
            ConnectionStatus::Stopping => "stopping",
            ConnectionStatus::Stopped => "stopped",
            ConnectionStatus::Error(_) => "error",
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(self, ConnectionStatus::Ready | ConnectionStatus::Degraded)
    }

    pub fn error_message(&self) -> Option<&str> {
        match self {
            ConnectionStatus::Error(msg) => Some(msg.as_str()),
            _ => None,
        }
    }
}

// ── MCP Server Config (maintains backward compat with auto_connect) ──

/// MCP server configuration, stored in config.json as part of AppConfig.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Unique identifier for this server connection
    pub id: String,
    /// User-visible name
    pub name: String,
    /// Command to spawn (e.g. "npx", "python", "node")
    pub command: String,
    /// Arguments passed to the command
    pub args: Vec<String>,
    /// Environment variables (optional)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<Vec<(String, String)>>,
    /// Startup policy (replaces auto_connect)
    #[serde(default)]
    pub startup: StartupPolicy,
    /// Legacy: auto-connect on startup (kept for backward compat, use startup instead)
    #[serde(
        default,
        skip_serializing_if = "is_false",
        deserialize_with = "deserialize_auto_connect"
    )]
    pub auto_connect: bool,
    /// Per-tool configuration (tool_name -> config)
    #[serde(default)]
    pub tool_configs: HashMap<String, ToolConfig>,
}

fn is_false(b: &bool) -> bool { !b }

/// Deserialize auto_connect so old configs without `startup` still work.
/// When `startup` is present, we ignore `auto_connect`.
fn deserialize_auto_connect<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let val: serde_json::Value = serde_json::Value::deserialize(deserializer)?;
    Ok(val.as_bool().unwrap_or(false))
}

impl McpServerConfig {
    /// Check if this server should auto-connect (respects new startup policy + legacy auto_connect).
    pub fn should_auto_connect(&self) -> bool {
        // New-style startup policy takes precedence
        if self.startup.launch_on_startup || self.startup.priority.is_some() {
            return true;
        }
        // Fallback to legacy field
        self.auto_connect
    }
}
