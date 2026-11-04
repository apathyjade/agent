// ── Toolchain IPC Commands ──
//
// Runtime detection, installation and version switching over IPC. Project
// binding commands live in `commands/project_env.rs`.

use tauri::{Emitter, State};

use crate::core::error::Result;
use crate::app::state::AppState;
use crate::runtime::toolchain::manager_detector::{self, VersionManager};
use crate::runtime::toolchain::registry::RuntimeVersion;
use crate::runtime::toolchain::{
    DiskUsageItem, InstallProgress, InstalledVersion, PathConflict, RuntimeDetector, RuntimeInfo,
    RuntimeType, VersionUpdate,
};

/// List all runtimes with detection status.
#[tauri::command]
pub async fn list_runtimes(state: State<'_, AppState>) -> Result<Vec<RuntimeInfo>> {
    Ok(state.toolchain.manager.detect_all().await)
}

/// Get cached runtime info (fast, no re-detection).
#[tauri::command]
pub async fn get_cached_runtimes(state: State<'_, AppState>) -> Result<Vec<RuntimeInfo>> {
    Ok(state.toolchain.manager.get_all_cached().await)
}

/// Re-detect a specific runtime.
#[tauri::command]
pub async fn refresh_runtime(
    state: State<'_, AppState>,
    runtime_type: String,
) -> Result<RuntimeInfo> {
    let rt: RuntimeType = runtime_type.parse()?;
    Ok(state.toolchain.manager.detect(&rt).await)
}

/// Install a runtime (built-in or via external manager if active).
/// Emits `install_progress` events.
#[tauri::command]
pub async fn install_runtime(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    runtime_type: String,
    version: Option<String>,
) -> Result<RuntimeInfo> {
    let rt: RuntimeType = runtime_type.parse()?;
    let manager = current_active_manager(&state, &rt).await;

    let app_handle_clone = app_handle.clone();
    let on_progress = move |progress: InstallProgress| {
        let _ = app_handle_clone.emit("install_progress", &progress);
    };

    state
        .toolchain
        .install(&rt, version, manager.as_deref(), on_progress)
        .await
}

/// List available versions for download from the registry (dynamic discovery).
#[tauri::command]
pub async fn list_available_versions(
    state: State<'_, AppState>,
    runtime_type: String,
) -> Result<Vec<RuntimeVersion>> {
    let rt: RuntimeType = runtime_type.parse()?;
    state.toolchain.registry.get_versions(&rt).await
}

/// Force refresh the version cache for a runtime type.
#[tauri::command]
pub async fn refresh_version_cache(
    state: State<'_, AppState>,
    runtime_type: String,
) -> Result<Vec<RuntimeVersion>> {
    let rt: RuntimeType = runtime_type.parse()?;
    state.toolchain.registry.force_refresh(&rt).await
}

/// List installed versions of a runtime.
#[tauri::command]
pub async fn list_installed_versions(
    state: State<'_, AppState>,
    runtime_type: String,
) -> Result<Vec<InstalledVersion>> {
    let rt: RuntimeType = runtime_type.parse()?;
    Ok(state.toolchain.manager.list_installed_versions(&rt).await)
}

/// Switch the active version for a runtime.
#[tauri::command]
pub async fn switch_runtime_version(
    state: State<'_, AppState>,
    runtime_type: String,
    version: String,
) -> Result<RuntimeInfo> {
    let rt: RuntimeType = runtime_type.parse()?;
    let manager = current_active_manager(&state, &rt).await;
    state
        .toolchain
        .switch(&rt, &version, manager.as_deref())
        .await
}

/// Uninstall a specific version.
#[tauri::command]
pub async fn uninstall_runtime_version(
    state: State<'_, AppState>,
    runtime_type: String,
    version: String,
) -> Result<RuntimeInfo> {
    let rt: RuntimeType = runtime_type.parse()?;
    let manager = current_active_manager(&state, &rt).await;
    state
        .toolchain
        .uninstall(&rt, &version, manager.as_deref())
        .await
}

