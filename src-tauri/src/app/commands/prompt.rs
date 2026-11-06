//! 系统提示词库 IPC 命令。
//!
//! 与「会话自己的系统提示词」不同：这里是可复用的提示词模板，其中最多
//! 一条被标记为默认值，新建会话时使用。

use chrono::Utc;
use tauri::State;
use uuid::Uuid;

use crate::core::db::models::SystemPrompt;
use crate::core::error::Result;
use crate::app::state::AppState;

/// 新建提示词模板。新模板**不会**自动成为默认值。
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

/// 列出全部提示词模板。
#[tauri::command]
pub async fn list_system_prompts(state: State<'_, AppState>) -> Result<Vec<SystemPrompt>> {
    let db = state.db.lock().await;
    db.list_system_prompts()
}

/// 删除提示词模板；删除默认模板后「默认」为空，而不是自动转移给别的模板。
#[tauri::command]
pub async fn delete_system_prompt(state: State<'_, AppState>, id: String) -> Result<()> {
    let db = state.db.lock().await;
    db.delete_system_prompt(&id)
}

/// 把指定模板设为唯一默认值（其余模板的 `is_default` 会被清空）。
#[tauri::command]
pub async fn set_default_system_prompt(state: State<'_, AppState>, id: String) -> Result<()> {
    let db = state.db.lock().await;
    db.set_default_system_prompt(&id)
}

/// 读取默认提示词模板；未设置时返回 `Ok(None)`。
#[tauri::command]
pub async fn get_default_system_prompt(state: State<'_, AppState>) -> Result<Option<SystemPrompt>> {
    let db = state.db.lock().await;
    db.get_default_system_prompt()
}
