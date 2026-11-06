//! MCP 服务器连接管理器。
//!
//! 每个服务器是一个子进程：管理器负责启动、MCP 握手、工具注册、健康检查
//! 与统计。工具以 [`McpToolWrapper`] 的形式注册进 `ToolRegistry`，实际调用
//! 通过克隆的 `Peer<RoleClient>` 发回子进程。

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Instant;

use rmcp::model::Tool as McpTool;
use rmcp::service::{RoleClient, RunningService, ServiceExt};
use rmcp::transport::TokioChildProcess;
use rig::message::ToolName;
use rig::tool::DynamicTool;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{ChildStderr, Command};
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

use crate::agent::tools::convert::ToolEntry;

use crate::core::error::{AppError, Result};
use crate::features::mcp::config::{ConfirmationMode, ConnectionStatus, McpServerConfig};
use crate::runtime::toolchain::RuntimeManager;
use crate::agent::tools::registry::ToolRegistry;

/// stderr 环形缓冲区的最大行数。
const STDERR_BUFFER_LINES: usize = 1000;
/// 健康检查的最小间隔，避免配置过小导致频繁加锁。
const MIN_HEALTH_INTERVAL_MS: u64 = 5_000;
/// 优先级分组之间的等待时间。
const GROUP_DELAY_MS: u64 = 500;

// ── 对外类型 ──

/// MCP 服务器暴露的某个工具的运行时信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInfo {
    /// 工具名
    pub name: String,
    /// 来自 MCP 服务器的工具描述
    pub description: String,
    /// 当前是否启用
    pub enabled: bool,
    /// 确认模式字符串：`auto_allow` | `confirm_once` | `deny`
    pub confirmation: String,
}

/// 一条连接的健康与使用统计。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionStats {
    /// 连接建立至今的秒数
    pub uptime_seconds: u64,
    /// 通过该连接执行的工具调用总数
    pub total_calls: u64,
    /// 失败的工具调用数
    pub error_count: u64,
    /// 工具调用的平均延迟（毫秒）
    pub avg_latency_ms: f64,
    /// 最近一次错误信息
    pub last_error: Option<String>,
}

/// MCP 服务器 stderr 的一行日志。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpLogEntry {
    pub timestamp: String,
    pub level: String,
    pub message: String,
}

/// 通过 IPC 返回给前端的连接运行时信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub id: String,
    pub name: String,
    /// 状态字符串（`ready`、`starting`、`error`、`stopped` 等）
    pub status: String,
    /// 人类可读的状态细节（例如错误信息）
    pub status_detail: Option<String>,
    pub tool_count: usize,
    /// 带配置状态的详细工具列表
    pub tools: Vec<McpToolInfo>,
    /// 健康与使用统计
    pub stats: ConnectionStats,
    pub error: Option<String>,
}

/// 一条活跃 MCP 连接的内部句柄。
struct ActiveConnection {
    name: String,
    running: RunningService<RoleClient, ()>,
    tool_names: Vec<String>,
    tools: Vec<McpToolInfo>,
    started_at: Instant,
    stats: CallStats,
    status: Arc<StdMutex<ConnectionStatus>>,
    stderr_buffer: Arc<StdMutex<VecDeque<String>>>,
}

/// MCP 工具调用的共享统计句柄。
///
/// 这四项总是成组创建、成组传递，因此打包成一个结构体，
/// 避免在函数签名里散落四个参数。
#[derive(Clone)]
struct CallStats {
    /// 累计调用次数。
    total_calls: Arc<AtomicU64>,
    /// 累计失败次数。
    error_count: Arc<AtomicU64>,
    /// 每次调用的耗时样本（毫秒）。
    latency_samples: Arc<StdMutex<Vec<u64>>>,
    /// 最近一次失败的原因。
    last_error: Arc<StdMutex<Option<String>>>,
}

