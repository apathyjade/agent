//! 项目（工作区）IPC 命令。
//!
//! 项目只是一条「名字 + 路径」记录，用于把会话分组、给 planner 提供工作
//! 目录。运行时绑定属于另一套机制，见 `project_env.rs`。

use chrono::Utc;
use tauri::State;
use uuid::Uuid;

use crate::core::db::models::{Project, Session as DbSession};
use crate::core::error::Result;
use crate::app::state::AppState;

/// 列出全部项目。
#[tauri::command]
pub async fn list_projects(state: State<'_, AppState>) -> Result<Vec<Project>> {
    let db = state.db.lock().await;
    db.list_projects()
}

/// 新建项目。`path` 不做存在性校验，允许先登记后创建目录。
#[tauri::command]
pub async fn create_project(
    state: State<'_, AppState>,
    name: String,
    path: String,
) -> Result<Project> {
    let now = Utc::now().to_rfc3339();
    let project = Project {
        id: Uuid::new_v4().to_string(),
        name,
        path,
        created_at: now.clone(),
        updated_at: now,
    };

    let db = state.db.lock().await;
    db.create_project(&project)?;

    Ok(project)
}

/// 删除项目。
///
/// 属于该项目的会话不会被删除，只会把 `project_id` 置空（解除关联）。
#[tauri::command]
pub async fn delete_project(state: State<'_, AppState>, id: String) -> Result<()> {
    let db = state.db.lock().await;
    // 先解除会话关联：把该项目的会话 `project_id` 置为 NULL。
    db.set_sessions_project_null(&id)?;
    db.delete_project(&id)
}

/// 列出某项目下的全部会话（含已归档）。
#[tauri::command]
pub async fn get_project_sessions(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<DbSession>> {
    let db = state.db.lock().await;
    db.get_sessions_by_project(&project_id)
}
