//! 工具链 IPC 命令。
//!
//! 运行时检测、安装、版本切换的对外接口。项目绑定相关命令在
//! `app/commands/project_env.rs`；两者共享同一个 `RuntimeServices`。
//!
//! 所有命令都通过 `runtime_type: String` 接收运行时类型（`node`、`python`
//! 等），解析失败返回 `InvalidInput`，不会 panic。

use tauri::{Emitter, State};

use crate::app::state::AppState;
use crate::core::error::{AppError, Result};
use crate::runtime::toolchain::manager_detector::{self, VersionManager};
use crate::runtime::toolchain::registry::RuntimeVersion;
use crate::runtime::toolchain::{
    DiskUsageItem, InstallProgress, InstalledVersion, PathConflict, RuntimeDetector, RuntimeInfo,
    RuntimeType, VersionUpdate,
};

// ── 检测 ──

/// 重新探测全部运行时并刷新缓存，返回探测结果。
///
/// 会真实执行 `node --version` 一类的命令，因此有明显耗时；界面首屏应改用
/// [`get_cached_runtimes`]。
#[tauri::command]
pub async fn list_runtimes(state: State<'_, AppState>) -> Result<Vec<RuntimeInfo>> {
    Ok(state.toolchain.manager.detect_all().await)
}

/// 读取上次探测写入的缓存，不执行任何外部命令。
///
/// 与 [`list_runtimes`] 的区别：这里只读内存/磁盘缓存，永远不会触发新的
/// 探测；缓存为空时返回空列表（或仅有未知项），而不是去现场检测。
#[tauri::command]
pub async fn get_cached_runtimes(state: State<'_, AppState>) -> Result<Vec<RuntimeInfo>> {
    Ok(state.toolchain.manager.get_all_cached().await)
}

/// 只重新探测**指定**运行时，并更新它的缓存项。
///
/// 与 [`list_runtimes`] 的区别：范围是单个运行时，适合用户手动点「刷新」
/// 的场景；不会影响其他运行时的缓存。
#[tauri::command]
pub async fn refresh_runtime(
    state: State<'_, AppState>,
    runtime_type: String,
) -> Result<RuntimeInfo> {
    let rt: RuntimeType = runtime_type.parse()?;
    Ok(state.toolchain.manager.detect(&rt).await)
}

// ── 安装 / 版本 ──

/// 安装指定版本（省略 `version` 时安装最新版）。
///
/// 若该运行时配置了外部版本管理器（如 fnm），安装会委托给它；否则使用
/// 内置下载安装。过程中通过 `install_progress` 事件推送进度。
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

/// 列出可从 registry 下载的版本（动态发现，可能走网络）。
#[tauri::command]
pub async fn list_available_versions(
    state: State<'_, AppState>,
    runtime_type: String,
) -> Result<Vec<RuntimeVersion>> {
    let rt: RuntimeType = runtime_type.parse()?;
    state.toolchain.registry.get_versions(&rt).await
}

/// 强制刷新某运行时的版本缓存，并返回刷新后的列表。
///
/// 与 [`list_available_versions`] 的区别：后者优先读缓存，这里一定重新
/// 拉取远端数据。
#[tauri::command]
pub async fn refresh_version_cache(
    state: State<'_, AppState>,
    runtime_type: String,
) -> Result<Vec<RuntimeVersion>> {
    let rt: RuntimeType = runtime_type.parse()?;
    state.toolchain.registry.force_refresh(&rt).await
}

/// 列出某运行时在本机已安装的版本（含来源与路径）。
#[tauri::command]
pub async fn list_installed_versions(
    state: State<'_, AppState>,
    runtime_type: String,
) -> Result<Vec<InstalledVersion>> {
    let rt: RuntimeType = runtime_type.parse()?;
    Ok(state.toolchain.manager.list_installed_versions(&rt).await)
}

/// 切换运行时版本。
///
/// 只在应用内生效（写别名/默认版本），不修改系统 PATH。
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

/// 卸载指定的已安装版本，返回卸载后的运行时状态。
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

/// 在系统文件管理器中打开某个版本的安装目录，返回该目录路径。
///
/// 目录不存在时返回 `NotFound`（而不是静默无操作）。
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
        // explorer /select,<path> 会打开父目录并选中文件；
        // 这里传入的是目录，直接打开即可。
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

/// 读取当前运行时安装目录（绝对路径字符串）。
#[tauri::command]
pub async fn get_runtime_install_dir(state: State<'_, AppState>) -> Result<String> {
    let path = state.toolchain.manager.get_install_dir().await;
    Ok(path.to_string_lossy().to_string())
}

/// 修改运行时安装目录并持久化到配置，返回生效后的路径。
///
/// 只改变后续安装的落点；已安装的版本不会被移动。
#[tauri::command]
pub async fn set_runtime_install_dir(state: State<'_, AppState>, dir: String) -> Result<String> {
    let new_dir = std::path::PathBuf::from(&dir);
    state
        .toolchain
        .manager
        .set_install_dir(new_dir.clone())
        .await;
    // 写入配置，重启后仍然生效。
    {
        let mut config = state.config.lock().await;
        config.runtime_install_dir = Some(dir);
        config.save()?;
    }
    let path = state.toolchain.manager.get_install_dir().await;
    Ok(path.to_string_lossy().to_string())
}