impl CallStats {
    /// 创建一组全新的统计句柄。
    fn new() -> Self {
        Self {
            total_calls: Arc::new(AtomicU64::new(0)),
            error_count: Arc::new(AtomicU64::new(0)),
            latency_samples: Arc::new(StdMutex::new(Vec::new())),
            last_error: Arc::new(StdMutex::new(None)),
        }
    }
}

// ── 管理器 ──

/// 管理 MCP 服务器连接的生命周期。
///
/// 每个服务器以子进程运行，其工具注册到 `ToolRegistry`；断开连接时会把
/// 这些工具一并注销。
pub struct McpServerManager {
    connections: Arc<Mutex<HashMap<String, ActiveConnection>>>,
    tools: Arc<Mutex<ToolRegistry>>,
    runtime_manager: Option<Arc<RuntimeManager>>,
}

impl McpServerManager {
    /// 用共享的工具注册表构造管理器。
    pub fn new(tools: Arc<Mutex<ToolRegistry>>) -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
            tools,
            runtime_manager: None,
        }
    }

    /// 注入运行时管理器，用于连接前的依赖校验。
    pub fn with_runtime_manager(mut self, rm: Arc<RuntimeManager>) -> Self {
        self.runtime_manager = Some(rm);
        self
    }

    // ── 对外 API ──

    /// 以子进程方式连接一个 MCP 服务器。
    pub async fn connect(&self, config: &McpServerConfig) -> Result<()> {
        let mut guard = self.connections.lock().await;
        self.connect_with_guard(config, &mut guard).await
    }

    /// 内部连接流程，调用方已持有 `connections` 锁。
    ///
    /// 阶段依次为：清理同 id 旧连接 → 校验运行时 → 构造命令 → 启动子进程
    /// 并接管 stderr → MCP 握手 → 列出工具 → 注册工具包装器 → 置为
    /// `Ready` → 启动健康检查。任一步失败都会把状态置为 `Error` 并返回。
    async fn connect_with_guard(
        &self,
        config: &McpServerConfig,
        guard: &mut HashMap<String, ActiveConnection>,
    ) -> Result<()> {
        if guard.contains_key(&config.id) {
            self.disconnect_inner(&config.id, guard).await;
        }

        let status = Arc::new(StdMutex::new(ConnectionStatus::Starting));
        let stderr_buffer = Arc::new(StdMutex::new(VecDeque::with_capacity(
            STDERR_BUFFER_LINES,
        )));

        self.validate_runtime(config, &status).await?;

        let cmd = Self::build_command(config);

        log::info!(
            "Spawning MCP server '{}': {} {:?}",
            config.name,
            config.command,
            config.args
        );

        let (transport, stderr_opt) = TokioChildProcess::builder(cmd)
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| {
                AppError::Mcp(format!(
                    "Failed to spawn MCP server '{}' (command: `{} {:?}`): {}",
                    config.name, config.command, config.args, e
                ))
            })?;

        Self::capture_stderr(stderr_opt, stderr_buffer.clone(), config.name.clone());

        // MCP 握手（initialize + initialized）。
        let cmd_display = format!("{} {:?}", config.command, config.args);
        let running = ().serve(transport).await.map_err(|e| {
            Self::set_status(
                &status,
                ConnectionStatus::Error(format!("MCP handshake failed: {}", e)),
            );
            AppError::Mcp(format!(
                "MCP handshake failed for '{}' (command: `{}`): {}",
                config.name, cmd_display, e
            ))
        })?;

        // 列出服务器暴露的工具。
        let mcp_tools: Vec<McpTool> = running.list_all_tools().await.map_err(|e| {
            Self::set_status(
                &status,
                ConnectionStatus::Error(format!("Failed to list tools: {}", e)),
            );
            AppError::Mcp(format!(
                "Failed to list MCP tools for '{}' (command: `{}`): {}",
                config.name, cmd_display, e
            ))
        })?;

        // 共享统计句柄。
        let peer = running.peer().clone();
        let started_at = Instant::now();
        let stats = CallStats::new();

        let (tool_names, tool_details) = self
            .register_tools(config, &mcp_tools, peer.clone(), &stats)
            .await;

        Self::set_status(&status, ConnectionStatus::Ready);

        let connection = ActiveConnection {
            name: config.name.clone(),
            running,
            tool_names,
            tools: tool_details,
            started_at,
            stats,
            status: status.clone(),
            stderr_buffer: stderr_buffer.clone(),
        };

        let conn_id = config.id.clone();
        guard.insert(conn_id.clone(), connection);

        Self::spawn_health_worker(
            self.connections.clone(),
            conn_id,
            config.name.clone(),
            config.startup.health_check_interval_ms.max(MIN_HEALTH_INTERVAL_MS),
        );

        log::info!(
            "MCP server '{}' connected with {} tools",
            config.name,
            mcp_tools.len()
        );
        Ok(())
    }

    /// 启动前校验命令所需的运行时（如 node、python）。
    ///
    /// 推断不出运行时则跳过校验；校验失败会把状态置为 `Error`。
    async fn validate_runtime(
        &self,
        config: &McpServerConfig,
        status: &Arc<StdMutex<ConnectionStatus>>,
    ) -> Result<()> {
        let Some(rm) = &self.runtime_manager else {
            return Ok(());
        };
        let Some(rt) = crate::runtime::toolchain::RuntimeType::infer_from_command(&config.command)
        else {
            return Ok(());
        };

        if let Err(msg) = rm.validate_runtime(&rt).await {
            let err_msg = format!(
                "MCP server '{}' runtime validation failed: {}",
                config.name, msg
            );
            log::error!("{}", err_msg);
            Self::set_status(status, ConnectionStatus::Error(err_msg.clone()));
            return Err(AppError::Mcp(err_msg));
        }
        Ok(())
    }

    /// 构造子进程命令。
    ///
    /// Windows 下 `.cmd` / `.bat` 不能直接 spawn，需要 `cmd /c` 包装。
    fn build_command(config: &McpServerConfig) -> Command {
        #[allow(unused_mut)]
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args);

        #[cfg(target_os = "windows")]
        {
            let lower = config.command.to_lowercase();
            if lower.ends_with(".cmd") || lower.ends_with(".bat") {
                let display = format!("{} {}", config.command, config.args.join(" "));
                drop(cmd);
                let mut wrapped = Command::new("cmd");
                wrapped.arg("/c");
                wrapped.arg(&config.command);
                wrapped.args(&config.args);
                log::info!(
                    "MCP server '{}': wrapped batch command with `cmd /c {}`",
                    config.name,
                    display
                );
                cmd = wrapped;
            }
        }

        cmd.kill_on_drop(true);
        if let Some(env_vars) = &config.env {
            for (k, v) in env_vars {
                cmd.env(k, v);
            }
        }
        cmd
    }

    /// 把子进程 stderr 收进环形缓冲区并转发到日志。
    ///
    /// 后台任务随 stderr 关闭而结束；缓冲区只保留最近
    /// [`STDERR_BUFFER_LINES`] 行，供前端诊断使用。
    fn capture_stderr(
        stderr: Option<ChildStderr>,
        buffer: Arc<StdMutex<VecDeque<String>>>,
        name: String,
    ) {
        let Some(stderr) = stderr else {
            return;
        };

        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let trimmed = line.trim_end().to_string();
                        if !trimmed.is_empty() {
                            log::warn!("[MCP stderr: {}] {}", name, trimmed);
                            let mut buffer = buffer.lock().unwrap_or_else(|e| e.into_inner());
                            if buffer.len() >= STDERR_BUFFER_LINES {
                                buffer.pop_front();
                            }
                            buffer.push_back(trimmed);
                        }
                    }
                }
            }
        });
    }

    /// 为服务器列出的每个工具创建包装器并注册到 `ToolRegistry`。
    ///
    /// 被禁用或被配置为 `deny` 的工具仍会出现在返回的详情里供前端展示，
    /// 但不会注册成可调用工具。同名旧工具会先注销再注册。
    async fn register_tools(
        &self,
        config: &McpServerConfig,
        mcp_tools: &[McpTool],
        peer: rmcp::service::Peer<rmcp::service::RoleClient>,
        stats: &CallStats,
    ) -> (Vec<String>, Vec<McpToolInfo>) {
        let mut tool_names: Vec<String> = Vec::new();
        let mut tool_details: Vec<McpToolInfo> = Vec::new();
        let mut tool_registry = self.tools.lock().await;

        for mcp_tool in mcp_tools {
            let tool_name = mcp_tool.name.to_string();
            let input_schema: Value = serde_json::to_value(&*mcp_tool.input_schema)
                .unwrap_or_else(|_| Value::Object(Map::new()));
            let description = mcp_tool.description.as_deref().unwrap_or("").to_string();

            let tool_config = config.tool_configs.get(&tool_name);
            let enabled = tool_config.map(|c| c.enabled).unwrap_or(true);
            let confirmation = tool_config
                .map(|c| c.confirmation.clone())
                .unwrap_or(ConfirmationMode::AutoAllow);

            tool_details.push(McpToolInfo {
                name: tool_name.clone(),
                description: description.clone(),
                enabled,
                confirmation: confirmation.as_str().to_string(),
            });

            if !enabled {
                log::info!(
                    "MCP tool '{}' on '{}' is disabled by config, skipping registration",
                    tool_name,
                    config.name
                );
                continue;
            }
            if matches!(confirmation, ConfirmationMode::Deny) {
                log::info!(
                    "MCP tool '{}' on '{}' is denied by config, skipping registration",
                    tool_name,
                    config.name
                );
                continue;
            }

            let wrapper = Arc::new(McpToolWrapper {
                registered_name: format!("{}__{}", config.name, tool_name),
                inner_name: tool_name.clone(),
                inner_description: description,
                inner_parameters: input_schema,
                peer: peer.clone(),
                confirmation,
                total_calls: stats.total_calls.clone(),
                error_count: stats.error_count.clone(),
                latency_samples: stats.latency_samples.clone(),
                last_error: stats.last_error.clone(),
            });

            // MCP 工具名带服务器前缀，避免不同服务器的同名工具互相覆盖。
            let registered_name = wrapper.registered_name.clone();
            let entry = match wrapper.into_entry() {
                Ok(entry) => entry,
                Err(e) => {
                    log::warn!(
                        "Skipping MCP tool '{}' on '{}': invalid registered name ({})",
                        tool_name,
                        config.name,
                        e
                    );
                    continue;
                }
            };

            if tool_registry.is_registered(&registered_name) {
                tool_registry.unregister(&registered_name);
            }
            tool_registry.register_mcp(&config.name, &registered_name, entry, true);
            tool_names.push(registered_name);
        }

        (tool_names, tool_details)
    }

    /// 启动健康检查协程：连接意外关闭时标记为 `Error`，从 `Degraded`
    /// 恢复时标记回 `Ready`；连接被移除后协程自行退出。
    fn spawn_health_worker(
        connections: Arc<Mutex<HashMap<String, ActiveConnection>>>,
        conn_id: String,
        name: String,
        interval_ms: u64,
    ) {
        tokio::spawn(async move {
            loop {
                sleep(Duration::from_millis(interval_ms)).await;
                let mut guard = connections.lock().await;
                let Some(conn) = guard.get_mut(&conn_id) else {
                    // 连接已被移除（主动断开）。
                    break;
                };

                if conn.running.is_closed() {
                    if Self::status_snapshot(&conn.status).is_active() {
                        let error_msg =
                            "Connection closed unexpectedly (process may have crashed)"
                                .to_string();
                        if let Ok(mut last) = conn.stats.last_error.lock() {
                            *last = Some(error_msg.clone());
                        }
                        Self::set_status(&conn.status, ConnectionStatus::Error(error_msg));
                        log::warn!("MCP health: '{}' connection lost, marked as Error", name);
                    }
                    // 已经是 error/stopped 则无需重复更新。
                    break;
                }

                if matches!(
                    Self::status_snapshot(&conn.status),
                    ConnectionStatus::Degraded
                ) {
                    Self::set_status(&conn.status, ConnectionStatus::Ready);
                    log::info!("MCP health: '{}' restored to Ready", name);
                }
            }
        });
    }

    /// 覆盖连接状态；互斥锁中毒时取回内部值继续使用。
    fn set_status(status: &Arc<StdMutex<ConnectionStatus>>, next: ConnectionStatus) {
        let mut guard = status.lock().unwrap_or_else(|e| e.into_inner());
        *guard = next;
    }

    /// 读取当前连接状态的快照。
    fn status_snapshot(status: &Arc<StdMutex<ConnectionStatus>>) -> ConnectionStatus {
        let guard = status.lock().unwrap_or_else(|e| e.into_inner());
        guard.clone()
    }

    /// 断开一个 MCP 服务器连接。
    pub async fn disconnect(&self, id: &str) -> Result<()> {
        let mut guard = self.connections.lock().await;
        self.disconnect_inner(id, &mut guard).await;
        Ok(())
    }

    /// 内部断开流程：置为 `Stopping`、注销工具、关闭 MCP 连接。
    async fn disconnect_inner(&self, id: &str, guard: &mut HashMap<String, ActiveConnection>) {
        if let Some(conn) = guard.remove(id) {
            Self::set_status(&conn.status, ConnectionStatus::Stopping);

            {
                let mut tool_registry = self.tools.lock().await;
                for tool_name in &conn.tool_names {
                    if tool_registry.is_registered(tool_name) {
                        tool_registry.unregister(tool_name);
                    }
                }
            }

            let mut running = conn.running;
            if let Err(e) = running.close().await {
                log::warn!("Error closing MCP connection '{}': {}", id, e);
            }
            log::info!("MCP server '{}' disconnected", conn.name);
        }
    }

    /// 重启一个 MCP 服务器（仅执行断开）。
    ///
    /// 完整重启由调用方按「断开 + 连接」两步完成。
    pub async fn restart(&self, id: &str) -> Result<()> {
        self.disconnect(id).await
    }

    /// 列出所有连接及其状态。
    pub async fn list_connections(&self) -> Vec<ConnectionInfo> {
        let guard = self.connections.lock().await;
        guard
            .iter()
            .map(|(id, conn)| {
                let stats = Self::compute_stats(conn);
                let status = Self::status_snapshot(&conn.status);
                let detail = status.error_message().map(|e| e.to_string());
                ConnectionInfo {
                    id: id.clone(),
                    name: conn.name.clone(),
                    status: status.as_str().to_string(),
                    status_detail: detail.clone(),
                    tool_count: conn.tool_names.len(),
                    tools: conn.tools.clone(),
                    stats,
                    error: detail,
                }
            })
            .collect()
    }

    /// 获取指定连接的详细工具信息。
    pub async fn get_connection_tools(&self, id: &str) -> Vec<McpToolInfo> {
        let guard = self.connections.lock().await;
        guard
            .get(id)
            .map(|conn| conn.tools.clone())
            .unwrap_or_default()
    }

    /// 获取指定连接的健康与使用统计。
    pub async fn get_connection_stats(&self, id: &str) -> Option<ConnectionStats> {
        let guard = self.connections.lock().await;
        guard.get(id).map(Self::compute_stats)
    }

    /// 获取指定连接的 stderr 日志。
    pub async fn get_connection_logs(&self, id: &str) -> Vec<String> {
        let guard = self.connections.lock().await;
        guard
            .get(id)
            .map(|conn| {
                let buffer = conn.stderr_buffer.lock().unwrap_or_else(|e| e.into_inner());
                buffer.iter().cloned().collect()
            })
            .unwrap_or_default()
    }

    /// 获取指定连接的当前状态字符串与错误细节。
    pub async fn get_connection_status(&self, id: &str) -> Option<(String, Option<String>)> {
        let guard = self.connections.lock().await;
        guard.get(id).map(|conn| {
            let status = Self::status_snapshot(&conn.status);
            (
                status.as_str().to_string(),
                status.error_message().map(|e| e.to_string()),
            )
        })
    }

    /// 根据共享计数器计算统计信息。
    fn compute_stats(conn: &ActiveConnection) -> ConnectionStats {
        let uptime = conn.started_at.elapsed().as_secs();
        let total_calls = conn.stats.total_calls.load(Ordering::Relaxed);
        let error_count = conn.stats.error_count.load(Ordering::Relaxed);
        let avg_latency_ms = {
            let samples = conn
                .stats
                .latency_samples
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if samples.is_empty() {
                0.0
            } else {
                let sum: u64 = samples.iter().sum();
                (sum as f64 / samples.len() as f64) / 1_000_000.0
            }
        };
        let last_error = conn
            .stats
            .last_error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();

        ConnectionStats {
            uptime_seconds: uptime,
            total_calls,
            error_count,
            avg_latency_ms,
            last_error,
        }
    }

    /// 关闭所有连接。
    pub async fn shutdown_all(&self) {
        let ids: Vec<String> = {
            let guard = self.connections.lock().await;
            guard.keys().cloned().collect()
        };
        for id in &ids {
            self.disconnect(id).await.unwrap_or_else(|e| {
                log::error!("Error disconnecting MCP server '{}': {}", id, e);
            });
        }
    }

    /// 按启动策略连接所有配置的服务器。
    ///
    /// 按 `startup.priority` 升序分组（缺省 99），组内按配置的
    /// `delay_ms` 依次等待后连接，组间等待 [`GROUP_DELAY_MS`]。
    pub async fn connect_all(&self, configs: &[McpServerConfig]) {
        let groups = Self::priority_groups(configs);

        for (i, (priority, cfgs)) in groups.iter().enumerate() {
            log::info!(
                "MCP auto-connect: starting priority group {} ({} server(s))",
                priority,
                cfgs.len()
            );

            // 先等完组内所有延迟，再依次连接。
            for cfg in cfgs {
                let delay = cfg.startup.delay_ms;
                if delay > 0 {
                    sleep(Duration::from_millis(delay)).await;
                }
            }

            for cfg in cfgs {
                self.connect_with_retry(cfg).await;
            }

            if i < groups.len() - 1 {
                sleep(Duration::from_millis(GROUP_DELAY_MS)).await;
            }
        }
    }

    /// 把需要自动连接的配置按优先级分组，返回按优先级升序的
    /// `(priority, configs)` 列表。
    fn priority_groups(configs: &[McpServerConfig]) -> Vec<(u32, Vec<McpServerConfig>)> {
        let mut groups: HashMap<u32, Vec<McpServerConfig>> = HashMap::new();
        for cfg in configs {
            if !cfg.should_auto_connect() {
                log::info!(
                    "MCP server '{}': skipping auto-connect (startup policy disabled)",
                    cfg.name
                );
                continue;
            }
            let priority = cfg.startup.priority.unwrap_or(99);
            groups.entry(priority).or_default().push(cfg.clone());
        }

        let mut priorities: Vec<u32> = groups.keys().copied().collect();
        priorities.sort_unstable();
        priorities
            .into_iter()
            .filter_map(|priority| {
                groups
                    .remove(&priority)
                    .map(|cfgs| (priority, cfgs))
            })
            .collect()
    }

    /// 连接单个服务器，按启动策略重试；最终失败只记录日志。
    async fn connect_with_retry(&self, cfg: &McpServerConfig) {
        let max_retries = cfg.startup.max_retries;
        let retry_delay = cfg.startup.retry_delay_ms;
        let mut attempt = 0;

        loop {
            attempt += 1;
            match self.connect(cfg).await {
                Ok(()) => break,
                Err(e) => {
                    if attempt <= max_retries {
                        log::warn!(
                            "MCP connect '{}' attempt {}/{} failed: {}. Retrying in {}ms...",
                            cfg.name,
                            attempt,
                            max_retries + 1,
                            e,
                            retry_delay
                        );
                        sleep(Duration::from_millis(retry_delay)).await;
                    } else {
                        log::error!(
                            "MCP connect '{}' failed after {} attempts: {}",
                            cfg.name,
                            max_retries + 1,
                            e
                        );
                        break;
                    }
                }
            }
        }
    }
}

