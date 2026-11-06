//! Skills 管理 IPC 命令。
//!
//! Skills 是从 Git 仓库或本地目录安装的能力包，按 `global` / `project`
//! 两种 scope 存放。安装目标 agent 由 `agents` 参数决定（claude、codex 等）。

use tauri::State;

use crate::core::error::Result;
use crate::features::skills::agents::{self, AgentOption};
use crate::features::skills::cli::CliStatus;
use crate::features::skills::legacy::{LegacyImportResult, LegacySkill};
use crate::features::skills::list::InstalledSkill;
use crate::features::skills::market::MarketSkill;
use crate::features::skills::{InstallRequest, RepoSkillPreview, UpdateCheck};
use crate::app::state::AppState;

/// 调用方未指定 scope 时使用的默认值。
const DEFAULT_SCOPE: &str = "global";

/// 检测本机 skills 运行环境（CLI 是否可用、版本等）。
#[tauri::command]
pub async fn get_skills_env(state: State<'_, AppState>) -> Result<CliStatus> {
    Ok(state.skills.env_status().await)
}

/// 列出可安装 skills 的目标 agent 选项（静态表，无 IO）。
#[tauri::command]
pub fn list_skill_agents() -> Vec<AgentOption> {
    agents::all().to_vec()
}

/// 列出已安装的 skills。
///
/// `scope` 缺省为 `global`；`project_path` 只在 scope 为 `project` 时有意义，
/// 其他情况传 `None` 即可。
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

/// 读取某个 skill 的 SKILL.md 全文。
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

/// 预览一个来源（Git URL / 本地路径）里可安装的 skills，不落盘。
#[tauri::command]
pub async fn preview_skill_source(
    state: State<'_, AppState>,
    source: String,
) -> Result<RepoSkillPreview> {
    state.skills.preview_source(&source).await
}

/// 扫描旧版 skills 目录，列出可迁移项。
#[tauri::command]
pub async fn detect_legacy_skills(state: State<'_, AppState>) -> Result<Vec<LegacySkill>> {
    state.skills.detect_legacy().await
}

/// 迁移旧版 skills 到新目录。
///
/// `paths` 是要迁移的源路径；`agents` 是目标 agent；`copy` 为 true 时保留
/// 原文件（复制），false 时移动。
#[tauri::command]
pub async fn import_legacy_skills(
    state: State<'_, AppState>,
    paths: Vec<String>,
    agents: Vec<String>,
    copy: bool,
) -> Result<LegacyImportResult> {
    state.skills.import_legacy(&paths, &agents, copy).await
}

/// 从来源安装指定 skills 到指定 agent 与 scope，返回可展示的结果摘要。
///
/// `skills` 为空表示安装来源里的全部 skills；`copy` 语义同
/// [`import_legacy_skills`]。
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

/// 卸载指定 skill，返回可展示的结果摘要。
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

/// 检查已安装 skills 是否有上游更新（可能走网络）。
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

/// 拉取并应用所有可用更新，返回结果摘要。
#[tauri::command]
pub async fn update_skills(
    state: State<'_, AppState>,
    scope: String,
    project_path: Option<String>,
) -> Result<String> {
    state.skills.update(&scope, project_path.as_deref()).await
}

/// 获取市场热门 skills；`limit` 为 `None` 时由市场端决定数量。
#[tauri::command]
pub async fn list_market_top_skills(limit: Option<i64>) -> Result<Vec<MarketSkill>> {
    crate::features::skills::market::fetch_popular_skills(limit).await
}

/// 在市场里按关键词搜索 skills；`limit` 语义同
/// [`list_market_top_skills`]。
#[tauri::command]
pub async fn search_market_skills(
    query: String,
    limit: Option<i64>,
) -> Result<Vec<MarketSkill>> {
    crate::features::skills::market::search_skills(&query, limit).await
}
