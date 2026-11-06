//! 工具开关 IPC 命令。
//!
//! 「已注册」与「已启用」是两件事：注册表里的工具始终存在，是否对模型
//! 可见由 `AppConfig::enabled_tools` 决定。

use tauri::State;

use crate::core::error::{AppError, Result};
use crate::app::state::AppState;
use crate::agent::tools::base::ToolInfo;

/// 列出所有已注册工具。
///
/// `enabled` 直接来自注册表——它在构造时已按配置初始化、在 `toggle_tool`
/// 时同步更新，这里再用配置覆盖只会引入两份真相。
#[tauri::command]
pub async fn list_tools(state: State<'_, AppState>) -> Result<Vec<ToolInfo>> {
    let tools = state.tools.lock().await;
    Ok(tools.catalog())
}

/// 启用或禁用某个工具，并同步到配置。
///
/// 工具未注册时返回 `NotFound`（禁用不存在的工具没有意义）。
#[tauri::command]
pub async fn toggle_tool(state: State<'_, AppState>, name: String, enabled: bool) -> Result<()> {
    let mut tools = state.tools.lock().await;
    let mut config = state.config.lock().await;

    // 先确认工具存在。
    if !tools.list().iter().any(|t| t.name == name) {
        return Err(AppError::NotFound(format!("Tool {} not found", name)));
    }

    tools.toggle(&name, enabled)?;

    if enabled {
        if !config.enabled_tools.contains(&name) {
            config.enabled_tools.push(name);
        }
    } else {
        config.enabled_tools.retain(|t| t != &name);
    }

    config.save()
}

/// 回复一次工具审批请求。
///
/// 找不到对应请求（已超时或已回复）时返回 `NotFound`，前端据此收起弹窗。
#[tauri::command]
pub async fn respond_tool_approval(
    state: State<'_, AppState>,
    id: String,
    approved: bool,
) -> Result<()> {
    if state.approval.respond(&id, approved) {
        Ok(())
    } else {
        Err(AppError::NotFound(format!(
            "Tool approval request '{}' not found or already answered",
            id
        )))
    }
}
