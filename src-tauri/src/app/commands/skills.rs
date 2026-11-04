use tauri::State;

use crate::core::error::Result;
use crate::features::skills::agents::{self, AgentOption};
use crate::features::skills::cli::CliStatus;
use crate::features::skills::legacy::{LegacyImportResult, LegacySkill};
use crate::features::skills::list::InstalledSkill;
use crate::features::skills::market::MarketSkill;
use crate::features::skills::{InstallRequest, RepoSkillPreview, UpdateCheck};
use crate::app::state::AppState;

/// Scope used when the caller does not specify one.
const DEFAULT_SCOPE: &str = "global";

#[tauri::command]
pub async fn get_skills_env(state: State<'_, AppState>) -> Result<CliStatus> {
    Ok(state.skills.env_status().await)
}

#[tauri::command]
pub fn list_skill_agents() -> Vec<AgentOption> {
    agents::all().to_vec()
}

#[tauri::command]
pub async fn list_installed_skills(
    state: State<'_, AppState>,
    scope: Option<String>,
    project_path: Option<String>,
) -> Result<Vec<InstalledSkill>> {
    let scope = scope.unwrap_or_else(|| DEFAULT_SCOPE.to_string());
    state
        .skills
        .list_installed(&scope, project_path.as_deref())
        .await
}

#[tauri::command]
pub async fn get_skill_content(
    state: State<'_, AppState>,
    name: String,
    scope: String,
    project_path: Option<String>,
) -> Result<String> {
    state
        .skills
        .content(&name, &scope, project_path.as_deref())
        .await
}

#[tauri::command]
pub async fn preview_skill_source(
    state: State<'_, AppState>,
    source: String,
) -> Result<RepoSkillPreview> {
    state.skills.preview_source(&source).await
}

#[tauri::command]
pub async fn detect_legacy_skills(state: State<'_, AppState>) -> Result<Vec<LegacySkill>> {
    state.skills.detect_legacy().await
}

#[tauri::command]
pub async fn import_legacy_skills(
    state: State<'_, AppState>,
    paths: Vec<String>,
    agents: Vec<String>,
    copy: bool,
) -> Result<LegacyImportResult> {
    state.skills.import_legacy(&paths, &agents, copy).await
}

#[tauri::command]
pub async fn install_skills(
    state: State<'_, AppState>,
    source: String,
    skills: Vec<String>,
    agents: Vec<String>,
    scope: String,
    project_path: Option<String>,
    copy: bool,
) -> Result<String> {
    state
        .skills
        .install(InstallRequest {
            source,
            skills,
            agents,
            scope,
            project_path,
            copy,
        })
        .await
}

#[tauri::command]
pub async fn remove_skill(
    state: State<'_, AppState>,
    name: String,
    scope: String,
    project_path: Option<String>,
) -> Result<String> {
    state
        .skills
        .remove(&name, &scope, project_path.as_deref())
        .await
}

#[tauri::command]
pub async fn check_skill_updates(
    state: State<'_, AppState>,
    scope: String,
    project_path: Option<String>,
) -> Result<UpdateCheck> {
    state
        .skills
        .check_updates(&scope, project_path.as_deref())
        .await
}

#[tauri::command]
pub async fn update_skills(
    state: State<'_, AppState>,
    scope: String,
    project_path: Option<String>,
) -> Result<String> {
    state.skills.update(&scope, project_path.as_deref()).await
}

#[tauri::command]
pub async fn list_market_top_skills(limit: Option<i64>) -> Result<Vec<MarketSkill>> {
    crate::features::skills::market::fetch_popular_skills(limit).await
}

#[tauri::command]
pub async fn search_market_skills(
    query: String,
    limit: Option<i64>,
) -> Result<Vec<MarketSkill>> {
    crate::features::skills::market::search_skills(&query, limit).await
}
