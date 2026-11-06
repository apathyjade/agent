//! 记忆 IPC 命令。
//!
//! 记忆是跨会话的长期事实库；`create/update` 接收结构化的参数对象，
//! 便于前端一次提交多个字段。检索能力在 [`search_memories`]。

use tauri::State;

use crate::core::error::Result;
use crate::features::memory::{CreateMemoryParams, MemoryInfo, UpdateMemoryParams};
use crate::app::state::AppState;

/// 新建一条记忆，返回落库后的完整记录（含生成的 id 与时间戳）。
#[tauri::command]
pub async fn create_memory(
    state: State<'_, AppState>,
    params: CreateMemoryParams,
) -> Result<MemoryInfo> {
    state.memory.create(params).await
}

/// 列出全部记忆（不分页），供管理界面展示。
#[tauri::command]
pub async fn list_memories(state: State<'_, AppState>) -> Result<Vec<MemoryInfo>> {
    state.memory.list().await
}

/// 按 id 读取单条记忆；不存在时返回 `NotFound`。
#[tauri::command]
pub async fn get_memory(state: State<'_, AppState>, id: String) -> Result<MemoryInfo> {
    state.memory.get(&id).await
}

/// 检索记忆。
///
/// `memory_type` 与 `scope` 都是可选过滤器：`None` 表示不按该维度过滤。
/// 有关键词时走混合检索，无关键词时按重要度/时间返回。
#[tauri::command]
pub async fn search_memories(
    state: State<'_, AppState>,
    query: String,
    memory_type: Option<String>,
    scope: Option<String>,
) -> Result<Vec<MemoryInfo>> {
    state.memory.search(&query, memory_type.as_deref(), scope.as_deref()).await
}

/// 更新记忆的指定字段；未提供的字段保持原值。
#[tauri::command]
pub async fn update_memory(
    state: State<'_, AppState>,
    id: String,
    params: UpdateMemoryParams,
) -> Result<MemoryInfo> {
    state.memory.update(&id, params).await
}

/// 删除一条记忆；同时清理它与会话、人设的关联。
#[tauri::command]
pub async fn delete_memory(state: State<'_, AppState>, id: String) -> Result<()> {
    state.memory.delete(&id).await
}
