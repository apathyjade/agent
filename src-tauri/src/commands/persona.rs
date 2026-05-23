use tauri::State;

use crate::error::Result;
use crate::memory::MemoryInfo;
use crate::persona::{CreatePersonaParams, PersonaInfo, PersonaResolution, UpdatePersonaParams};
use crate::state::AppState;

/// Which persona to use (for IPC round-trip: manual name or "auto").
#[derive(serde::Deserialize)]
#[serde(untagged)]
pub enum PersonaTarget {
    Id(String),
    Name { name: String },
}

#[tauri::command]
pub async fn create_persona(
    state: State<'_, AppState>,
    params: CreatePersonaParams,
) -> Result<PersonaInfo> {
    state.persona.create(params).await
}

#[tauri::command]
pub async fn list_personas(state: State<'_, AppState>) -> Result<Vec<PersonaInfo>> {
    state.persona.list().await
}

#[tauri::command]
pub async fn get_persona(
    state: State<'_, AppState>,
    id: String,
) -> Result<PersonaInfo> {
    state.persona.get(&id).await
}

#[tauri::command]
pub async fn update_persona(
    state: State<'_, AppState>,
    id: String,
    params: UpdatePersonaParams,
) -> Result<PersonaInfo> {
    state.persona.update(&id, params).await
}

#[tauri::command]
pub async fn delete_persona(state: State<'_, AppState>, id: String) -> Result<()> {
    state.persona.delete(&id).await
}

/// Resolve which persona to use for the given message + project context.
/// The `active_persona_id` is the current session's active persona (if any).
/// Returns the resolved persona info and a label describing how it was chosen.
#[tauri::command]
pub async fn resolve_persona(
    state: State<'_, AppState>,
    message: String,
    project_path: Option<String>,
    active_persona_id: Option<String>,
) -> Result<ResolveResult> {
    let path = project_path.as_deref();
    let active = active_persona_id.as_deref();
    match state.persona.resolve(&message, path, active).await {
        PersonaResolution::Manual(p) => Ok(ResolveResult { persona: p, mode: "manual".to_string() }),
        PersonaResolution::Auto(p) => Ok(ResolveResult { persona: p, mode: "auto".to_string() }),
        PersonaResolution::Default(p) => Ok(ResolveResult { persona: p, mode: "default".to_string() }),
        PersonaResolution::None => Err(crate::error::AppError::NotFound("No persona available".to_string())),
    }
}

#[derive(serde::Serialize)]
pub struct ResolveResult {
    pub persona: PersonaInfo,
    pub mode: String,
}

// ── Memory linking ──

#[tauri::command]
pub async fn link_memory_to_persona(
    state: State<'_, AppState>,
    persona_id: String,
    memory_id: String,
) -> Result<()> {
    state.persona.link_memory(&persona_id, &memory_id).await
}

#[tauri::command]
pub async fn unlink_memory_from_persona(
    state: State<'_, AppState>,
    persona_id: String,
    memory_id: String,
) -> Result<()> {
    state.persona.unlink_memory(&persona_id, &memory_id).await
}

#[tauri::command]
pub async fn get_persona_memories(
    state: State<'_, AppState>,
    persona_id: String,
) -> Result<Vec<MemoryInfo>> {
    let ids = state.persona.get_linked_memory_ids(&persona_id).await?;
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let db = state.db.lock().await;
    let mut memories = Vec::new();
    for mid in &ids {
        if let Ok(Some(rec)) = db.get_memory(mid) {
            memories.push(MemoryInfo::from(rec));
        }
    }
    // Sort by relevance desc
    memories.sort_by(|a, b| b.relevance.partial_cmp(&a.relevance).unwrap_or(std::cmp::Ordering::Equal));
    Ok(memories)
}

// ── Project binding ──

#[tauri::command]
pub async fn bind_persona_project(
    state: State<'_, AppState>,
    persona_id: String,
    project_path: String,
    auto_select: Option<bool>,
) -> Result<()> {
    state.persona.bind_project(&persona_id, &project_path, auto_select.unwrap_or(false)).await
}

#[tauri::command]
pub async fn unbind_persona_project(
    state: State<'_, AppState>,
    persona_id: String,
    project_path: String,
) -> Result<()> {
    state.persona.unbind_project(&persona_id, &project_path).await
}
