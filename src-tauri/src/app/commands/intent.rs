//! Intent management IPC commands.
//!
//! See the design note §7 for the contract. Every write command stamps
//! `updated_at` itself: the repository never reads the clock.
//!
//! The frontend never sends a whole database record for a write. `IntentInput`
//! carries only the editable fields; `id`, `domain_id`, `version` and the
//! timestamps stay owned by the backend.

use tauri::State;

use crate::app::state::AppState;
use crate::core::db::models::{IntentDomainRecord, IntentRecord};
use crate::core::error::{AppError, Result};
use crate::features::intent_management::json::{parse_intent_array, to_json_array, to_records};

// ── DTOs ──

/// Wire shape of a domain. Mirrors [`IntentDomainRecord`] field for field.
#[derive(serde::Serialize)]
pub struct IntentDomainInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub is_active: bool,
    pub source: String,
    pub version: i64,
    pub created_at: String,
    pub updated_at: String,
}

impl From<IntentDomainRecord> for IntentDomainInfo {
    fn from(record: IntentDomainRecord) -> Self {
        Self {
            id: record.id,
            name: record.name,
            description: record.description,
            is_active: record.is_active,
            source: record.source,
            version: record.version,
            created_at: record.created_at,
            updated_at: record.updated_at,
        }
    }
}

/// Wire shape of an intent. Mirrors [`IntentRecord`] field for field;
/// `examples` and `allowed_tools` are real arrays so the frontend never has to
/// parse a JSON string.
#[derive(serde::Serialize)]
pub struct IntentInfo {
    pub id: String,
    pub domain_id: String,
    pub name: String,
    pub description: String,
    pub examples: Vec<String>,
    pub workflow: String,
    pub allowed_tools: Vec<String>,
    pub default_agent: String,
    pub scope: String,
    pub enabled: bool,
    pub version: i64,
    pub confidence_threshold: f64,
    pub fallback_intent: String,
    pub path: String,
    pub auto_escalate: bool,
    pub system_prompt_appendix: String,
    pub model_id: String,
    pub max_iterations: Option<usize>,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

impl From<IntentRecord> for IntentInfo {
    fn from(record: IntentRecord) -> Self {
        Self {
            id: record.id,
            domain_id: record.domain_id,
            name: record.name,
            description: record.description,
            examples: record.examples,
            workflow: record.workflow,
            allowed_tools: record.allowed_tools,
            default_agent: record.default_agent,
            scope: record.scope,
            enabled: record.enabled,
            version: record.version,
            confidence_threshold: record.confidence_threshold,
            fallback_intent: record.fallback_intent,
            path: record.path,
            auto_escalate: record.auto_escalate,
            system_prompt_appendix: record.system_prompt_appendix,
            model_id: record.model_id,
            max_iterations: record.max_iterations,
            sort_order: record.sort_order,
            created_at: record.created_at,
            updated_at: record.updated_at,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_threshold() -> f64 {
    0.7
}

fn default_path() -> String {
    "fast".to_string()
}

fn default_scope() -> String {
    "global".to_string()
}

/// Editable intent fields accepted from the frontend.
///
/// Defaults mirror the schema, so a minimal payload creates a usable intent.
#[derive(serde::Deserialize)]
pub struct IntentInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub examples: Vec<String>,
    #[serde(default)]
    pub workflow: String,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub default_agent: String,
    #[serde(default = "default_scope")]
    pub scope: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_threshold")]
    pub confidence_threshold: f64,
    #[serde(default)]
    pub fallback_intent: String,
    #[serde(default = "default_path")]
    pub path: String,
    #[serde(default)]
    pub auto_escalate: bool,
    #[serde(default)]
    pub system_prompt_appendix: String,
    #[serde(default)]
    pub model_id: String,
    #[serde(default)]
    pub max_iterations: Option<usize>,
    #[serde(default)]
    pub sort_order: i64,
}

impl IntentInput {
    /// Validates the fields the router relies on. Returns a message naming the
    /// offending field instead of letting a bad row reach SQLite.
    fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(AppError::InvalidInput("意图名称不能为空".to_string()));
        }

        let path = self.path.trim();
        if !matches!(path, "fast" | "deep") {
            return Err(AppError::InvalidInput(format!(
                "path 必须是 fast 或 deep，实际为「{path}」"
            )));
        }

        if !self.confidence_threshold.is_finite()
            || !(0.0..=1.0).contains(&self.confidence_threshold)
        {
            return Err(AppError::InvalidInput(format!(
                "confidence_threshold 必须在 [0.0, 1.0] 之间，实际为 {}",
                self.confidence_threshold
            )));
        }

        Ok(())
    }

