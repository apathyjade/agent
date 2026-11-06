//! MCP（Model Context Protocol）连接管理 IPC 命令。
//!
//! 负责 server 的增删、连接生命周期、工具级配置与健康统计。MCP 工具会
//! 动态注册进 `ToolRegistry`，与内置工具走同一套调用路径。

use std::collections::HashMap;

use tauri::State;

use crate::core::error::{AppError, Result};
use crate::features::mcp::config::{ConfirmationMode, McpServerConfig, StartupPolicy, ToolConfig};
use crate::features::mcp::manager::{ConnectionInfo, ConnectionStats};
use crate::app::state::AppState;

/// 列出所有 MCP 连接及其当前状态（`ready` / `degraded` / `failed` 等）。
#[tauri::command]
pub async fn list_mcp_connections(state: State<'_, AppState>) -> Result<Vec<ConnectionInfo>> {
    Ok(state.mcp.list_connections().await)
}

/// 新增 MCP server：校验运行时、立即连接、成功后才写入配置。
///
/// `runtime` 为空或 `"auto"` 表示不指定运行时；其他取值必须能解析且通过
/// 可用性校验，否则返回 `InvalidInput`。返回连接信息。
#[tauri::command]
pub async fn add_mcp_server(
    state: State<'_, AppState>,
    name: String,
    command: String,
    args: Vec<String>,
    runtime: Option<String>,
) -> Result<ConnectionInfo> {
    // 指定了运行时就先校验可用性。
    if let Some(ref rt_str) = runtime {
        if !rt_str.is_empty() && rt_str != "auto" {
            if let Ok(rt) = rt_str.parse::<crate::runtime::toolchain::RuntimeType>() {
                if let Err(msg) = state.toolchain.manager.validate_runtime(&rt).await {
                    return Err(crate::core::error::AppError::InvalidInput(msg));
                }
            }
        }
    }

    let id = uuid::Uuid::new_v4().to_string();
    let config = McpServerConfig {
        id: id.clone(),
        name,
        command,
        args,
        env: None,
        startup: StartupPolicy::default(),
        auto_connect: true,
        tool_configs: HashMap::new(),
        runtime: runtime.unwrap_or_default(),
    };

    // 先连接；连不上就不落配置。
    state.mcp.connect(&config).await?;

    // 写入配置。
    {
        let mut app_config = state.config.lock().await;
        app_config.mcp_servers.push(config);
        app_config.save()?;
    }

    // 返回连接信息。
    let infos = state.mcp.list_connections().await;
    infos.into_iter().find(|c| c.id == id).ok_or_else(|| {
        AppError::NotFound(format!("MCP connection '{}' not found after connect", id))
    })
}

/// 断开并从配置中移除 MCP server。
#[tauri::command]
pub async fn remove_mcp_server(state: State<'_, AppState>, id: String) -> Result<()> {
    // 先断开连接。
    state.mcp.disconnect(&id).await?;

    // 再从配置里删除。
    {
        let mut app_config = state.config.lock().await;
        app_config.mcp_servers.retain(|s| s.id != id);
        app_config.save()?;
    }

    Ok(())
}

/// 按配置连接一个已登记的 MCP server；`id` 不存在时返回 `NotFound`。
#[tauri::command]
pub async fn connect_mcp_server(state: State<'_, AppState>, id: String) -> Result<()> {
    let config = {
        let app_config = state.config.lock().await;
        app_config
            .mcp_servers
            .iter()
            .find(|s| s.id == id)
            .cloned()
            .ok_or_else(|| {
                crate::core::error::AppError::NotFound(format!("MCP server '{}' not found", id))
            })?
    };

    state.mcp.connect(&config).await
}

/// 断开连接但保留配置，之后仍可 [`connect_mcp_server`]。
#[tauri::command]
pub async fn disconnect_mcp_server(state: State<'_, AppState>, id: String) -> Result<()> {
    state.mcp.disconnect(&id).await
}