/// 检查运行时是否可用，返回**人类可读的状态文案**而不是错误。
///
/// 成功时是 `✅ <名称> 可用`，失败时是底层给出的原因（例如「未检测到
/// node」）。调用方（MCP 模块）依赖「永远 `Ok`」这一约定来展示提示，
/// 因此不要把不可用转换成 `Err`。
#[tauri::command]
pub async fn validate_runtime(state: State<'_, AppState>, runtime_type: String) -> Result<String> {
    let rt: RuntimeType = runtime_type.parse()?;
    match state.toolchain.manager.validate_runtime(&rt).await {
        Ok(()) => Ok(format!("✅ {} 可用", rt.display_name())),
        Err(msg) => Ok(msg),
    }
}

// ── 版本管理器 ──

/// 列出某运行时所有可用的外部版本管理器（如 fnm、pyenv）。
#[tauri::command]
pub async fn get_version_managers(runtime_type: String) -> Result<Vec<VersionManager>> {
    let rt: RuntimeType = runtime_type.parse()?;
    Ok(manager_detector::detect_managers(&rt))
}

/// 设置某运行时使用的版本管理器并持久化。
///
/// `manager_id` 必须是 [`get_version_managers`] 返回的 id；这里不做存在性
/// 校验，写错会在后续安装时才失败。
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

/// 读取某运行时当前配置的版本管理器 id；未配置时返回 `None`。
#[tauri::command]
pub async fn get_active_manager(
    state: State<'_, AppState>,
    runtime_type: String,
) -> Result<Option<String>> {
    let rt: RuntimeType = runtime_type.parse()?;
    let config = state.config.lock().await;
    Ok(config.active_managers.get(&rt.to_string()).cloned())
}

/// 下载并安装一个版本管理器可执行文件，返回安装结果说明。
///
/// 支持 `.tar.gz` / `.tgz` / `.zip` / 单文件 `.gz`；可执行文件会被复制到
/// `<config_dir>/agent/bin/`（Windows 下补 `.exe`）。
///
/// 返回值是**面向用户的一句话**（形如 `<id> 已安装到 <路径>`），前端直接
/// 展示；失败原因通过 `Err` 返回，而不是混在字符串里。
#[tauri::command]
pub async fn install_manager_tool(manager_id: String, download_url: String) -> Result<String> {
    let temp_dir = std::env::temp_dir().join(format!("manager_install_{}", manager_id));
    reset_temp_dir(&temp_dir)?;

    let filename = download_url
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{}.tar.gz", manager_id));
    let dest = temp_dir.join(&filename);

    download_to(&download_url, &dest).await?;
    extract_archive(&dest, &temp_dir, &filename)?;

    // 归档布局因项目而异：有的直接放在根目录，有的多套一层子目录。
    // 只向下探测一层，与历史行为保持一致。
    let Some(binary) = find_manager_binary(&temp_dir, &manager_id, 1) else {
        let _ = std::fs::remove_dir_all(&temp_dir);
        return Err(invalid_input(format!(
            "无法在下载文件中找到 {} 可执行文件",
            manager_id
        )));
    };

    let result = install_binary(&binary, &manager_id);
    let _ = std::fs::remove_dir_all(&temp_dir);
    result
}

// ── 建议 / 别名 / 解析 ──

// `serde_json::json!` 展开出的代码会被 clippy 判为 unwrap。
#[allow(clippy::disallowed_methods)]
/// 根据命令行内容推断它需要的运行时（供 MCP 对话框提示用）。
///
/// 返回的 JSON 字段：`runtime_type`、`available`、`version`、`source`、
/// `display_name`。推断不出来时 `runtime_type` 与 `display_name` 为 `null`，
/// 并带上 `error` 说明。
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

/// 设置某运行时的默认版本（同时写别名与配置）。
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

/// 读取某运行时的默认版本；未设置时返回 `None`。
#[tauri::command]
pub async fn get_runtime_default(
    state: tauri::State<'_, AppState>,
    runtime_type: String,
) -> Result<Option<String>> {
    let rt: RuntimeType = runtime_type.parse()?;
    Ok(state.toolchain.aliases.get_default(&rt).await)
}

/// 把版本规格（`latest`、`lts`、`20`、`^20.1` 等）解析成精确版本号。
#[tauri::command]
pub async fn resolve_version(
    state: tauri::State<'_, AppState>,
    runtime_type: String,
    version_spec: String,
) -> Result<String> {
    let rt: RuntimeType = runtime_type.parse()?;
    state.toolchain.resolver.resolve(&rt, &version_spec).await
}

