//! 项目运行时绑定 IPC 命令。
//!
//! 这些命令是应用特有的：它们把项目目录登记进应用数据库，并持久化扫描
//! 出的运行时版本需求。工具链引擎本身在 `crate::runtime::toolchain`。

use tauri::State;

use crate::core::error::Result;
use crate::runtime::project_env::{
    service as project_service, BoundProject, ProjectDetector, ProjectScanResult, SyncResult,
};
use crate::app::state::AppState;

/// 扫描项目目录，识别 `.nvmrc`、`rust-toolchain.toml` 等文件里的版本要求。
///
/// 只读：不写数据库、不安装任何东西。
#[tauri::command]
pub async fn scan_project(path: String) -> Result<ProjectScanResult> {
    let path = std::path::PathBuf::from(&path);
    ProjectDetector::scan(&path).await
}

/// 扫描并登记一个项目（扫描 + 持久化），返回含 id 与需求的绑定记录。
#[tauri::command]
pub async fn add_bound_project(state: State<'_, AppState>, path: String) -> Result<BoundProject> {
    project_service::bind(&state.db, &path).await
}

/// 列出所有已绑定项目及其运行时需求。
///
/// 与 `list_projects` 的区别：这里返回的是**运行时绑定**（`BoundProject`），
/// 带扫描出的版本要求，供工具链页面使用；`list_projects` 只返回工作区记录。
#[tauri::command]
pub async fn list_bound_projects(state: State<'_, AppState>) -> Result<Vec<BoundProject>> {
    project_service::list(&state.db).await
}

/// 按 id 移除项目绑定。
///
/// 只删除绑定记录与需求快照，不会删除项目目录，也不卸载任何运行时。
#[tauri::command]
pub async fn remove_bound_project(state: State<'_, AppState>, id: String) -> Result<()> {
    project_service::remove(&state.db, &id).await
}

/// 把项目的运行时版本同步到需求版本（缺失的版本会先安装）。
///
/// 返回逐步执行结果；部分失败时 `success=false` 并带 `error`，但已成功的
/// 步骤不会回滚。
#[tauri::command]
pub async fn sync_project(state: State<'_, AppState>, id: String) -> Result<SyncResult> {
    let active_managers = state.config.lock().await.active_managers.clone();
    project_service::sync(&state.toolchain, &state.db, &id, &active_managers).await
}
