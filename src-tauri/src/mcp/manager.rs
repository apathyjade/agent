use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Instant;

use std::future::Future;
use std::pin::Pin;

use rmcp::model::Tool as McpTool;
use rmcp::service::{RoleClient, RunningService, ServiceExt};
use rmcp::transport::TokioChildProcess;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

use rig::completion::ToolDefinition;
use rig::tool::{ToolDyn, ToolError};

use crate::environment::RuntimeManager;
use crate::error::{AppError, Result};
use crate::mcp::config::{ConfirmationMode, ConnectionStatus, McpServerConfig};
use crate::tools::registry::ToolRegistry;

// ── Public types ──

/// Runtime info about a tool exposed by an MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolInfo {
    /// Tool name
    pub name: String,
    /// Tool description from the MCP server
    pub description: String,
    /// Whether the tool is currently enabled
    pub enabled: bool,
    /// Confirmation mode as string: "auto_allow" | "confirm_once" | "deny"
    pub confirmation: String,
}

/// Health and usage statistics for a connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionStats {
    /// Seconds since the connection was established
    pub uptime_seconds: u64,
    /// Total tool calls executed through this connection
    pub total_calls: u64,
    /// Number of failed tool calls
    pub error_count: u64,
    /// Average latency of tool calls in milliseconds
    pub avg_latency_ms: f64,
    /// Last error message, if any
    pub last_error: Option<String>,
}

/// A single log entry from an MCP server's stderr.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpLogEntry {
    pub timestamp: String,
    pub level: String,
    pub message: String,
}

/// Runtime info about a connection, returned to frontend via IPC
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub id: String,
    pub name: String,
    /// Connection status as string ("ready", "starting", "error", "stopped", etc.)
    pub status: String,
    /// Human-readable status detail (e.g. error message)
    pub status_detail: Option<String>,
    pub tool_count: usize,
    /// Detailed tool list with configuration status
    pub tools: Vec<McpToolInfo>,
    /// Health and usage stats
    pub stats: ConnectionStats,
    pub error: Option<String>,
}

/// Internal handle for an active MCP server connection.
struct ActiveConnection {
    name: String,
    running: RunningService<RoleClient, ()>,
    tool_names: Vec<String>,
    tools: Vec<McpToolInfo>,
    started_at: Instant,
    total_calls: Arc<AtomicU64>,
    error_count: Arc<AtomicU64>,
    latency_samples: Arc<StdMutex<Vec<u64>>>,
    last_error: Arc<StdMutex<Option<String>>>,
    status: Arc<StdMutex<ConnectionStatus>>,
    stderr_buffer: Arc<StdMutex<VecDeque<String>>>,
}

// ── Manager ──

/// Manages lifecycle of MCP server connections.
/// Each server runs as a child process; its tools are registered in ToolRegistry.
pub struct McpServerManager {
    connections: Arc<Mutex<HashMap<String, ActiveConnection>>>,
    tools: Arc<Mutex<ToolRegistry>>,
    runtime_manager: Option<Arc<RuntimeManager>>,
}