/// 以 MCP peer 为后端的工具包装器。
///
/// 调用通过克隆的 `Peer<RoleClient>` 发回子进程，并顺带累计调用次数、
/// 失败次数与延迟样本。
pub struct McpToolWrapper {
    /// 注册进 `ToolRegistry` 的名字（带服务器前缀，避免覆盖内置工具）。
    registered_name: String,
    /// MCP 服务器上的原始工具名（调用时用它）。
    inner_name: String,
    inner_description: String,
    inner_parameters: Value,
    peer: rmcp::service::Peer<rmcp::service::RoleClient>,
    confirmation: ConfirmationMode,
    total_calls: Arc<AtomicU64>,
    error_count: Arc<AtomicU64>,
    latency_samples: Arc<StdMutex<Vec<u64>>>,
    last_error: Arc<StdMutex<Option<String>>>,
}

impl McpToolWrapper {
    /// 转成注册表条目：运行时形态是 `DynamicTool`，没有可推导的 footprint。
    fn into_entry(self: &Arc<Self>) -> std::result::Result<ToolEntry, String> {
        let name = ToolName::new(self.registered_name.clone()).map_err(|e| e.to_string())?;

        let wrapper = self.clone();
        let dynamic = DynamicTool::new(
            name,
            self.inner_description.clone(),
            self.inner_parameters.clone(),
            move |args: Value| {
                let wrapper = wrapper.clone();
                Box::pin(async move { wrapper.call_json(args).await })
            },
        );

        // MCP 服务器不提供本项目的风险注解，只能按用户配置映射：
        // AutoAllow 视为只读放行；ConfirmOnce 视为可写、走统一审批闸门。
        let annotations = match self.confirmation {
            ConfirmationMode::AutoAllow => rig::completion::ToolAnnotations::default()
                .with_read_only(true)
                .with_destructive(false)
                .with_open_world(true),
            _ => rig::completion::ToolAnnotations::default()
                .with_read_only(false)
                .with_destructive(true)
                .with_open_world(true),
        };

        Ok(ToolEntry::from_dynamic(dynamic.with_annotations(annotations)))
    }

