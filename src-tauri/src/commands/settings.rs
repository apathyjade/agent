use std::collections::HashMap;
use tauri::State;

use crate::error::Result;
use crate::state::AppState;

#[tauri::command]
pub async fn update_settings(
    state: State<'_, AppState>,
    settings: HashMap<String, String>,
) -> Result<()> {
    let db = state.db.lock().await;
    for (key, value) in settings {
        db.set_setting(&key, &value)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<HashMap<String, String>> {
    let db = state.db.lock().await;
    let rows = db.get_all_settings()?;
    let map: HashMap<String, String> = rows.into_iter().map(|s| (s.key, s.value)).collect();
    Ok(map)
}