/// 检查所有运行时是否有可升级版本（可能走网络）。
#[tauri::command]
pub async fn check_runtime_updates(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<VersionUpdate>> {
    state.toolchain.check_updates().await
}

/// 检测 PATH 冲突：同一运行时在 PATH 上有多个可执行文件。
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

// ── 批量安装 ──

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

/// 批量安装多个运行时，逐项 emit `install_progress`。
///
/// 单项失败不会中断整批：失败信息写进对应的 [`BatchInstallResult`]。
/// `runtime_type` 解析失败仍会让整个调用返回 `Err`（属于调用方参数错误）。
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

// ── 磁盘占用 ──

/// 统计所有运行时占用的磁盘空间。
#[tauri::command]
pub async fn get_runtime_disk_usage(state: State<'_, AppState>) -> Result<Vec<DiskUsageItem>> {
    Ok(state.toolchain.manager.get_all_disk_usage().await)
}

// ── 内部辅助 ──

/// 读取某运行时当前配置的版本管理器 id；未配置时返回 `None`。
async fn current_active_manager(state: &State<'_, AppState>, rt: &RuntimeType) -> Option<String> {
    let config = state.config.lock().await;
    config.active_managers.get(&rt.to_string()).cloned()
}

/// 把 IO/serde 错误统一包装成 `InvalidInput`。
fn invalid_input(error: impl std::fmt::Display) -> AppError {
    AppError::InvalidInput(error.to_string())
}

/// 清空并重建临时目录；不存在时直接创建。
fn reset_temp_dir(dir: &std::path::Path) -> Result<()> {
    if dir.exists() {
        std::fs::remove_dir_all(dir).map_err(invalid_input)?;
    }
    std::fs::create_dir_all(dir).map_err(invalid_input)?;
    Ok(())
}

/// 下载 `url` 并写入 `dest`。
async fn download_to(url: &str, dest: &std::path::Path) -> Result<()> {
    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| invalid_input(format!("下载失败: {}", e)))?;

    let content = response
        .bytes()
        .await
        .map_err(|e| invalid_input(format!("读取响应失败: {}", e)))?;

    std::fs::write(dest, &content).map_err(invalid_input)
}

/// 按扩展名解压归档：`.tar.gz`/`.tgz`、`.zip`、单文件 `.gz`。
///
/// 其他扩展名不做处理——后续「查找可执行文件」会因此失败并返回明确错误。
fn extract_archive(dest: &std::path::Path, temp_dir: &std::path::Path, filename: &str) -> Result<()> {
    let extension = filename.to_lowercase();

    if extension.ends_with(".tar.gz") || extension.ends_with(".tgz") {
        let file = std::fs::File::open(dest).map_err(invalid_input)?;
        let decoder = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(decoder);
        archive
            .unpack(temp_dir)
            .map_err(|e| invalid_input(format!("解压失败: {}", e)))?;
    } else if extension.ends_with(".zip") {
        let file = std::fs::File::open(dest).map_err(invalid_input)?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| invalid_input(format!("解压 zip 失败: {}", e)))?;
        archive
            .extract(temp_dir)
            .map_err(|e| invalid_input(format!("解压 zip 失败: {}", e)))?;
    } else if extension.ends_with(".gz") {
        let file = std::fs::File::open(dest).map_err(invalid_input)?;
        let mut decoder = flate2::read::GzDecoder::new(file);
        let mut out = std::fs::File::create(temp_dir.join("output")).map_err(invalid_input)?;
        std::io::copy(&mut decoder, &mut out).map_err(invalid_input)?;
    }

    Ok(())
}

/// 在解压目录中查找与管理器同名的可执行文件。
///
/// `depth` 控制额外下探的子目录层数：0 只看当前目录，1 再看一层。
fn find_manager_binary(
    dir: &std::path::Path,
    manager_id: &str,
    depth: usize,
) -> Option<std::path::PathBuf> {
    let wanted = manager_id.to_lowercase();
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_file() {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if stem == manager_id || stem.to_lowercase() == wanted {
                return Some(path);
            }
        } else if path.is_dir() && depth > 0 {
            if let Some(found) = find_manager_binary(&path, manager_id, depth - 1) {
                return Some(found);
            }
        }
    }
    None
}

/// 把可执行文件复制到 `<config_dir>/agent/bin/`，返回面向用户的说明。
fn install_binary(binary: &std::path::Path, manager_id: &str) -> Result<String> {
    let install_dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("agent")
        .join("bin");
    std::fs::create_dir_all(&install_dir).map_err(invalid_input)?;

    let target_name = if cfg!(target_os = "windows") {
        format!("{}.exe", manager_id)
    } else {
        manager_id.to_string()
    };
    let target = install_dir.join(&target_name);

    std::fs::copy(binary, &target)
        .map_err(|e| invalid_input(format!("复制可执行文件失败: {}", e)))?;

    #[cfg(not(target_os = "windows"))]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))
            .map_err(invalid_input)?;
    }

    Ok(format!("{} 已安装到 {}", manager_id, target.display()))
}