    /// Copies the editable fields onto an existing record, leaving identity,
    /// `version` and `created_at` untouched.
    fn apply_to(&self, record: &mut IntentRecord) {
        record.name = self.name.trim().to_string();
        record.description = self.description.clone();
        record.examples = self.examples.clone();
        record.workflow = self.workflow.clone();
        record.allowed_tools = self.allowed_tools.clone();
        record.default_agent = self.default_agent.clone();
        record.scope = self.scope.clone();
        record.enabled = self.enabled;
        record.confidence_threshold = self.confidence_threshold;
        record.fallback_intent = self.fallback_intent.clone();
        record.path = self.path.trim().to_string();
        record.auto_escalate = self.auto_escalate;
        record.system_prompt_appendix = self.system_prompt_appendix.clone();
        record.model_id = self.model_id.clone();
        record.max_iterations = self.max_iterations;
        record.sort_order = self.sort_order;
    }
}

// ── Helpers ──

/// Returns `wanted` when it is not already taken, otherwise appends the first
/// free ` (n)` suffix, n starting at 2. Past 20 suffixes the caller still gets
/// a distinct string (`" (uuid)"`), which the UNIQUE constraint may reject —
/// at that point a human has 21 domains with the same name.
pub(crate) fn unique_domain_name(existing: &[String], wanted: &str) -> String {
    if !existing.iter().any(|name| name == wanted) {
        return wanted.to_string();
    }

    for suffix in 2..=21 {
        let candidate = format!("{wanted} ({suffix})");
        if !existing.iter().any(|name| name == &candidate) {
            return candidate;
        }
    }

    format!("{wanted} (uuid)")
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn not_found_domain(id: &str) -> AppError {
    AppError::NotFound(format!("领域不存在: {id}"))
}

fn not_found_intent(id: &str) -> AppError {
    AppError::NotFound(format!("意图不存在: {id}"))
}

// ── Domains ──

#[tauri::command]
pub async fn list_intent_domains(state: State<'_, AppState>) -> Result<Vec<IntentDomainInfo>> {
    let db = state.db.lock().await;
    Ok(db
        .list_intent_domains()?
        .into_iter()
        .map(IntentDomainInfo::from)
        .collect())
}

#[tauri::command]
pub async fn get_active_intent_domain(
    state: State<'_, AppState>,
) -> Result<Option<IntentDomainInfo>> {
    let db = state.db.lock().await;
    Ok(db.get_active_intent_domain()?.map(IntentDomainInfo::from))
}

#[tauri::command]
pub async fn create_intent_domain(
    state: State<'_, AppState>,
    name: String,
    description: String,
) -> Result<IntentDomainInfo> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::InvalidInput("领域名称不能为空".to_string()));
    }

    let timestamp = now();
    let db = state.db.lock().await;
    if db.list_intent_domains()?.iter().any(|d| d.name == name) {
        return Err(AppError::InvalidInput(format!("领域名称已存在：{name}")));
    }

    let domain = IntentDomainRecord {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        description,
        is_active: false,
        source: "user".to_string(),
        version: 1,
        created_at: timestamp.clone(),
        updated_at: timestamp,
    };
    db.insert_intent_domain(&domain)?;
    Ok(domain.into())
}

#[tauri::command]
pub async fn update_intent_domain(
    state: State<'_, AppState>,
    id: String,
    name: String,
    description: String,
) -> Result<IntentDomainInfo> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::InvalidInput("领域名称不能为空".to_string()));
    }

    let db = state.db.lock().await;
    let mut domain = db.get_intent_domain(&id)?.ok_or_else(|| not_found_domain(&id))?;

    if db
        .list_intent_domains()?
        .iter()
        .any(|d| d.id != id && d.name == name)
    {
        return Err(AppError::InvalidInput(format!("领域名称已存在：{name}")));
    }

    domain.name = name;
    domain.description = description;
    domain.updated_at = now();
    db.update_intent_domain(&domain)?;
    Ok(domain.into())
}

#[tauri::command]
pub async fn delete_intent_domain(state: State<'_, AppState>, id: String) -> Result<()> {
    let db = state.db.lock().await;
    db.delete_intent_domain(&id)
}

#[tauri::command]
pub async fn set_active_intent_domain(state: State<'_, AppState>, id: String) -> Result<()> {
    let db = state.db.lock().await;
    db.set_active_intent_domain(&id)
}

// ── Intents ──

#[tauri::command]
pub async fn list_intents(
    state: State<'_, AppState>,
    domain_id: String,
) -> Result<Vec<IntentInfo>> {
    let db = state.db.lock().await;
    Ok(db
        .list_intents(&domain_id)?
        .into_iter()
        .map(IntentInfo::from)
        .collect())
}

#[tauri::command]
pub async fn get_intent(state: State<'_, AppState>, id: String) -> Result<IntentInfo> {
    let db = state.db.lock().await;
    let intent = db.get_intent(&id)?.ok_or_else(|| not_found_intent(&id))?;
    Ok(intent.into())
}