impl McpServerManager {
    pub fn new(tools: Arc<Mutex<ToolRegistry>>) -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
            tools,
            runtime_manager: None,
        }
    }

    /// Set the runtime manager for pre-connect validation.
    pub fn with_runtime_manager(mut self, rm: Arc<RuntimeManager>) -> Self {
        self.runtime_manager = Some(rm);
        self
    }

    // ── Public API ──

    /// Connect to an MCP server by spawning it as a child process.
    pub async fn connect(&self, config: &McpServerConfig) -> Result<()> {
        let mut guard = self.connections.lock().await;
        self.connect_with_guard(config, &mut guard).await
    }

    /// Internal connect that takes a pre-locked guard.
    async fn connect_with_guard(
        &self,
        config: &McpServerConfig,
        guard: &mut HashMap<String, ActiveConnection>,
    ) -> Result<()> {
        // Disconnect existing connection with same id
        if guard.contains_key(&config.id) {
            self.disconnect_inner(&config.id, guard).await;
        }

        // Set status to Starting
        let status = Arc::new(StdMutex::new(ConnectionStatus::Starting));
        let stderr_buffer = Arc::new(StdMutex::new(VecDeque::with_capacity(1000)));

        // 1b. Validate runtime before spawning
        if let Some(rm) = &self.runtime_manager {
            let inferred = crate::environment::RuntimeType::infer_from_command(&config.command);
            if let Some(rt) = inferred {
                if let Err(msg) = rm.validate_runtime(&rt).await {
                    let err_msg = format!(
                        "MCP server '{}' runtime validation failed: {}",
                        config.name, msg
                    );
                    log::error!("{}", err_msg);
                    {
                        let mut s = status.lock().unwrap_or_else(|e| e.into_inner());
                        *s = ConnectionStatus::Error(err_msg.clone());
                    }
                    return Err(AppError::Skill(err_msg));
                }
            }
        }

        #[allow(unused_mut)]
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args);

        // Windows: batch files (.cmd/.bat) need cmd /c wrapper
        #[cfg(target_os = "windows")]
        {
            let lower = config.command.to_lowercase();
            if lower.ends_with(".cmd") || lower.ends_with(".bat") {
                let display = format!("{} {}", &config.command, &config.args.join(" "));
                drop(cmd);
                let mut wrapped = Command::new("cmd");
                wrapped.arg("/c");
                wrapped.arg(&config.command);
                wrapped.args(&config.args);
                log::info!(
                    "MCP server '{}': wrapped batch command with `cmd /c {}`",
                    config.name, display
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

        log::info!(
            "Spawning MCP server '{}': {} {:?}",
            config.name, config.command, config.args
        );

        // 2. Spawn with stderr piped for diagnostics
        let (transport, stderr_opt) = TokioChildProcess::builder(cmd)
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| {
                AppError::Skill(format!(
                    "Failed to spawn MCP server '{}' (command: `{} {:?}`): {}",
                    config.name, config.command, config.args, e
                ))
            })?;

        // 3. Capture stderr into buffer + log
        let buf = stderr_buffer.clone();
        let name_for_stderr = config.name.clone();
        if let Some(stderr) = stderr_opt {
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
                                log::warn!("[MCP stderr: {}] {}", name_for_stderr, trimmed);
                                let mut buffer = buf.lock().unwrap_or_else(|e| e.into_inner());
                                if buffer.len() >= 1000 {
                                    buffer.pop_front();
                                }
                                buffer.push_back(trimmed);
                            }
                        }
                    }
                }
            });
        }

        // 4. Perform MCP handshake (initialize + initialized)
        let cmd_display = format!("{} {:?}", config.command, config.args);
        let running = ()
            .serve(transport)
            .await
            .map_err(|e| {
                {
                    let mut s = status.lock().unwrap_or_else(|e| e.into_inner());
                    *s = ConnectionStatus::Error(format!("MCP handshake failed: {}", e));
                }
                AppError::Skill(format!(
                    "MCP handshake failed for '{}' (command: `{}`): {}",
                    config.name, cmd_display, e
                ))
            })?;

        // 5. List tools from the server
        let mcp_tools: Vec<McpTool> = running
            .list_all_tools()
            .await
            .map_err(|e| {
                {
                    let mut s = status.lock().unwrap_or_else(|e| e.into_inner());
                    *s = ConnectionStatus::Error(format!("Failed to list tools: {}", e));
                }
                AppError::Skill(format!(
                    "Failed to list MCP tools for '{}' (command: `{}`): {}",
                    config.name, cmd_display, e
                ))
            })?;

        // 6. Setup shared stats tracking
        let peer = running.peer().clone();
        let started_at = Instant::now();
        let total_calls = Arc::new(AtomicU64::new(0));
        let error_count = Arc::new(AtomicU64::new(0));
        let latency_samples = Arc::new(std::sync::Mutex::new(Vec::new()));
        let last_error = Arc::new(std::sync::Mutex::new(None));

        // 7. Create tool wrappers and register in ToolRegistry
        let mut tool_names: Vec<String> = Vec::new();
        let mut tool_details: Vec<McpToolInfo> = Vec::new();

        {
            let mut tool_registry = self.tools.lock().await;

            for mcp_tool in &mcp_tools {
                let tool_name = mcp_tool.name.to_string();
                let input_schema: Value =
                    serde_json::to_value(&*mcp_tool.input_schema)
                        .unwrap_or_else(|_| Value::Object(Map::new()));

                let description = mcp_tool
                    .description
                    .as_deref()
                    .unwrap_or("")
                    .to_string();

                // Check tool-level config from McpServerConfig
                let tool_config = config.tool_configs.get(&tool_name);
                let enabled = tool_config.map(|c| c.enabled).unwrap_or(true);
                let confirmation = tool_config
                    .map(|c| c.confirmation.clone())
                    .unwrap_or(ConfirmationMode::AutoAllow);

                // Store tool info (including disabled/denied tools for frontend visibility)
                tool_details.push(McpToolInfo {
                    name: tool_name.clone(),
                    description: description.clone(),
                    enabled,
                    confirmation: confirmation.as_str().to_string(),
                });

                // Skip tool if disabled or denied
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

                let wrapper = McpToolWrapper {
                    inner_name: tool_name.clone(),
                    inner_description: description,
                    inner_parameters: input_schema,
                    peer: peer.clone(),
                    confirmation,
                    total_calls: total_calls.clone(),
                    error_count: error_count.clone(),
                    latency_samples: latency_samples.clone(),
                    last_error: last_error.clone(),
                };

                // Unregister previous version if exists
                if tool_registry.is_registered(&tool_name) {
                    tool_registry.unregister(&tool_name);
                }
                tool_registry.register_dynamic(&tool_name, Arc::new(wrapper), true);
                tool_names.push(tool_name);
            }
        }

        // 8. Set status to Ready
        {
            let mut s = status.lock().unwrap_or_else(|e| e.into_inner());
            *s = ConnectionStatus::Ready;
        }

        let connection = ActiveConnection {
            name: config.name.clone(),
            running,
            tool_names,
            tools: tool_details,
            started_at,
            total_calls,
            error_count,
            latency_samples,
            last_error,
            status: status.clone(),
            stderr_buffer: stderr_buffer.clone(),
        };

        let conn_id = config.id.clone();
        guard.insert(conn_id.clone(), connection);

        // 9. Spawn health check worker (monitors connection liveness)
        let health_interval = config.startup.health_check_interval_ms.max(5000); // min 5s
        let connections_arc = self.connections.clone();
        let name = config.name.clone();
        tokio::spawn(async move {
            loop {
                sleep(Duration::from_millis(health_interval)).await;
                let mut guard = connections_arc.lock().await;
                if let Some(conn) = guard.get_mut(&conn_id) {
                    let is_closed = conn.running.is_closed();
                    let current_status = {
                        let s = conn.status.lock().unwrap_or_else(|e| e.into_inner());
                        s.clone()
                    };
                    if is_closed {
                        if current_status.is_active() {
                            // Connection dropped unexpectedly
                            let mut s = conn.status.lock().unwrap_or_else(|e| e.into_inner());
                            let error_msg = "Connection closed unexpectedly (process may have crashed)".to_string();
                            {
                                if let Ok(mut last) = conn.last_error.lock() {
                                    *last = Some(error_msg.clone());
                                }
                            }
                            *s = ConnectionStatus::Error(error_msg);
                            log::warn!(
                                "MCP health: '{}' connection lost, marked as Error",
                                name
                            );
                        }
                        // If already error/stopped, no need to update again
                        break; // Stop the health worker
                    } else {
                        // Connection is alive — if it was degraded, restore to Ready
                        if matches!(current_status, ConnectionStatus::Degraded) {
                            let mut s = conn.status.lock().unwrap_or_else(|e| e.into_inner());
                            *s = ConnectionStatus::Ready;
                            log::info!("MCP health: '{}' restored to Ready", name);
                        }
                    }
                } else {
                    // Connection has been removed (disconnected)
                    break;
                }
            }
        });

        log::info!(
            "MCP server '{}' connected with {} tools",
            config.name,
            mcp_tools.len()
        );
        Ok(())
    }

    /// Disconnect from an MCP server.
    pub async fn disconnect(&self, id: &str) -> Result<()> {
        let mut guard = self.connections.lock().await;
        self.disconnect_inner(id, &mut guard).await;
        Ok(())
    }

    async fn disconnect_inner(
        &self,
        id: &str,
        guard: &mut HashMap<String, ActiveConnection>,
    ) {
        if let Some(conn) = guard.remove(id) {
            // Mark as stopping
            {
                let mut s = conn.status.lock().unwrap_or_else(|e| e.into_inner());
                *s = ConnectionStatus::Stopping;
            }

            // Unregister all tools of this connection from ToolRegistry
            {
                let mut tool_registry = self.tools.lock().await;
                for tool_name in &conn.tool_names {
                    if tool_registry.is_registered(tool_name) {
                        tool_registry.unregister(tool_name);
                    }
                }
            }
            // Close the MCP connection
            let mut running = conn.running;
            if let Err(e) = running.close().await {
                log::warn!("Error closing MCP connection '{}': {}", id, e);
            }
            log::info!("MCP server '{}' disconnected", conn.name);
        }
    }

    /// Restart an MCP server by disconnecting only.
    /// Callers should use disconnect + connect pattern for full restart.
    pub async fn restart(&self, id: &str) -> Result<()> {
        self.disconnect(id).await
    }

    /// List all connections with status.
    pub async fn list_connections(&self) -> Vec<ConnectionInfo> {
        let guard = self.connections.lock().await;
        guard
            .iter()
            .map(|(id, conn)| {
                let stats = Self::compute_stats(conn);
                let (status_str, detail) = {
                    let s = conn.status.lock().unwrap_or_else(|e| e.into_inner());
                    (s.as_str().to_string(), s.error_message().map(|e| e.to_string()))
                };
                ConnectionInfo {
                    id: id.clone(),
                    name: conn.name.clone(),
                    status: status_str,
                    status_detail: detail.clone(),
                    tool_count: conn.tool_names.len(),
                    tools: conn.tools.clone(),
                    stats,
                    error: detail,
                }
            })
            .collect()
    }

    /// Get detailed tool info for a specific connection.
    pub async fn get_connection_tools(&self, id: &str) -> Vec<McpToolInfo> {
        let guard = self.connections.lock().await;
        guard
            .get(id)
            .map(|conn| conn.tools.clone())
            .unwrap_or_default()
    }

    /// Get health and usage stats for a specific connection.
    pub async fn get_connection_stats(&self, id: &str) -> Option<ConnectionStats> {
        let guard = self.connections.lock().await;
        guard.get(id).map(|conn| Self::compute_stats(conn))
    }

    /// Get stderr logs for a specific connection.
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

    /// Get current status for a specific connection.
    pub async fn get_connection_status(&self, id: &str) -> Option<(String, Option<String>)> {
        let guard = self.connections.lock().await;
        guard.get(id).map(|conn| {
            let s = conn.status.lock().unwrap_or_else(|e| e.into_inner());
            (s.as_str().to_string(), s.error_message().map(|e| e.to_string()))
        })
    }

    fn compute_stats(conn: &ActiveConnection) -> ConnectionStats {
        let uptime = conn.started_at.elapsed().as_secs();
        let total_calls = conn.total_calls.load(Ordering::Relaxed);
        let error_count = conn.error_count.load(Ordering::Relaxed);
        let avg_latency_ms = {
            let samples = conn
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

    /// Shutdown all connections.
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

    /// Connect to all configured servers, respecting startup policy.
    /// Handles priority ordering and delay between groups.
    pub async fn connect_all(&self, configs: &[McpServerConfig]) {
        // Build priority groups from configs where should_auto_connect is true
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

        // Sort priority keys ascending
        let mut priorities: Vec<u32> = groups.keys().copied().collect();
        priorities.sort_unstable();

        for (i, pri) in priorities.iter().enumerate() {
            let cfgs = groups.remove(pri).unwrap_or_default();
            log::info!(
                "MCP auto-connect: starting priority group {} ({} server(s))",
                pri,
                cfgs.len()
            );

            // Start all servers in this priority group in parallel
            let handles: Vec<_> = cfgs
                .iter()
                .map(|cfg| {
                    let id = cfg.id.clone();
                    let name = cfg.name.clone();
                    let delay = cfg.startup.delay_ms;
                    async move {
                        if delay > 0 {
                            sleep(Duration::from_millis(delay)).await;
                        }
                        (id, name)
                    }
                })
                .collect();

            // Wait for all in this group before moving to next
            for handle in handles {
                let (cfg_id, cfg_name) = handle.await;
                // Find the config again (it moved into the handle above)
                let cfg = configs.iter().find(|c| c.id == cfg_id).cloned();
                if let Some(cfg) = cfg {
                    // Retry logic
                    let max_retries = cfg.startup.max_retries;
                    let retry_delay = cfg.startup.retry_delay_ms;
                    let mut attempt = 0;
                    loop {
                        attempt += 1;
                        match self.connect(&cfg).await {
                            Ok(()) => break,
                            Err(e) => {
                                if attempt <= max_retries {
                                    log::warn!(
                                        "MCP connect '{}' attempt {}/{} failed: {}. Retrying in {}ms...",
                                        cfg_name,
                                        attempt,
                                        max_retries + 1,
                                        e,
                                        retry_delay
                                    );
                                    sleep(Duration::from_millis(retry_delay)).await;
                                } else {
                                    log::error!(
                                        "MCP connect '{}' failed after {} attempts: {}",
                                        cfg_name,
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

            // Delay between priority groups (if not last)
            if i < priorities.len() - 1 {
                sleep(Duration::from_millis(500)).await;
            }
        }
    }
}

/// Wrapper that makes a Tool backed by an MCP peer.
/// Calls call_tool() via the cloned Peer<RoleClient>.
pub struct McpToolWrapper {
    inner_name: String,
    inner_description: String,
    inner_parameters: Value,
    peer: rmcp::service::Peer<rmcp::service::RoleClient>,
    confirmation: ConfirmationMode,
    total_calls: Arc<AtomicU64>,
    error_count: Arc<AtomicU64>,
    latency_samples: Arc<std::sync::Mutex<Vec<u64>>>,
    last_error: Arc<std::sync::Mutex<Option<String>>>,
}

impl ToolDyn for McpToolWrapper {
    fn name(&self) -> String {
        self.inner_name.clone()
    }

    fn definition<'a>(
        &'a self,
        _prompt: String,
    ) -> Pin<Box<dyn Future<Output = ToolDefinition> + Send + 'a>> {
        Box::pin(async move {
            ToolDefinition {
                name: self.inner_name.clone(),
                description: self.inner_description.clone(),
                parameters: self.inner_parameters.clone(),
            }
        })
    }

    fn call<'a>(
        &'a self,
        args: String,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<String, ToolError>> + Send + 'a>>
    {
        Box::pin(async move {
            // Parse JSON args from string
            let input: Value = serde_json::from_str(&args)
                .map_err(|e| ToolError::JsonError(e))?;

            // Check confirmation mode — deny if configured
            if matches!(self.confirmation, ConfirmationMode::Deny) {
                return Err(ToolError::ToolCallError(
                    format!(
                        "MCP tool '{}' execution denied by configuration",
                        self.inner_name
                    )
                    .into(),
                ));
            }

            let start = Instant::now();

            // The arguments must be a JSON object (Map) for CallToolRequestParams
            let arguments = match input {
                Value::Object(map) => map,
                other => {
                    let mut map = Map::new();
                    map.insert("value".to_string(), other);
                    map
                }
            };

            let tool_name = self.inner_name.clone();
            let params = rmcp::model::CallToolRequestParams::new(tool_name)
                .with_arguments(arguments);

            let result = self.peer.call_tool(params).await.map_err(|e| {
                self.error_count.fetch_add(1, Ordering::Relaxed);
                {
                    if let Ok(mut last) = self.last_error.lock() {
                        *last = Some(e.to_string());
                    }
                }
                ToolError::ToolCallError(
                    format!("MCP tool '{}' error: {}", self.inner_name, e).into(),
                )
            })?;

            // Track success metrics
            self.total_calls.fetch_add(1, Ordering::Relaxed);
            let elapsed_ns = start.elapsed().as_nanos() as u64;
            if let Ok(mut samples) = self.latency_samples.lock() {
                samples.push(elapsed_ns);
                if samples.len() > 100 {
                    samples.remove(0);
                }
            }

            // Convert MCP content array to JSON value, then serialize to string
            let result_value = if result.content.is_empty() {
                Value::Null
            } else if result.content.len() == 1 {
                if let Some(text) = result.content[0].as_text() {
                    Value::String(text.text.clone())
                } else {
                    let items: Vec<Value> = result
                        .content
                        .iter()
                        .map(|c| {
                            if let Some(text) = c.as_text() {
                                Value::String(text.text.clone())
                            } else {
                                Value::String(format!("{:?}", c))
                            }
                        })
                        .collect();
                    Value::Array(items)
                }
            } else {
                let items: Vec<Value> = result
                    .content
                    .iter()
                    .map(|c| {
                        if let Some(text) = c.as_text() {
                            Value::String(text.text.clone())
                        } else {
                            Value::String(format!("{:?}", c))
                        }
                    })
                    .collect();
                Value::Array(items)
            };

            serde_json::to_string(&result_value)
                .map_err(|e| ToolError::JsonError(e))
        })
    }
}