/// 重启 MCP server：先断开、等待 200ms 让进程干净退出，再重新连接。
#[tauri::command]
pub async fn restart_mcp_server(state: State<'_, AppState>, id: String) -> Result<()> {
    let config = {
        let app_config = state.config.lock().await;
        app_config
            .mcp_servers
            .iter()
            .find(|s| s.id == id)
            .cloned()
            .ok_or_else(|| {
                crate::core::error::AppError::NotFound(format!("MCP server '{}' not found", id))
            })?
    };

    // 必须先断开，再重连。
    state.mcp.disconnect(&id).await?;
    // 留一点时间确保旧进程完全退出。
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    state.mcp.connect(&config).await
}

/// 读取某个 MCP 连接的 stderr 日志（供排查启动失败）。
#[tauri::command]
pub async fn get_mcp_server_logs(state: State<'_, AppState>, id: String) -> Result<Vec<String>> {
    Ok(state.mcp.get_connection_logs(&id).await)
}

/// 更新某个 MCP 工具的配置（启用状态与确认模式）。
///
/// `confirmation` 是 `ConfirmationMode::parse` 能识别的字符串（如 `always`、
/// `never`）。配置真正发生变化且连接处于 `ready`/`degraded` 时，会自动
/// 热重载该 server；连接不存在时静默返回 `Ok`。
#[tauri::command]
pub async fn update_mcp_tool_config(
    state: State<'_, AppState>,
    connection_id: String,
    tool_name: String,
    enabled: bool,
    confirmation: String,
) -> Result<()> {
    let mode = ConfirmationMode::parse(&confirmation);

    let needs_reconnect = {
        let mut config = state.config.lock().await;
        if let Some(server) = config
            .mcp_servers
            .iter_mut()
            .find(|s| s.id == connection_id)
        {
            let changed = server
                .tool_configs
                .get(&tool_name)
                .map(|c| c.enabled != enabled || c.confirmation != mode)
                .unwrap_or(true);
            server.tool_configs.insert(
                tool_name,
                ToolConfig {
                    enabled,
                    confirmation: mode,
                },
            );
            config.save()?;
            changed
        } else {
            false
        }
    };

    // 配置确实变了且 server 正在运行：热重载使其生效。
    if needs_reconnect {
        if let Some((is_active, _)) = state.mcp.get_connection_status(&connection_id).await {
            if is_active == "ready" || is_active == "degraded" {
                log::info!(
                    "Tool config changed for '{}', hot-reloading MCP server",
                    connection_id
                );
                let config = {
                    let app_config = state.config.lock().await;
                    app_config
                        .mcp_servers
                        .iter()
                        .find(|s| s.id == connection_id)
                        .cloned()
                };
                if let Some(cfg) = config {
                    state.mcp.disconnect(&connection_id).await?;
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    state.mcp.connect(&cfg).await?;
                }
            }
        }
    }

    Ok(())
}

/// 更新某个 MCP server 的启动策略。
///
/// 每个参数都是可选的：`None` 表示保持原值。`priority` 传负数表示清空为
/// `None`（不参与启动排序）。server 不存在时静默返回 `Ok`。
#[tauri::command]
pub async fn update_mcp_startup_policy(
    state: State<'_, AppState>,
    id: String,
    launch_on_startup: Option<bool>,
    launch_on_demand: Option<bool>,
    priority: Option<i32>,
    max_retries: Option<u32>,
    health_check_interval_ms: Option<u64>,
) -> Result<()> {
    let mut config = state.config.lock().await;
    if let Some(server) = config.mcp_servers.iter_mut().find(|s| s.id == id) {
        if let Some(v) = launch_on_startup {
            server.startup.launch_on_startup = v;
        }
        if let Some(v) = launch_on_demand {
            server.startup.launch_on_demand = v;
        }
        if let Some(v) = priority {
            server.startup.priority = if v < 0 { None } else { Some(v as u32) };
        }
        if let Some(v) = max_retries {
            server.startup.max_retries = v;
        }
        if let Some(v) = health_check_interval_ms {
            server.startup.health_check_interval_ms = v;
        }
        config.save()?;
    }
    Ok(())
}

/// 读取某个 MCP 连接的健康与调用统计；连接不存在时返回 `NotFound`。
#[tauri::command]
pub async fn get_mcp_connection_stats(
    state: State<'_, AppState>,
    id: String,
) -> Result<ConnectionStats> {
    state.mcp.get_connection_stats(&id).await.ok_or_else(|| {
        crate::core::error::AppError::NotFound(format!("MCP connection '{}' not found", id))
    })
}
