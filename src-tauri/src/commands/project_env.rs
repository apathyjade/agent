// ── Project environment IPC commands ──
//
// These commands are app-specific: they bind project directories and persist
// requirements in the application database. The toolchain engine itself lives
// in `crate::toolchain`.

use tauri::State;

use crate::error::Result;
use crate::project_env::{
    service as project_service, BoundProject, ProjectDetector, ProjectScanResult, SyncResult,
};
use crate::state::AppState;

/// Scan a project directory for runtime version requirements.
#[tauri::command]
pub async fn scan_project(path: String) -> Result<ProjectScanResult> {
    let path = std::path::PathBuf::from(&path);
    ProjectDetector::scan(&path).await
}

/// Add a project to bound projects (scan + persist).
#[tauri::command]
pub async fn add_bound_project(state: State<'_, AppState>, path: String) -> Result<BoundProject> {
    project_service::bind(&state.db, &path).await
}

/// List all bound projects.
#[tauri::command]
pub async fn list_bound_projects(state: State<'_, AppState>) -> Result<Vec<BoundProject>> {
    project_service::list(&state.db).await
}

/// Remove a bound project.
#[tauri::command]
pub async fn remove_bound_project(state: State<'_, AppState>, id: String) -> Result<()> {
    project_service::remove(&state.db, &id).await
}

/// Sync a project's runtime versions.
#[tauri::command]
pub async fn sync_project(state: State<'_, AppState>, id: String) -> Result<SyncResult> {
    let active_managers = state.config.lock().await.active_managers.clone();
    project_service::sync(&state.toolchain, &state.db, &id, &active_managers).await
}
