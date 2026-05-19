use chrono::Utc;
use tauri::State;
use uuid::Uuid;

use crate::db::models::SystemPrompt;
use crate::error::Result;
use crate::state::AppState;

#[tauri::command]
pub async fn create_system_prompt(
    state: State<'_, AppState>,
    name: String,
    content: String,
) -> Result<SystemPrompt> {
    let prompt = SystemPrompt {
        id: Uuid::new_v4().to_string(),
        name,
        content,
        is_default: false,
        created_at: Utc::now().to_rfc3339(),
    };

    let db = state.db.lock().await;
    db.create_system_prompt(&prompt)?;
    Ok(prompt)
}

#[tauri::command]
pub async fn list_system_prompts(state: State<'_, AppState>) -> Result<Vec<SystemPrompt>> {
    let db = state.db.lock().await;
    db.list_system_prompts()
}

#[tauri::command]
pub async fn delete_system_prompt(state: State<'_, AppState>, id: String) -> Result<()> {
    let db = state.db.lock().await;
    db.delete_system_prompt(&id)
}

#[tauri::command]
pub async fn set_default_system_prompt(state: State<'_, AppState>, id: String) -> Result<()> {
    let db = state.db.lock().await;
    db.set_default_system_prompt(&id)
}

#[tauri::command]
pub async fn get_default_system_prompt(state: State<'_, AppState>) -> Result<Option<SystemPrompt>> {
    let db = state.db.lock().await;
    db.get_default_system_prompt()
}
