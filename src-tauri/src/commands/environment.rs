// ── Runtime Environment IPC Commands ──

use tauri::{Emitter, State};

use crate::environment::{
    check_updates, BoundProject, InstallProgress, InstalledVersion, PathConflict, ProjectDetector,
    ProjectRuntimeRequirement, ProjectScanResult, RuntimeDetector, RuntimeInfo, RuntimeType,
    SyncAction, SyncResult, VersionUpdate,
};
use crate::environment::registry::RuntimeVersion;
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

    // Prevent concurrent installation of the same runtime
    state.runtime_manager.try_begin_install(&rt).await
        .map_err(crate::error::AppError::InvalidInput)?;

    let app_handle_clone = app_handle.clone();
    let on_progress = move |progress: InstallProgress| {
        let _ = app_handle_clone.emit("install_progress", &progress);
    };

    let result = state.runtime_manager.install_runtime(&rt, version, on_progress).await;
    state.runtime_manager.end_install(&rt).await;
    result
}

/// List available versions for download from the registry (dynamic discovery).
#[tauri::command]
pub async fn list_available_versions(
    state: State<'_, AppState>,
    runtime_type: String,
) -> Result<Vec<RuntimeVersion>> {
    let rt = parse_runtime_type(&runtime_type)?;
    state.runtime_registry.get_versions(&rt).await
}

