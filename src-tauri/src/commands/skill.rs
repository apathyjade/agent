use tauri::State;
use serde_json::Value;

use crate::error::Result;
use crate::state::AppState;
use crate::skills::{DiscoveredSkill, SkillInfo, SkillDetail};

#[tauri::command]
pub async fn list_skills(state: State<'_, AppState>) -> Result<Vec<SkillInfo>> {
    state.skills.lock().await.list().await
}

#[tauri::command]
pub async fn get_skill_detail(state: State<'_, AppState>, id: String) -> Result<SkillDetail> {
    state.skills.lock().await.get_detail(&id).await
}

#[tauri::command]
pub async fn install_skill_from_path(state: State<'_, AppState>, path: String) -> Result<SkillInfo> {
    state.skills.lock().await.install_from_path(&path).await
}

#[tauri::command]
pub async fn uninstall_skill(state: State<'_, AppState>, id: String) -> Result<()> {
    state.skills.lock().await.uninstall(&id).await
}

#[tauri::command]
pub async fn toggle_skill(state: State<'_, AppState>, id: String, enabled: bool) -> Result<()> {
    state.skills.lock().await.toggle(&id, enabled).await
}

#[tauri::command]
pub async fn configure_skill(state: State<'_, AppState>, id: String, config: Value) -> Result<()> {
    state.skills.lock().await.configure(&id, config).await
}

#[tauri::command]
pub async fn scan_local_skills(state: State<'_, AppState>) -> Result<Vec<DiscoveredSkill>> {
    state.skills.lock().await.scan_local().await
}

#[tauri::command]
pub async fn import_scanned_skill(
    state: State<'_, AppState>,
    discovered_id: String,
    discovered_path: String,
    agent_sources: Vec<String>,
) -> Result<SkillInfo> {
    state.skills.lock().await.import_scanned(&discovered_id, &discovered_path, agent_sources).await
}
