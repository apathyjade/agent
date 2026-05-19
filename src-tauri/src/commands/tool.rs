use tauri::State;

use crate::error::{AppError, Result};
use crate::state::AppState;
use crate::tools::r#trait::ToolInfo;

#[tauri::command]
pub async fn list_tools(state: State<'_, AppState>) -> Result<Vec<ToolInfo>> {
    let tools = state.tools.lock().await;
    let config = state.config.lock().await;

    let tool_list: Vec<ToolInfo> = tools
        .list()
        .into_iter()
        .map(|t| ToolInfo {
            enabled: config.enabled_tools.contains(&t.name),
            ..t
        })
        .collect();

    Ok(tool_list)
}

#[tauri::command]
pub async fn toggle_tool(state: State<'_, AppState>, name: String, enabled: bool) -> Result<()> {
    let mut tools = state.tools.lock().await;
    let mut config = state.config.lock().await;

    // Verify tool exists
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