/// Force refresh the version cache for a runtime type.
#[tauri::command]
pub async fn refresh_version_cache(
    state: State<'_, AppState>,
    runtime_type: String,
) -> Result<Vec<RuntimeVersion>> {
    let rt = parse_runtime_type(&runtime_type)?;
    state.runtime_registry.force_refresh(&rt).await
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

/// Open a version's installation directory in the OS file manager.
#[tauri::command]
pub async fn open_version_directory(
    state: State<'_, AppState>,
    runtime_type: String,
    version: String,
) -> Result<String> {
    let rt = parse_runtime_type(&runtime_type)?;
    let install_dir = state.runtime_manager.get_install_dir().await;
    let version_dir = install_dir.join(rt.dir_name()).join(&version);

    if !version_dir.exists() {
        return Err(crate::error::AppError::NotFound(format!(
            "版本目录不存在: {}", version_dir.display()
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
            .map_err(|e| crate::error::AppError::InvalidInput(
                format!("打开目录失败: {}", e)
            ))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path_str)
            .spawn()
            .map_err(|e| crate::error::AppError::InvalidInput(
                format!("打开目录失败: {}", e)
            ))?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&path_str)
            .spawn()
            .map_err(|e| crate::error::AppError::InvalidInput(
                format!("打开目录失败: {}", e)
            ))?;
    }

    log::info!("已打开版本目录: {}", path_str);
    Ok(path_str)
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

// ── Project Binding Commands ──

/// Scan a project directory for runtime version requirements.
#[tauri::command]
pub async fn scan_project(path: String) -> Result<ProjectScanResult> {
    let p = std::path::PathBuf::from(&path);
    ProjectDetector::scan(&p).await
}

/// Add a project to bound projects (scan + persist).
#[tauri::command]
pub async fn add_bound_project(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<BoundProject> {
    let p = std::path::PathBuf::from(&path);
    let name = p
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    // Scan project requirements
    let scan = ProjectDetector::scan(&p).await?;

    let db = state.db.lock().await;
    let model = db.add_bound_project(&path, &name)?;

    // Store requirements as JSON
    if !scan.requirements.is_empty() {
        let json = serde_json::to_string(&scan.requirements)?;
        db.update_bound_project(&crate::db::models::BoundProjectModel {
            requirements: Some(json),
            ..model.clone()
        })?;
    }

    Ok(BoundProject {
        id: model.id,
        path: model.path,
        name: model.name,
        auto_sync: model.auto_sync,
        last_scan: if scan.requirements.is_empty() {
            None
        } else {
            Some(
                scan.requirements
                    .iter()
                    .map(|r| format!("{}: {}", r.runtime_type.display_name(), r.version_spec))
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        },
        requirements: scan.requirements,
        created_at: model.created_at,
        updated_at: model.updated_at,
    })
}

/// List all bound projects.
#[tauri::command]
pub async fn list_bound_projects(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<BoundProject>> {
    let db = state.db.lock().await;
    let models = db.list_bound_projects()?;
    let mut projects = Vec::new();
    for m in models {
        let requirements: Vec<ProjectRuntimeRequirement> = m
            .requirements
            .as_deref()
            .and_then(|r| serde_json::from_str(r).ok())
            .unwrap_or_default();
        projects.push(BoundProject {
            id: m.id,
            path: m.path,
            name: m.name,
            auto_sync: m.auto_sync,
            last_scan: m.last_scan,
            requirements,
            created_at: m.created_at,
            updated_at: m.updated_at,
        });
    }
    Ok(projects)
}

/// Remove a bound project.
#[tauri::command]
pub async fn remove_bound_project(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<()> {
    let db = state.db.lock().await;
    db.remove_bound_project(&id)?;
    Ok(())
}

/// Sync a project's runtime versions.
#[tauri::command]
pub async fn sync_project(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<SyncResult> {
    // Get project from DB
    let (model, requirements) = {
        let db = state.db.lock().await;
        let models = db.list_bound_projects()?;
        let model = models
            .iter()
            .find(|m| m.id == id)
            .cloned()
            .ok_or_else(|| crate::error::AppError::NotFound(format!("项目 {} 未找到", id)))?;
        let requirements: Vec<ProjectRuntimeRequirement> = model
            .requirements
            .as_deref()
            .and_then(|r| serde_json::from_str(r).ok())
            .unwrap_or_default();
        (model, requirements)
    };

    let mut actions = Vec::new();
    let mut all_success = true;

    for req in requirements {
        let rt = &req.runtime_type;
        let info = state.runtime_manager.detect(rt).await;
        let current = info.version.clone().unwrap_or_default();

        // Resolve the spec
        let target = state.version_resolver.resolve(rt, &req.version_spec).await;
        match target {
            Ok(target_ver) => {
                if current == target_ver {
                    actions.push(SyncAction {
                        runtime_type: rt.clone(),
                        action: "already_matched".into(),
                        from_version: Some(current),
                        to_version: target_ver,
                        success: true,
                    });
                    continue;
                }

                // Try to switch (install if needed)
                match state.runtime_manager.switch_version(rt, &target_ver).await {
                    Ok(()) => {
                        actions.push(SyncAction {
                            runtime_type: rt.clone(),
                            action: "switch".into(),
                            from_version: Some(current),
                            to_version: target_ver,
                            success: true,
                        });
                    }
                    Err(_) => {
                        // Version not installed
                        actions.push(SyncAction {
                            runtime_type: rt.clone(),
                            action: "skipped".into(),
                            from_version: Some(current),
                            to_version: target_ver,
                            success: false,
                        });
                        all_success = false;
                    }
                }
            }
            Err(_) => {
                actions.push(SyncAction {
                    runtime_type: rt.clone(),
                    action: "skipped".into(),
                    from_version: Some(current),
                    to_version: req.version_spec.clone(),
                    success: false,
                });
                all_success = false;
            }
        }
    }

    // Update last_scan timestamp
    {
        let db = state.db.lock().await;
        let now = chrono::Utc::now().to_rfc3339();
        db.update_bound_project(&crate::db::models::BoundProjectModel {
            last_scan: Some(now),
            ..model
        })?;
    }

    Ok(SyncResult {
        project_id: id,
        actions,
        success: all_success,
        error: if all_success { None } else { Some("部分同步失败".into()) },
    })
}

// ── Alias Commands ──

/// Set the default version for a runtime.
#[tauri::command]
pub async fn set_runtime_default(
    state: tauri::State<'_, AppState>,
    runtime_type: String,
    version: String,
) -> Result<()> {
    let rt = parse_runtime_type(&runtime_type)?;
    state.alias_manager.set_default(&rt, version).await;
    Ok(())
}

/// Get the default version for a runtime.
#[tauri::command]
pub async fn get_runtime_default(
    state: tauri::State<'_, AppState>,
    runtime_type: String,
) -> Result<Option<String>> {
    let rt = parse_runtime_type(&runtime_type)?;
    Ok(state.alias_manager.get_default(&rt).await)
}

// ── Version Resolution Commands ──

/// Resolve a version spec to an exact version.
#[tauri::command]
pub async fn resolve_version(
    state: tauri::State<'_, AppState>,
    runtime_type: String,
    version_spec: String,
) -> Result<String> {
    let rt = parse_runtime_type(&runtime_type)?;
    state.version_resolver.resolve(&rt, &version_spec).await
}

// ── Upgrade Check ──

/// Check for runtime version updates.
#[tauri::command]
pub async fn check_runtime_updates(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<VersionUpdate>> {
    check_updates(&state.runtime_manager, &state.runtime_registry).await
}

// ── PATH Conflict Detection ──

/// Detect PATH conflicts — multiple executables for the same runtime.
#[tauri::command]
pub async fn detect_path_conflicts(
    _state: State<'_, AppState>,
) -> Result<Vec<PathConflict>> {
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
        let rt = parse_runtime_type(&item.runtime_type)?;

        // Prevent concurrent installation of the same runtime
        if let Err(msg) = state.runtime_manager.try_begin_install(&rt).await {
            results.push(BatchInstallResult {
                runtime_type: item.runtime_type,
                version: item.version.clone().unwrap_or_default(),
                success: false,
                error: Some(msg),
            });
            continue;
        }

        let app_clone = app_handle.clone();
        let on_progress = move |progress: InstallProgress| {
            let _ = app_clone.emit("install_progress", &progress);
        };
        let install_result = state.runtime_manager.install_runtime(&rt, item.version.clone(), on_progress).await;
        state.runtime_manager.end_install(&rt).await;

        match install_result {
            Ok(info) => {
                results.push(BatchInstallResult {
                    runtime_type: item.runtime_type,
                    version: info.version.unwrap_or_default(),
                    success: info.available,
                    error: info.error,
                });
            }
            Err(e) => {
                results.push(BatchInstallResult {
                    runtime_type: item.runtime_type,
                    version: item.version.unwrap_or_default(),
                    success: false,
                    error: Some(e.to_string()),
                });
            }
        }
    }
    Ok(results)
}

fn parse_runtime_type(s: &str) -> Result<RuntimeType> {
    match s {
        "node" => Ok(RuntimeType::Node),
        "python" => Ok(RuntimeType::Python),
        "docker" => Ok(RuntimeType::Docker),
        "uv" => Ok(RuntimeType::Uv),
        "go" => Ok(RuntimeType::Go),
        "rust" | "rustc" => Ok(RuntimeType::Rust),
        "java" => Ok(RuntimeType::Java),
        "deno" => Ok(RuntimeType::Deno),
        "bun" => Ok(RuntimeType::Bun),
        "ruby" | "irb" | "gem" => Ok(RuntimeType::Ruby),
        _ => Err(crate::error::AppError::InvalidInput(format!(
            "未知运行时类型: {}. 支持: node, python, docker, uv, go, rust, java, deno, bun, ruby", s
        ))),
    }
}
