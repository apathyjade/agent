//! MCP 服务器配置与状态枚举。
//!
//! 配置随 `AppConfig` 一起持久化到 `config.json`；旧版本只有
//! `auto_connect` 布尔字段，新版本用 `startup` 策略描述优先级、延迟与重试，
//! 反序列化时两者都兼容。

use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize};

use crate::runtime::toolchain::RuntimeType;

// ── 确认模式 ──

/// 单个 MCP 工具的确认模式。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationMode {
    /// 直接执行，不询问（安全工具的默认值）
    #[default]
    AutoAllow,
    /// 每次调用都询问
    ConfirmOnce,
    /// 始终拒绝
    Deny,
}

impl ConfirmationMode {
    /// 返回供前端使用的字符串表示。
    pub fn as_str(&self) -> &'static str {
        match self {
            ConfirmationMode::AutoAllow => "auto_allow",
            ConfirmationMode::ConfirmOnce => "confirm_once",
            ConfirmationMode::Deny => "deny",
        }
    }

    /// 从字符串解析；无法识别的值回退到 `AutoAllow`。
    pub fn parse(s: &str) -> Self {
        match s {
            "confirm_once" => ConfirmationMode::ConfirmOnce,
            "deny" => ConfirmationMode::Deny,
            _ => ConfirmationMode::AutoAllow,
        }
    }
}

/// 单个 MCP 工具的配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolConfig {
    /// 是否启用（启用才会注册进 `ToolRegistry`）
    #[serde(default = "default_tool_enabled")]
    pub enabled: bool,
    /// 该工具的确认模式
    #[serde(default)]
    pub confirmation: ConfirmationMode,
}

fn default_tool_enabled() -> bool {
    true
}

// ── 启动策略 ──

/// MCP 服务器的启动策略。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartupPolicy {
    /// 启动优先级（越小越早）。为 `None` 表示仅手动启动。
    #[serde(default)]
    pub priority: Option<u32>,
    /// 所属优先级分组开始后再额外等待的毫秒数
    #[serde(default)]
    pub delay_ms: u64,
    /// 应用启动时自动连接
    #[serde(default = "default_launch_on_startup")]
    pub launch_on_startup: bool,
    /// 按需启动（首次工具调用时再连接）
    #[serde(default)]
    pub launch_on_demand: bool,
    /// 连接失败时的最大重试次数
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    /// 两次重试之间的等待毫秒数
    #[serde(default = "default_retry_delay_ms")]
    pub retry_delay_ms: u64,
    /// 健康检查间隔毫秒数（0 表示关闭）
    #[serde(default = "default_health_check_interval_ms")]
    pub health_check_interval_ms: u64,
}

fn default_launch_on_startup() -> bool {
    true
}
fn default_max_retries() -> u32 {
    3
}
fn default_retry_delay_ms() -> u64 {
    2000
}
fn default_health_check_interval_ms() -> u64 {
    30000
}

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
    /// 是否应在启动时自动连接：要求 `launch_on_startup` 且配置了优先级。
    pub fn should_launch_on_startup(&self) -> bool {
        self.launch_on_startup && self.priority.is_some()
    }
}

// ── 连接状态 ──

/// MCP 服务器连接的运行时状态。
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionStatus {
    /// 用户禁用了该服务器
    Disabled,
    /// 在启动队列中等待（延迟计时）
    Waiting,
    /// 正在启动子进程并完成 MCP 握手
    Starting,
    /// 已连接且工具已注册
    Ready,
    /// 连接降级（健康检查失败但仍存活）
    Degraded,
    /// 正在优雅停止
    Stopping,
    /// 已完全停止 / 断开
    Stopped,
    /// 错误状态，附带信息
    Error(String),
}

impl ConnectionStatus {
    /// 返回供前端使用的状态字符串。
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

    /// 是否处于可用状态（`Ready` 或 `Degraded`）。
    pub fn is_active(&self) -> bool {
        matches!(self, ConnectionStatus::Ready | ConnectionStatus::Degraded)
    }

    /// 错误状态时返回错误信息。
    pub fn error_message(&self) -> Option<&str> {
        match self {
            ConnectionStatus::Error(msg) => Some(msg.as_str()),
            _ => None,
        }
    }
}

// ── MCP 服务器配置（保留 auto_connect 的向后兼容） ──

/// MCP 服务器配置，作为 `AppConfig` 的一部分存入 `config.json`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// 该服务器连接的唯一标识
    pub id: String,
    /// 用户可见的名称
    pub name: String,
    /// 启动命令（如 `npx`、`python`、`node`）
    pub command: String,
    /// 传给命令的参数
    pub args: Vec<String>,
    /// 环境变量（可选）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<Vec<(String, String)>>,
    /// 启动策略（取代 auto_connect）
    #[serde(default)]
    pub startup: StartupPolicy,
    /// 旧字段：启动时自动连接（保留兼容，优先使用 `startup`）
    #[serde(
        default,
        skip_serializing_if = "is_false",
        deserialize_with = "deserialize_auto_connect"
    )]
    pub auto_connect: bool,
    /// 单工具配置（工具名 → 配置）
    #[serde(default)]
    pub tool_configs: HashMap<String, ToolConfig>,
    /// 运行时类型提示（如 `node`、`python`、`docker`、`uvx`、`go`）。
    /// 为空或 `auto` 时由命令推断。
    #[serde(default)]
    pub runtime: String,
}

fn is_false(b: &bool) -> bool {
    !b
}

/// 反序列化 `auto_connect`，使没有 `startup` 的旧配置仍可用。
/// 存在 `startup` 时忽略该字段。
fn deserialize_auto_connect<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let val: serde_json::Value = serde_json::Value::deserialize(deserializer)?;
    Ok(val.as_bool().unwrap_or(false))
}

impl McpServerConfig {
    /// 取得该服务器实际生效的运行时类型。
    ///
    /// 显式配置时用配置值，否则从命令推断。
    pub fn effective_runtime(&self) -> Option<RuntimeType> {
        if self.runtime.is_empty() || self.runtime == "auto" {
            RuntimeType::infer_from_command(&self.command)
        } else {
            self.runtime.parse().ok()
        }
    }

    /// 是否应自动连接（新启动策略优先，回退到旧 `auto_connect`）。
    pub fn should_auto_connect(&self) -> bool {
        if self.startup.launch_on_startup || self.startup.priority.is_some() {
            return true;
        }
        self.auto_connect
    }
}
