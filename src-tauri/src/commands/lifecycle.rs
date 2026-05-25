use tauri::State;
use crate::error::Result;
use crate::state::AppState;
use crate::db::models::SessionSummary;
use crate::lifecycle::config::LifecycleConfig;

#[tauri::command]
pub async fn get_session_summaries(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<SessionSummary>> {
    let db = state.db.lock().await;
    db.get_session_summaries(&session_id)
}

#[tauri::command]
pub async fn force_generate_summary(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<()> {
    let db = state.db.lock().await;
    let session = db.get_session(&session_id)?
        .ok_or_else(|| crate::error::AppError::NotFound("Session not found".to_string()))?;
    drop(db);

    crate::lifecycle::summarizer::maybe_generate_summary(
        &state.lifecycle,
        &session_id,
        &session.model_id,
    ).await
}

#[tauri::command]
pub async fn get_lifecycle_config(
    state: State<'_, AppState>,
) -> Result<LifecycleConfig> {
    Ok(state.lifecycle.config.lock().await.clone())
}

#[tauri::command]
pub async fn update_lifecycle_config(
    state: State<'_, AppState>,
    config: LifecycleConfig,
) -> Result<()> {
    *state.lifecycle.config.lock().await = config;
    state.lifecycle.save_config().await
}