    /// 真正发起 MCP 调用，并保留 `is_error` 语义。
    async fn call_json(
        &self,
        input: Value,
    ) -> std::result::Result<rig::tool::ToolOutput, rig::tool::ToolExecutionError> {
        use rig::tool::ToolExecutionError;

        // 配置为 deny 的工具直接拒绝执行。
        if matches!(self.confirmation, ConfirmationMode::Deny) {
            return Err(ToolExecutionError::permission_denied(format!(
                "MCP tool '{}' execution denied by configuration",
                self.inner_name
            )));
        }

        let start = Instant::now();

        // CallToolRequestParams 要求参数是 JSON 对象。
        let arguments = match input {
            Value::Object(map) => map,
            other => {
                let mut map = Map::new();
                map.insert("value".to_string(), other);
                map
            }
        };

        let params = rmcp::model::CallToolRequestParams::new(self.inner_name.clone())
            .with_arguments(arguments);

        let result = self.peer.call_tool(params).await.map_err(|e| {
            self.error_count.fetch_add(1, Ordering::Relaxed);
            if let Ok(mut last) = self.last_error.lock() {
                *last = Some(e.to_string());
            }
            ToolExecutionError::other(format!("MCP tool '{}' error: {}", self.inner_name, e))
        })?;

        // 成功后才计入调用次数与延迟样本。
        self.total_calls.fetch_add(1, Ordering::Relaxed);
        let elapsed_ns = start.elapsed().as_nanos() as u64;
        if let Ok(mut samples) = self.latency_samples.lock() {
            samples.push(elapsed_ns);
            if samples.len() > 100 {
                samples.remove(0);
            }
        }

        // MCP content 数组转成 JSON 值，再序列化成文本；`is_error` 落成工具错误。
        let outcome = Self::render_result(&result);
        if outcome.is_err() {
            self.error_count.fetch_add(1, Ordering::Relaxed);
            if let Ok(mut last) = self.last_error.lock() {
                *last = outcome.as_ref().err().map(|e| e.to_string());
            }
        }
        outcome
    }