/// Open a version's installation directory in the OS file manager.
#[tauri::command]
pub async fn open_version_directory(
    state: State<'_, AppState>,
    runtime_type: String,
    version: String,
) -> Result<String> {
    let rt: RuntimeType = runtime_type.parse()?;
    let install_dir = state.toolchain.manager.get_install_dir().await;
    let version_dir = install_dir.join(rt.dir_name()).join(&version);

    if !version_dir.exists() {
        return Err(crate::core::error::AppError::NotFound(format!(
            "版本目录不存在: {}",
            version_dir.display()
        )));
    }

    let path_str = version_dir.to_string_lossy().to_string();

    #[cfg(target_os = "windows")]
    {
        // explorer /select,<path> opens the folder with the file selected
        // But for a directory, just open it directly
        std::process::Command::new("explorer")
            .arg(&path_str)
            .spawn()
            .map_err(|e| crate::core::error::AppError::InvalidInput(format!("打开目录失败: {}", e)))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path_str)
            .spawn()
            .map_err(|e| crate::core::error::AppError::InvalidInput(format!("打开目录失败: {}", e)))?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&path_str)
            .spawn()
            .map_err(|e| crate::core::error::AppError::InvalidInput(format!("打开目录失败: {}", e)))?;
    }

    log::info!("已打开版本目录: {}", path_str);
    Ok(path_str)
}