#[tauri::command]
pub async fn create_intent(
    state: State<'_, AppState>,
    domain_id: String,
    intent: IntentInput,
) -> Result<IntentInfo> {
    intent.validate()?;
    let name = intent.name.trim();

    let timestamp = now();
    let db = state.db.lock().await;
    if db.get_intent_domain(&domain_id)?.is_none() {
        return Err(not_found_domain(&domain_id));
    }
    if db.list_intents(&domain_id)?.iter().any(|i| i.name == name) {
        return Err(AppError::InvalidInput(format!(
            "该领域内已存在名为「{name}」的意图"
        )));
    }

    let record = IntentRecord {
        id: uuid::Uuid::new_v4().to_string(),
        domain_id,
        name: name.to_string(),
        description: intent.description.clone(),
        examples: intent.examples.clone(),
        workflow: intent.workflow.clone(),
        allowed_tools: intent.allowed_tools.clone(),
        default_agent: intent.default_agent.clone(),
        scope: intent.scope.clone(),
        enabled: intent.enabled,
        version: 1,
        confidence_threshold: intent.confidence_threshold,
        fallback_intent: intent.fallback_intent.clone(),
        path: intent.path.trim().to_string(),
        auto_escalate: intent.auto_escalate,
        system_prompt_appendix: intent.system_prompt_appendix.clone(),
        model_id: intent.model_id.clone(),
        max_iterations: intent.max_iterations,
        sort_order: intent.sort_order,
        created_at: timestamp.clone(),
        updated_at: timestamp,
    };
    db.insert_intent(&record)?;
    Ok(record.into())
}

#[tauri::command]
pub async fn update_intent(
    state: State<'_, AppState>,
    id: String,
    intent: IntentInput,
) -> Result<IntentInfo> {
    intent.validate()?;
    let name = intent.name.trim().to_string();

    let db = state.db.lock().await;
    let mut existing = db.get_intent(&id)?.ok_or_else(|| not_found_intent(&id))?;

    if db
        .list_intents(&existing.domain_id)?
        .iter()
        .any(|i| i.id != id && i.name == name)
    {
        return Err(AppError::InvalidInput(format!(
            "该领域内已存在名为「{name}」的意图"
        )));
    }

    intent.apply_to(&mut existing);
    existing.updated_at = now();
    db.update_intent(&existing)?;

    let updated = db.get_intent(&id)?.ok_or_else(|| not_found_intent(&id))?;
    Ok(updated.into())
}

#[tauri::command]
pub async fn delete_intent(state: State<'_, AppState>, id: String) -> Result<()> {
    let db = state.db.lock().await;
    db.delete_intent(&id)
}

#[tauri::command]
pub async fn set_intent_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<()> {
    let timestamp = now();
    let db = state.db.lock().await;
    db.set_intent_enabled(&id, enabled, &timestamp)
}

// ── Import / export ──

#[tauri::command]
pub async fn import_intent_domain(
    state: State<'_, AppState>,
    json: String,
    name: String,
) -> Result<IntentDomainInfo> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::InvalidInput("领域名称不能为空".to_string()));
    }

    let inputs = parse_intent_array(&json)?;
    let timestamp = now();
    let domain_id = uuid::Uuid::new_v4().to_string();
    let intents = to_records(&domain_id, &inputs, &timestamp)?;

    // Reject before touching the database: an imported domain that cannot be
    // activated is worse than no import at all.
    if !intents.iter().any(|intent| intent.enabled) {
        return Err(AppError::InvalidInput(
            "导入内容没有启用的意图，无法导入".to_string(),
        ));
    }

    let db = state.db.lock().await;
    let existing: Vec<String> = db
        .list_intent_domains()?
        .into_iter()
        .map(|domain| domain.name)
        .collect();

    let domain = IntentDomainRecord {
        id: domain_id,
        name: unique_domain_name(&existing, &name),
        description: String::new(),
        is_active: false,
        source: "user".to_string(),
        version: 1,
        created_at: timestamp.clone(),
        updated_at: timestamp,
    };
    db.insert_intent_domain_atomic(&domain, &intents)?;
    Ok(domain.into())
}

#[tauri::command]
pub async fn export_intent_domain(
    state: State<'_, AppState>,
    domain_id: String,
) -> Result<String> {
    let db = state.db.lock().await;
    if db.get_intent_domain(&domain_id)?.is_none() {
        return Err(not_found_domain(&domain_id));
    }
    let intents = db.list_intents(&domain_id)?;
    to_json_array(&intents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_name_returns_base_when_free() {
        let existing = vec!["程序开发".to_string(), "写作".to_string()];
        assert_eq!(unique_domain_name(&existing, "翻译"), "翻译");
    }

    #[test]
    fn unique_name_appends_suffix() {
        let existing = vec!["导入领域".to_string(), "导入领域 (2)".to_string()];
        assert_eq!(unique_domain_name(&existing, "导入领域"), "导入领域 (3)");
    }

    #[test]
    fn unique_name_falls_back_after_twenty_suffixes() {
        let mut existing = vec!["领域".to_string()];
        for n in 2..=21 {
            existing.push(format!("领域 ({n})"));
        }
        assert_eq!(unique_domain_name(&existing, "领域"), "领域 (uuid)");
    }
}