    /// 把 MCP 的调用结果转成工具输出。
    ///
    /// 纯函数，便于在没有真实 MCP 连接的情况下验证 `structured_content` 与
    /// `is_error` 的语义。
    fn render_result(
        result: &rmcp::model::CallToolResult,
    ) -> std::result::Result<rig::tool::ToolOutput, rig::tool::ToolExecutionError> {
        use rig::tool::{ToolExecutionError, ToolOutput};

        let items: Vec<Value> = result
            .content
            .iter()
            .map(|c| match c.as_text() {
                Some(text) => Value::String(text.text.clone()),
                None => Value::String(format!("{:?}", c)),
            })
            .collect();

        let result_value = if let Some(structured) = result.structured_content.clone() {
            structured
        } else if result.content.is_empty() {
            Value::Null
        } else if result.content.len() == 1 {
            match result.content[0].as_text() {
                Some(text) => Value::String(text.text.clone()),
                None => Value::Array(items),
            }
        } else {
            Value::Array(items)
        };

        let text = serde_json::to_string(&result_value)
            .map_err(|e| ToolExecutionError::other(format!("failed to serialize MCP result: {}", e)))?;

        // 服务器声明失败时按工具错误返回，不能把错误文本当成功结果。
        if result.is_error.unwrap_or(false) {
            return Err(ToolExecutionError::other(text));
        }

        Ok(ToolOutput::text(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::{CallToolResult, Content};

    fn result(content: Vec<Content>, structured: Option<Value>, is_error: bool) -> CallToolResult {
        let mut value = serde_json::json!({
            "content": serde_json::to_value(&content).expect("content 应可序列化"),
            "isError": is_error,
        });
        if let Some(structured) = structured {
            value["structuredContent"] = structured;
        }
        serde_json::from_value(value).expect("CallToolResult 应能反序列化")
    }

    #[test]
    fn render_result_prefers_structured_content() {
        let value = McpToolWrapper::render_result(&result(
            vec![Content::text("ignored")],
            Some(serde_json::json!({ "rows": 3 })),
            false,
        ))
        .expect("成功结果应可渲染");

        let parsed: Value = serde_json::from_str(&value.render()).expect("渲染结果应为 JSON");
        assert_eq!(parsed, serde_json::json!({ "rows": 3 }));
    }

    #[test]
    fn render_result_keeps_single_text() {
        let value = McpToolWrapper::render_result(&result(
            vec![Content::text("hello")],
            None,
            false,
        ))
        .expect("成功结果应可渲染");

        // 单个文本结果会先序列化成 JSON 字符串，因此渲染带引号。
        assert_eq!(value.render(), "\"hello\"");
    }

    #[test]
    fn is_error_becomes_tool_error() {
        let error = McpToolWrapper::render_result(&result(
            vec![Content::text("server exploded")],
            None,
            true,
        ))
        .expect_err("is_error=true 必须落成工具错误");

        assert!(
            error.model_output().as_text().is_some(),
            "错误文本应回灌给模型: {:?}",
            error
        );
    }
}
