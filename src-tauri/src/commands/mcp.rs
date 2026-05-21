use std::collections::HashMap;

use tauri::State;

use crate::error::Result;
use crate::mcp::config::{ConfirmationMode, McpServerConfig, StartupPolicy, ToolConfig};
use crate::mcp::manager::{ConnectionInfo, ConnectionStats};
use crate::state::AppState;

#[tauri::command]
pub async fn list_mcp_connections(state: State<'_, AppState>) -> Result<Vec<ConnectionInfo>> {
    Ok(state.mcp.list_connections().await)
}

#[tauri::command]
pub async fn add_mcp_server(
    state: State<'_, AppState>,
    name: String,
    command: String,
    args: Vec<String>,
    runtime: Option<String>,
) -> Result<ConnectionInfo> {
    // Validate runtime if provided
    if let Some(ref rt_str) = runtime {
        if !rt_str.is_empty() && rt_str != "auto" {
            if let Some(rt) = crate::environment::RuntimeType::from_str(rt_str) {
                if let Err(msg) = state.runtime_manager.validate_runtime(&rt).await {
                    return Err(crate::error::AppError::InvalidInput(msg));
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

    // Connect to the server
    state.mcp.connect(&config).await?;

    // Save to config
    {
        let mut app_config = state.config.lock().await;
        app_config.mcp_servers.push(config);
        app_config.save()?;
    }

    // Return connection info
    let infos = state.mcp.list_connections().await;
    Ok(infos.into_iter().find(|c| c.id == id).unwrap())
}

#[tauri::command]
pub async fn remove_mcp_server(state: State<'_, AppState>, id: String) -> Result<()> {
    // Disconnect
    state.mcp.disconnect(&id).await?;

    // Remove from config
    {
        let mut app_config = state.config.lock().await;
        app_config.mcp_servers.retain(|s| s.id != id);
        app_config.save()?;
    }

    Ok(())
}

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
                crate::error::AppError::NotFound(format!("MCP server '{}' not found", id))
            })?
    };

    state.mcp.connect(&config).await
}

#[tauri::command]
pub async fn disconnect_mcp_server(state: State<'_, AppState>, id: String) -> Result<()> {
    state.mcp.disconnect(&id).await
}

/// Restart an MCP server: disconnect then reconnect.
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
                crate::error::AppError::NotFound(format!("MCP server '{}' not found", id))
            })?
    };

    // Disconnect first, then reconnect
    state.mcp.disconnect(&id).await?;
    // Small delay to ensure clean shutdown
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    state.mcp.connect(&config).await
}

/// Get stderr logs for a specific MCP server connection.
#[tauri::command]
pub async fn get_mcp_server_logs(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<String>> {
    Ok(state.mcp.get_connection_logs(&id).await)
}

/// Update the tool-level configuration (enabled/disabled, confirmation mode)
/// for a specific tool on a connected MCP server.
/// Automatically hot-reloads the server so the new config takes effect.
#[tauri::command]
pub async fn update_mcp_tool_config(
    state: State<'_, AppState>,
    connection_id: String,
    tool_name: String,
    enabled: bool,
    confirmation: String,
) -> Result<()> {
    let mode = ConfirmationMode::from_str(&confirmation);

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

    // If tool config changed and server is connected, hot-reload
    if needs_reconnect {
        if let Some((is_active, _)) = state.mcp.get_connection_status(&connection_id).await {
            if is_active == "ready" || is_active == "degraded" {
                log::info!("Tool config changed for '{}', hot-reloading MCP server", connection_id);
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

/// Update the startup policy for a specific MCP server.
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

/// Get health and usage statistics for a specific MCP connection.
#[tauri::command]
pub async fn get_mcp_connection_stats(
    state: State<'_, AppState>,
    id: String,
) -> Result<ConnectionStats> {
    state
        .mcp
        .get_connection_stats(&id)
        .await
        .ok_or_else(|| {
            crate::error::AppError::NotFound(format!("MCP connection '{}' not found", id))
        })
}
