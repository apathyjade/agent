use tauri::State;
use serde_json::Value;

use crate::error::Result;
use crate::skills::market::MarketSkill;
use crate::skills::scanner::ReconcileResult;
use crate::skills::{SkillInfo, SkillDetail};
use crate::state::AppState;

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
pub async fn reconcile_skills(state: State<'_, AppState>) -> Result<ReconcileResult> {
    state.skills.lock().await.reconcile().await
}

#[tauri::command]
pub async fn list_market_top_skills(state: State<'_, AppState>, limit: Option<i64>) -> Result<Vec<MarketSkill>> {
    let _ = &*state.skills.lock().await;
    crate::skills::market::fetch_popular_skills(limit).await
}

#[tauri::command]
pub async fn search_market_skills(
    state: State<'_, AppState>,
    query: String,
    limit: Option<i64>,
) -> Result<Vec<MarketSkill>> {
    let _ = &*state.skills.lock().await;
    crate::skills::market::search_skills(&query, limit).await
}

#[tauri::command]
pub async fn install_market_skill(state: State<'_, AppState>, source: String) -> Result<String> {
    let _ = &*state.skills.lock().await;
    crate::skills::market::install_market_skill(&source).await
}
