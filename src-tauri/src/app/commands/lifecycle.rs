//! 会话生命周期 IPC 命令。
//!
//! 覆盖摘要查询 / 强制生成，以及生命周期配置的读写。自动摘要与自动归档
//! 的触发逻辑在 `features::session::lifecycle`，这里只做入口。

use tauri::State;
use crate::core::error::Result;
use crate::app::state::AppState;
use crate::core::db::models::SessionSummary;
use crate::features::session::lifecycle::config::LifecycleConfig;

/// 列出某会话已生成的全部摘要，按时间倒序。
#[tauri::command]
pub async fn get_session_summaries(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<SessionSummary>> {
    let db = state.db.lock().await;
    db.get_session_summaries(&session_id)
}

/// 忽略阈值，强制为会话生成一次摘要。
///
/// 仍然受「消息太少不值得总结」等内部条件约束；会话不存在时返回
/// `NotFound`。
#[tauri::command]
pub async fn force_generate_summary(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<()> {
    let db = state.db.lock().await;
    let session = db.get_session(&session_id)?
        .ok_or_else(|| crate::core::error::AppError::NotFound("Session not found".to_string()))?;
    drop(db);

    crate::features::session::lifecycle::summarizer::maybe_generate_summary(
        &state.lifecycle,
        &session_id,
        &session.model_id,
    ).await
}

/// 读取当前生命周期配置（摘要间隔、归档阈值等）。
#[tauri::command]
pub async fn get_lifecycle_config(
    state: State<'_, AppState>,
) -> Result<LifecycleConfig> {
    Ok(state.lifecycle.config.lock().await.clone())
}

/// 覆盖生命周期配置并落盘。
///
/// 立即写入内存状态；正在进行的归档检查使用新值，不回溯已归档会话。
#[tauri::command]
pub async fn update_lifecycle_config(
    state: State<'_, AppState>,
    config: LifecycleConfig,
) -> Result<()> {
    *state.lifecycle.config.lock().await = config;
    state.lifecycle.save_config().await
}