/// Get or set the runtime install directory.
/// If dir is provided, updates the path. Always returns the current path.
#[tauri::command]
pub async fn get_runtime_install_dir(state: State<'_, AppState>) -> Result<String> {
    let path = state.toolchain.manager.get_install_dir().await;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn set_runtime_install_dir(state: State<'_, AppState>, dir: String) -> Result<String> {
    let new_dir = std::path::PathBuf::from(&dir);
    state
        .toolchain
        .manager
        .set_install_dir(new_dir.clone())
        .await;
    // Persist to config
    {
        let mut config = state.config.lock().await;
        config.runtime_install_dir = Some(dir);
        config.save()?;
    }
    let path = state.toolchain.manager.get_install_dir().await;
    Ok(path.to_string_lossy().to_string())
}

/// Validate a runtime (for use by MCP module).
#[tauri::command]
pub async fn validate_runtime(state: State<'_, AppState>, runtime_type: String) -> Result<String> {
    let rt: RuntimeType = runtime_type.parse()?;
    match state.toolchain.manager.validate_runtime(&rt).await {
        Ok(()) => Ok(format!("✅ {} 可用", rt.display_name())),
        Err(msg) => Ok(msg),
    }
}

// ── Version Manager Commands ──

/// Get all available version managers for a runtime type.
#[tauri::command]
pub async fn get_version_managers(runtime_type: String) -> Result<Vec<VersionManager>> {
    let rt: RuntimeType = runtime_type.parse()?;
    Ok(manager_detector::detect_managers(&rt))
}

/// Set the active manager for a runtime type (persisted to config).
#[tauri::command]
pub async fn set_active_manager(
    state: State<'_, AppState>,
    runtime_type: String,
    manager_id: String,
) -> Result<()> {
    let rt: RuntimeType = runtime_type.parse()?;
    let mut config = state.config.lock().await;
    config.active_managers.insert(rt.to_string(), manager_id);
    config.save()?;
    Ok(())
}

/// Get the active manager for a runtime type.
#[tauri::command]
pub async fn get_active_manager(
    state: State<'_, AppState>,
    runtime_type: String,
) -> Result<Option<String>> {
    let rt: RuntimeType = runtime_type.parse()?;
    let config = state.config.lock().await;
    Ok(config.active_managers.get(&rt.to_string()).cloned())
}

/// Install a version manager tool by downloading it from the given URL.
/// Returns a status message.
#[tauri::command]
pub async fn install_manager_tool(manager_id: String, download_url: String) -> Result<String> {
    let temp_dir = std::env::temp_dir().join(format!("manager_install_{}", manager_id));
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir)
            .map_err(|e| crate::core::error::AppError::InvalidInput(e.to_string()))?;
    }
    std::fs::create_dir_all(&temp_dir)
        .map_err(|e| crate::core::error::AppError::InvalidInput(e.to_string()))?;

    let url = download_url;
    let default_filename = format!("{}.tar.gz", manager_id);
    let filename = url.rsplit('/').next().unwrap_or(&default_filename);
    let dest = temp_dir.join(filename);

    // Download (async)
    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| crate::core::error::AppError::InvalidInput(format!("下载失败: {}", e)))?;

    let content = response
        .bytes()
        .await
        .map_err(|e| crate::core::error::AppError::InvalidInput(format!("读取响应失败: {}", e)))?;

    std::fs::write(&dest, &content)
        .map_err(|e| crate::core::error::AppError::InvalidInput(e.to_string()))?;

    // Try to extract — supports .tar.gz, .gz, .zip
    let extension = filename.to_lowercase();
    if extension.ends_with(".tar.gz") || extension.ends_with(".tgz") {
        let file = std::fs::File::open(&dest)
            .map_err(|e| crate::core::error::AppError::InvalidInput(e.to_string()))?;
        let decoder = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(decoder);
        archive
            .unpack(&temp_dir)
            .map_err(|e| crate::core::error::AppError::InvalidInput(format!("解压失败: {}", e)))?;
    } else if extension.ends_with(".zip") {
        let file = std::fs::File::open(&dest)
            .map_err(|e| crate::core::error::AppError::InvalidInput(e.to_string()))?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| crate::core::error::AppError::InvalidInput(format!("解压 zip 失败: {}", e)))?;
        archive
            .extract(&temp_dir)
            .map_err(|e| crate::core::error::AppError::InvalidInput(format!("解压 zip 失败: {}", e)))?;
    } else if extension.ends_with(".gz") {
        let file = std::fs::File::open(&dest)
            .map_err(|e| crate::core::error::AppError::InvalidInput(e.to_string()))?;
        let mut decoder = flate2::read::GzDecoder::new(file);
        let mut out = std::fs::File::create(temp_dir.join("output"))
            .map_err(|e| crate::core::error::AppError::InvalidInput(e.to_string()))?;
        std::io::copy(&mut decoder, &mut out)
            .map_err(|e| crate::core::error::AppError::InvalidInput(e.to_string()))?;
    }

    // Find executable in extracted files
    let mut installed_binary = None;
    if let Ok(entries) = std::fs::read_dir(&temp_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                if name == manager_id || name == manager_id.to_lowercase().as_str() {
                    installed_binary = Some(path.to_string_lossy().to_string());
                    break;
                }
            }
            if path.is_dir() {
                if let Ok(sub_entries) = std::fs::read_dir(&path) {
                    for sub in sub_entries.flatten() {
                        let sub_path = sub.path();
                        if sub_path.is_file() {
                            let name = sub_path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                            if name == manager_id || name == manager_id.to_lowercase().as_str() {
                                installed_binary = Some(sub_path.to_string_lossy().to_string());
                                break;
                            }
                        }
                    }
                }
            }
            if installed_binary.is_some() {
                break;
            }
        }
    }

    match installed_binary {
        Some(bin_path) => {
            let install_dir = dirs::config_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("agent")
                .join("bin");
            std::fs::create_dir_all(&install_dir)
                .map_err(|e| crate::core::error::AppError::InvalidInput(e.to_string()))?;

            let target_name = if cfg!(target_os = "windows") {
                format!("{}.exe", manager_id)
            } else {
                manager_id.clone()
            };
            let target = install_dir.join(&target_name);

            std::fs::copy(&bin_path, &target).map_err(|e| {
                crate::core::error::AppError::InvalidInput(format!("复制可执行文件失败: {}", e))
            })?;

            #[cfg(not(target_os = "windows"))]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))
                    .map_err(|e| crate::core::error::AppError::InvalidInput(e.to_string()))?;
            }

            // Cleanup temp
            let _ = std::fs::remove_dir_all(&temp_dir);

            Ok(format!("{} 已安装到 {}", manager_id, target.display()))
        }
        None => {
            let _ = std::fs::remove_dir_all(&temp_dir);
            Err(crate::core::error::AppError::InvalidInput(format!(
                "无法在下载文件中找到 {} 可执行文件",
                manager_id
            )))
        }
    }
}

/// Suggest runtime info for a given CLI command (for MCP dialog).
// `serde_json::json!` expands to code that clippy sees as an unwrap.
#[allow(clippy::disallowed_methods)]
#[tauri::command]
pub async fn suggest_runtime_for_command(
    state: State<'_, AppState>,
    command: String,
) -> Result<serde_json::Value> {
    if let Some(runtime_type) = RuntimeType::infer_from_command(&command) {
        let info = state.toolchain.manager.detect(&runtime_type).await;
        Ok(serde_json::json!({
            "runtime_type": runtime_type,
            "available": info.available,
            "version": info.version,
            "source": info.source,
            "display_name": runtime_type.display_name(),
        }))
    } else {
        Ok(serde_json::json!({
            "runtime_type": null,
            "available": false,
            "display_name": null,
            "error": format!("无法从命令 '{}' 推断运行时类型", command),
        }))
    }
}

