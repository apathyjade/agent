use tauri::State;

use crate::error::Result;
use crate::memory::{CreateMemoryParams, MemoryInfo, UpdateMemoryParams};
use crate::state::AppState;

#[tauri::command]
pub async fn create_memory(
    state: State<'_, AppState>,
    params: CreateMemoryParams,
) -> Result<MemoryInfo> {
    state.memory.create(params).await
}

#[tauri::command]
pub async fn list_memories(state: State<'_, AppState>) -> Result<Vec<MemoryInfo>> {
    state.memory.list().await
}

#[tauri::command]
pub async fn get_memory(state: State<'_, AppState>, id: String) -> Result<MemoryInfo> {
    state.memory.get(&id).await
}

#[tauri::command]
pub async fn search_memories(
    state: State<'_, AppState>,
    query: String,
    memory_type: Option<String>,
    scope: Option<String>,
) -> Result<Vec<MemoryInfo>> {
    state.memory.search(&query, memory_type.as_deref(), scope.as_deref()).await
}

#[tauri::command]
pub async fn update_memory(
    state: State<'_, AppState>,
    id: String,
    params: UpdateMemoryParams,
) -> Result<MemoryInfo> {
    state.memory.update(&id, params).await
}

#[tauri::command]
pub async fn delete_memory(state: State<'_, AppState>, id: String) -> Result<()> {
    state.memory.delete(&id).await
}
