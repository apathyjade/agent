//! 通用键值设置 IPC 命令。
//!
//! 这是 SQLite `settings` 表的薄封装，用于零散的 UI 偏好与调试快照
//! （如 `request_ctx:<session_id>`）。结构化配置请走 `AppConfig`。

use std::collections::HashMap;
use tauri::State;

use crate::core::error::Result;
use crate::app::state::AppState;

/// 批量写入键值设置；同一事务内逐条 upsert。
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

/// 读取全部设置，返回 key → value 映射。
#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<HashMap<String, String>> {
    let db = state.db.lock().await;
    let rows = db.get_all_settings()?;
    let map: HashMap<String, String> = rows.into_iter().map(|s| (s.key, s.value)).collect();
    Ok(map)
}
