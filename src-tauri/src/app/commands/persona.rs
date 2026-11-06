//! 人设 IPC 命令。
//!
//! 人设 = 角色提示词 + 关联记忆 + 可选模型覆盖。会话可以绑定人设，也可以
//! 由 [`resolve_persona`] 按消息内容自动匹配。

use tauri::State;

use crate::core::error::Result;
use crate::features::memory::MemoryInfo;
use crate::features::persona::{CreatePersonaParams, PersonaInfo, PersonaResolution, UpdatePersonaParams};
use crate::app::state::AppState;

/// 选择人设的两种传参形态：按 id，或按名称对象。
#[derive(serde::Deserialize)]
#[serde(untagged)]
pub enum PersonaTarget {
    Id(String),
    Name { name: String },
}

/// 新建人设，返回落库后的完整记录。
#[tauri::command]
pub async fn create_persona(
    state: State<'_, AppState>,
    params: CreatePersonaParams,
) -> Result<PersonaInfo> {
    state.persona.create(params).await
}

/// 列出全部人设（含内置）。
#[tauri::command]
pub async fn list_personas(state: State<'_, AppState>) -> Result<Vec<PersonaInfo>> {
    state.persona.list().await
}

/// 按 id 读取单个人设；不存在时返回 `NotFound`。
#[tauri::command]
pub async fn get_persona(
    state: State<'_, AppState>,
    id: String,
) -> Result<PersonaInfo> {
    state.persona.get(&id).await
}

/// 更新人设的指定字段；未提供的字段保持原值。
#[tauri::command]
pub async fn update_persona(
    state: State<'_, AppState>,
    id: String,
    params: UpdatePersonaParams,
) -> Result<PersonaInfo> {
    state.persona.update(&id, params).await
}

/// 删除人设；关联的记忆不会被删除，只解除关联。
#[tauri::command]
pub async fn delete_persona(state: State<'_, AppState>, id: String) -> Result<()> {
    state.persona.delete(&id).await
}

/// 为一条消息解析应当使用的人设。
///
/// `active_persona_id` 是会话当前绑定的人设（`None` 表示没有）。返回人设
/// 本体与选择方式：`manual`（会话绑定）、`auto`（按内容匹配）、`default`
/// （内置默认）。四种解析结果都没有时返回 `NotFound`。
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
        PersonaResolution::None => Err(crate::core::error::AppError::NotFound("No persona available".to_string())),
    }
}

/// [`resolve_persona`] 的返回体。
#[derive(serde::Serialize)]
pub struct ResolveResult {
    pub persona: PersonaInfo,
    /// 选择方式：`manual` / `auto` / `default`。
    pub mode: String,
}

// ── 记忆关联 ──

/// 把一条记忆关联到人设（重复关联是幂等的）。
#[tauri::command]
pub async fn link_memory_to_persona(
    state: State<'_, AppState>,
    persona_id: String,
    memory_id: String,
) -> Result<()> {
    state.persona.link_memory(&persona_id, &memory_id).await
}

/// 解除记忆与人设的关联；记忆本身保留。
#[tauri::command]
pub async fn unlink_memory_from_persona(
    state: State<'_, AppState>,
    persona_id: String,
    memory_id: String,
) -> Result<()> {
    state.persona.unlink_memory(&persona_id, &memory_id).await
}

/// 列出人设关联的记忆，按相关度降序。
///
/// 关联 id 指向已被删除的记忆时跳过该项，不报错。
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
    // 按相关度降序排列。
    memories.sort_by(|a, b| b.relevance.partial_cmp(&a.relevance).unwrap_or(std::cmp::Ordering::Equal));
    Ok(memories)
}

// ── 项目绑定 ──

/// 把人设绑定到项目目录。
///
/// `auto_select`：`Some(true)` 表示在该项目里自动选中此人设，`None` 等价于
/// `false`（仅登记绑定关系）。
#[tauri::command]
pub async fn bind_persona_project(
    state: State<'_, AppState>,
    persona_id: String,
    project_path: String,
    auto_select: Option<bool>,
) -> Result<()> {
    state.persona.bind_project(&persona_id, &project_path, auto_select.unwrap_or(false)).await
}

/// 解除人设与项目目录的绑定。
#[tauri::command]
pub async fn unbind_persona_project(
    state: State<'_, AppState>,
    persona_id: String,
    project_path: String,
) -> Result<()> {
    state.persona.unbind_project(&persona_id, &project_path).await
}