// ── Alias Commands ──

/// Set the default version for a runtime.
#[tauri::command]
pub async fn set_runtime_default(
    state: tauri::State<'_, AppState>,
    runtime_type: String,
    version: String,
) -> Result<()> {
    let rt: RuntimeType = runtime_type.parse()?;
    state
        .toolchain
        .aliases
        .set_default(&rt, version.clone())
        .await;
    let mut config = state.config.lock().await;
    config.runtime_defaults.insert(rt.to_string(), version);
    config.save()?;
    Ok(())
}

/// Get the default version for a runtime.
#[tauri::command]
pub async fn get_runtime_default(
    state: tauri::State<'_, AppState>,
    runtime_type: String,
) -> Result<Option<String>> {
    let rt: RuntimeType = runtime_type.parse()?;
    Ok(state.toolchain.aliases.get_default(&rt).await)
}

// ── Version Resolution Commands ──

/// Resolve a version spec to an exact version.
#[tauri::command]
pub async fn resolve_version(
    state: tauri::State<'_, AppState>,
    runtime_type: String,
    version_spec: String,
) -> Result<String> {
    let rt: RuntimeType = runtime_type.parse()?;
    state.toolchain.resolver.resolve(&rt, &version_spec).await
}

// ── Upgrade Check ──

/// Check for runtime version updates.
#[tauri::command]
pub async fn check_runtime_updates(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<VersionUpdate>> {
    state.toolchain.check_updates().await
}

// ── PATH Conflict Detection ──

/// Detect PATH conflicts — multiple executables for the same runtime.
#[tauri::command]
pub async fn detect_path_conflicts(_state: State<'_, AppState>) -> Result<Vec<PathConflict>> {
    let mut conflicts = Vec::new();
    for rt in RuntimeType::all() {
        let detector = RuntimeDetector::new();
        let result = detector.detect_path_conflicts(rt).await;
        conflicts.extend(result);
    }
    Ok(conflicts)
}

// ── Batch Install ──

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BatchInstallItem {
    pub runtime_type: String,
    pub version: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BatchInstallResult {
    pub runtime_type: String,
    pub version: String,
    pub success: bool,
    pub error: Option<String>,
}

/// Batch install multiple runtimes at once, emitting install_progress per item.
#[tauri::command]
pub async fn batch_install_runtimes(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    installs: Vec<BatchInstallItem>,
) -> Result<Vec<BatchInstallResult>> {
    let mut results = Vec::new();
    for item in installs {
        let rt = item.runtime_type.parse()?;
        let manager = current_active_manager(&state, &rt).await;

        let app_clone = app_handle.clone();
        let on_progress = move |progress: InstallProgress| {
            let _ = app_clone.emit("install_progress", &progress);
        };

        match state
            .toolchain
            .install(&rt, item.version.clone(), manager.as_deref(), on_progress)
            .await
        {
            Ok(info) => {
                results.push(BatchInstallResult {
                    runtime_type: item.runtime_type,
                    version: info.version.unwrap_or_default(),
                    success: info.available,
                    error: info.error,
                });
            }
            Err(error) => {
                results.push(BatchInstallResult {
                    runtime_type: item.runtime_type,
                    version: item.version.unwrap_or_default(),
                    success: false,
                    error: Some(error.to_string()),
                });
            }
        }
    }
    Ok(results)
}

// ── Disk Usage ──

/// Get disk usage for all runtimes.
#[tauri::command]
pub async fn get_runtime_disk_usage(state: State<'_, AppState>) -> Result<Vec<DiskUsageItem>> {
    Ok(state.toolchain.manager.get_all_disk_usage().await)
}

/// Get the active manager ID for a runtime type, or None if not configured.
async fn current_active_manager(state: &State<'_, AppState>, rt: &RuntimeType) -> Option<String> {
    let config = state.config.lock().await;
    config.active_managers.get(&rt.to_string()).cloned()
}
