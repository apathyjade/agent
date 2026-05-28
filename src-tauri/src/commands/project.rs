use chrono::Utc;
use tauri::State;
use uuid::Uuid;

use crate::db::models::{Project, Session as DbSession};
use crate::error::Result;
use crate::state::AppState;

#[tauri::command]
pub async fn list_projects(state: State<'_, AppState>) -> Result<Vec<Project>> {
    let db = state.db.lock().await;
    db.list_projects()
}

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

#[tauri::command]
pub async fn delete_project(state: State<'_, AppState>, id: String) -> Result<()> {
    let db = state.db.lock().await;
    // Unlink sessions: set project_id to NULL for sessions belonging to this project
    db.set_sessions_project_null(&id)?;
    db.delete_project(&id)
}

#[tauri::command]
pub async fn get_project_sessions(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<DbSession>> {
    let db = state.db.lock().await;
    db.get_sessions_by_project(&project_id)
}
