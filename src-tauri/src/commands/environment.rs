// ── Runtime Environment IPC Commands ──

use tauri::{Emitter, State};

use crate::environment::{AvailableVersion, InstallProgress, InstalledVersion, RuntimeInfo, RuntimeType};
use crate::error::Result;
use crate::state::AppState;

/// List all runtimes with detection status.
#[tauri::command]
pub async fn list_runtimes(state: State<'_, AppState>) -> Result<Vec<RuntimeInfo>> {
    Ok(state.runtime_manager.detect_all().await)
}

/// Get cached runtime info (fast, no re-detection).
#[tauri::command]
pub async fn get_cached_runtimes(state: State<'_, AppState>) -> Result<Vec<RuntimeInfo>> {
    Ok(state.runtime_manager.get_all_cached().await)
}

/// Re-detect a specific runtime.
#[tauri::command]
pub async fn refresh_runtime(
    state: State<'_, AppState>,
    runtime_type: String,
) -> Result<RuntimeInfo> {
    let rt = parse_runtime_type(&runtime_type)?;
    Ok(state.runtime_manager.detect(&rt).await)
}

/// Install a runtime (built-in). Supports optional version parameter.
/// Emits `install_progress` events.
#[tauri::command]
pub async fn install_runtime(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    runtime_type: String,
    version: Option<String>,
) -> Result<RuntimeInfo> {
    let rt = parse_runtime_type(&runtime_type)?;
    let app_handle_clone = app_handle.clone();
    let on_progress = move |progress: InstallProgress| {
        let _ = app_handle_clone.emit("install_progress", &progress);
    };
    state.runtime_manager.install_runtime(&rt, version, on_progress).await
}

/// List available versions for download.
#[tauri::command]
pub async fn list_available_versions(
    state: State<'_, AppState>,
    runtime_type: String,
) -> Result<Vec<AvailableVersion>> {
    let rt = parse_runtime_type(&runtime_type)?;
    Ok(state.runtime_manager.list_available_versions(&rt).await)
}

/// List installed versions of a runtime.
#[tauri::command]
pub async fn list_installed_versions(
    state: State<'_, AppState>,
    runtime_type: String,
) -> Result<Vec<InstalledVersion>> {
    let rt = parse_runtime_type(&runtime_type)?;
    Ok(state.runtime_manager.list_installed_versions(&rt).await)
}

/// Switch the active version for a runtime.
#[tauri::command]
pub async fn switch_runtime_version(
    state: State<'_, AppState>,
    runtime_type: String,
    version: String,
) -> Result<RuntimeInfo> {
    let rt = parse_runtime_type(&runtime_type)?;
    state.runtime_manager.switch_version(&rt, &version).await?;
    Ok(state.runtime_manager.detect(&rt).await)
}

/// Uninstall a specific version.
#[tauri::command]
pub async fn uninstall_runtime_version(
    state: State<'_, AppState>,
    runtime_type: String,
    version: String,
) -> Result<RuntimeInfo> {
    let rt = parse_runtime_type(&runtime_type)?;
    state.runtime_manager.uninstall_version(&rt, &version).await?;
    Ok(state.runtime_manager.detect(&rt).await)
}

/// Get or set the runtime install directory.
/// If dir is provided, updates the path. Always returns the current path.
#[tauri::command]
pub async fn get_runtime_install_dir(state: State<'_, AppState>) -> Result<String> {
    let path = state.runtime_manager.get_install_dir().await;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn set_runtime_install_dir(
    state: State<'_, AppState>,
    dir: String,
) -> Result<String> {
    let new_dir = std::path::PathBuf::from(&dir);
    state.runtime_manager.set_install_dir(new_dir.clone()).await;
    // Persist to config
    {
        let mut config = state.config.lock().await;
        config.runtime_install_dir = Some(dir);
        config.save()?;
    }
    let path = state.runtime_manager.get_install_dir().await;
    Ok(path.to_string_lossy().to_string())
}

/// Validate a runtime (for use by MCP module).
#[tauri::command]
pub async fn validate_runtime(
    state: State<'_, AppState>,
    runtime_type: String,
) -> Result<String> {
    let rt = parse_runtime_type(&runtime_type)?;
    match state.runtime_manager.validate_runtime(&rt).await {
        Ok(()) => Ok(format!("✅ {} 可用", rt.display_name())),
        Err(msg) => Ok(msg),
    }
}

/// Suggest runtime info for a given CLI command (for MCP dialog).
#[tauri::command]
pub async fn suggest_runtime_for_command(
    state: State<'_, AppState>,
    command: String,
) -> Result<serde_json::Value> {
    let rt = RuntimeType::infer_from_command(&command);
    match rt {
        Some(runtime_type) => {
            let info = state.runtime_manager.detect(&runtime_type).await;
            Ok(serde_json::json!({
                "runtime_type": runtime_type,
                "available": info.available,
                "version": info.version,
                "source": info.source,
                "display_name": runtime_type.display_name(),
            }))
        }
        None => Ok(serde_json::json!({
            "runtime_type": null,
            "available": false,
            "display_name": null,
            "error": format!("无法从命令 '{}' 推断运行时类型", command),
        })),
    }
}

fn parse_runtime_type(s: &str) -> Result<RuntimeType> {
    match s {
        "node" => Ok(RuntimeType::Node),
        "python" => Ok(RuntimeType::Python),
        "docker" => Ok(RuntimeType::Docker),
        "uv" => Ok(RuntimeType::Uv),
        "go" => Ok(RuntimeType::Go),
        _ => Err(crate::error::AppError::InvalidInput(format!(
            "未知运行时类型: {}. 支持: node, python, docker, uv, go", s
        ))),
    }
}
