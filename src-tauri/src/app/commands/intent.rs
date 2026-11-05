//! Intent management IPC commands.
//!
//! See the design note §7 for the contract. Every write command stamps
//! `updated_at` itself: the repository never reads the clock.
//!
//! The frontend never sends a whole database record for a write. `IntentInput`
//! carries only the editable fields and treats every field as optional:
//! **absent on create means the schema default, absent on update means "keep
//! the stored value"**. `id`, `domain_id`, `version` and the timestamps stay
//! owned by the backend.

use tauri::State;

use crate::app::state::AppState;
use crate::core::db::models::{IntentDomainRecord, IntentRecord};
use crate::core::db::repository::Database;
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

/// Wire shape of a session's domain binding, rendered by the chat header
/// selector. `domain_id` is kept even when the domain row is gone so the
/// front-end can still describe the stored prior.
#[derive(Debug, serde::Serialize)]
pub struct SessionIntentState {
    pub domain_id: Option<String>,
    pub domain_name: Option<String>,
    /// `auto` (the id is only a routing prior) or `pinned` (user chose it).
    pub mode: String,
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

/// Deserializes a present field into `Some`, including an explicit JSON `null`,
/// so `Option<Option<T>>` can tell "absent" from "present but null".
fn deserialize_some<'de, T, D>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    T: serde::Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// Editable intent fields accepted from the frontend.
///
/// Every field is optional: create fills absent fields from the schema
/// defaults, update keeps the stored value for absent fields. `name` is the
/// one field that is required on create and validated when present on update.
#[derive(serde::Deserialize)]
pub struct IntentInput {
    pub name: Option<String>,
    pub description: Option<String>,
    pub examples: Option<Vec<String>>,
    pub workflow: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
    pub default_agent: Option<String>,
    pub scope: Option<String>,
    pub enabled: Option<bool>,
    pub confidence_threshold: Option<f64>,
    pub fallback_intent: Option<String>,
    pub path: Option<String>,
    pub auto_escalate: Option<bool>,
    pub system_prompt_appendix: Option<String>,
    pub model_id: Option<String>,
    /// `Some(None)` clears the value; `None` keeps whatever is stored.
    #[serde(default, deserialize_with = "deserialize_some")]
    pub max_iterations: Option<Option<usize>>,
    pub sort_order: Option<i64>,
}

impl IntentInput {
    /// Validates the fields the router relies on. Returns a message naming the
    /// offending field instead of letting a bad row reach SQLite.
    ///
    /// `require_name` is true on create (a nameless intent is invalid) and
    /// false on update (absent means "keep the stored name").
    fn validate(&self, require_name: bool) -> Result<()> {
        match self.name.as_deref().map(str::trim) {
            Some("") => {
                return Err(AppError::InvalidInput("意图名称不能为空".to_string()));
            }
            None if require_name => {
                return Err(AppError::InvalidInput("意图名称不能为空".to_string()));
            }
            _ => {}
        }

        if let Some(path) = self.path.as_deref() {
            let path = path.trim();
            if !matches!(path, "fast" | "deep") {
                return Err(AppError::InvalidInput(format!(
                    "path 必须是 fast 或 deep，实际为「{path}」"
                )));
            }
        }

        // Deep-routed messages leave this command before `resolve_tools` runs,
        // so an empty tool list would silently grant the whole registry. The
        // data model must not promise a restriction the runtime cannot apply.
        if self.path.as_deref().map(str::trim) == Some("deep")
            && matches!(self.allowed_tools.as_deref(), Some([]))
        {
            return Err(AppError::InvalidInput(
                "path 为 deep 的意图无法限制工具：allowed_tools 不能为空".to_string(),
            ));
        }

        if let Some(threshold) = self.confidence_threshold {
            if !threshold.is_finite() || !(0.0..=1.0).contains(&threshold) {
                return Err(AppError::InvalidInput(format!(
                    "confidence_threshold 必须在 [0.0, 1.0] 之间，实际为 {threshold}"
                )));
            }
        }

        Ok(())
    }

    /// Copies the present fields onto an existing record; absent fields keep
    /// the stored value. Identity, `version` and `created_at` are untouched.
    fn apply_to(self, record: &mut IntentRecord) {
        if let Some(name) = self.name {
            record.name = name.trim().to_string();
        }
        if let Some(description) = self.description {
            record.description = description;
        }
        if let Some(examples) = self.examples {
            record.examples = examples;
        }
        if let Some(workflow) = self.workflow {
            record.workflow = workflow;
        }
        if let Some(allowed_tools) = self.allowed_tools {
            record.allowed_tools = allowed_tools;
        }
        if let Some(default_agent) = self.default_agent {
            record.default_agent = default_agent;
        }
        if let Some(scope) = self.scope {
            record.scope = scope;
        }
        if let Some(enabled) = self.enabled {
            record.enabled = enabled;
        }
        if let Some(confidence_threshold) = self.confidence_threshold {
            record.confidence_threshold = confidence_threshold;
        }
        if let Some(fallback_intent) = self.fallback_intent {
            record.fallback_intent = fallback_intent.trim().to_string();
        }
        if let Some(path) = self.path {
            record.path = path.trim().to_string();
        }
        if let Some(auto_escalate) = self.auto_escalate {
            record.auto_escalate = auto_escalate;
        }
        if let Some(system_prompt_appendix) = self.system_prompt_appendix {
            record.system_prompt_appendix = system_prompt_appendix;
        }
        if let Some(model_id) = self.model_id {
            record.model_id = model_id;
        }
        if let Some(max_iterations) = self.max_iterations {
            record.max_iterations = max_iterations;
        }
        if let Some(sort_order) = self.sort_order {
            record.sort_order = sort_order;
        }
    }

    /// Builds a fresh record: absent fields fall back to the schema defaults,
    /// present fields win.
    fn into_new_record(self, id: String, domain_id: String, timestamp: String) -> IntentRecord {
        let mut record = IntentRecord {
            id,
            domain_id,
            name: String::new(),
            description: String::new(),
            examples: Vec::new(),
            workflow: String::new(),
            allowed_tools: Vec::new(),
            default_agent: String::new(),
            scope: default_scope(),
            enabled: default_true(),
            version: 1,
            confidence_threshold: default_threshold(),
            fallback_intent: String::new(),
            path: default_path(),
            auto_escalate: false,
            system_prompt_appendix: String::new(),
            model_id: String::new(),
            max_iterations: None,
            sort_order: 0,
            created_at: timestamp.clone(),
            updated_at: timestamp,
        };
        self.apply_to(&mut record);
        record
    }
}

// ── Helpers ──

/// Returns `wanted` when it is not already taken, otherwise appends the first
/// free ` (n)` suffix, n starting at 2. After 20 attempts a fresh uuid is
/// appended, so the returned value is always a usable, unique domain name.
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

    format!("{wanted} ({})", uuid::Uuid::new_v4())
}

/// Rejects a name already used by another record. `items` yields `(id, name)`
/// pairs; `except_id` skips the record being updated.
fn ensure_name_available<'a, I>(
    items: I,
    name: &str,
    except_id: Option<&str>,
    message: String,
) -> Result<()>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let taken = items
        .into_iter()
        .any(|(id, existing)| existing == name && except_id != Some(id));
    if taken {
        return Err(AppError::InvalidInput(message));
    }
    Ok(())
}

/// Resolves a non-empty `fallback_intent` to the target intent's id, by id or
/// by name. Empty stays empty. `intents` is the stored list and `merged` is the
/// record about to be written, so a rename can target its own new name. A value
/// that matches nothing is rejected with `InvalidInput` naming it.
///
/// Callers must write the returned id back into the record: the classifier
/// compares the stored value against `intent.id` only, so a name stored verbatim
/// would silently degrade to `clarify`.
fn resolve_fallback_target(intents: &[IntentRecord], merged: &IntentRecord) -> Result<String> {
    let target = merged.fallback_intent.trim();
    if target.is_empty() {
        return Ok(String::new());
    }

    let found = std::iter::once(merged)
        .chain(intents.iter().filter(|intent| intent.id != merged.id))
        .find(|intent| intent.id == target || intent.name == target);

    match found {
        Some(intent) => Ok(intent.id.clone()),
        None => Err(AppError::InvalidInput(format!(
            "fallback_intent「{target}」在该领域中找不到对应意图"
        ))),
    }
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Rebuild the in-process intent index after a write. The old snapshot keeps
/// serving until `warm_up` atomically replaces it.
fn refresh_intent_index(state: &AppState) {
    let router = state.intent_router.clone();
    tauri::async_runtime::spawn(async move {
        router.mark_dirty().await;
        router.warm_up().await;
    });
}

fn not_found_domain(id: &str) -> AppError {
    AppError::NotFound(format!("领域不存在: {id}"))
}

fn not_found_intent(id: &str) -> AppError {
    AppError::NotFound(format!("意图不存在: {id}"))
}

fn not_found_session(id: &str) -> AppError {
    AppError::NotFound(format!("会话不存在: {id}"))
}

// ── Session domain binding ──

const MODE_AUTO: &str = "auto";
const MODE_PINNED: &str = "pinned";

/// Requested binding for a session. `None` from the front-end means "auto",
/// not "no domain": the stored id survives as the routing prior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionIntentMode {
    Auto,
    Pinned,
}

impl SessionIntentMode {
    fn from_domain_id(domain_id: Option<&str>) -> Self {
        match domain_id {
            Some(_) => Self::Pinned,
            None => Self::Auto,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => MODE_AUTO,
            Self::Pinned => MODE_PINNED,
        }
    }
}

/// Everything the decision needs about the stored state, kept as plain values
/// so it can be unit-tested without a database.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SessionIntentFacts<'a> {
    pub session_id: &'a str,
    pub session_exists: bool,
    /// Currently stored prior; reused as-is when switching to `auto`.
    pub stored_domain_id: Option<&'a str>,
    /// Whether the *requested* domain row exists.
    pub domain_exists: bool,
    /// Enabled intents of the *requested* domain.
    pub enabled_intent_count: i64,
}

/// What a successful change persists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SessionIntentDecision<'a> {
    pub mode: SessionIntentMode,
    /// `None` would erase the prior, so `auto` returns the stored id instead.
    pub domain_id: Option<&'a str>,
}

/// Decides what a session domain change should write.
///
/// - missing session is `NotFound`;
/// - `pinned` requires an existing domain with at least one enabled intent,
///   the same invariant `set_active_intent_domain` enforces;
/// - `auto` always succeeds and keeps the stored domain as prior.
pub(crate) fn decide_session_intent_change<'a>(
    facts: SessionIntentFacts<'a>,
    mode: SessionIntentMode,
    requested_domain_id: Option<&'a str>,
) -> Result<SessionIntentDecision<'a>> {
    if !facts.session_exists {
        return Err(not_found_session(facts.session_id));
    }

    match mode {
        SessionIntentMode::Auto => Ok(SessionIntentDecision {
            mode,
            domain_id: facts.stored_domain_id,
        }),
        SessionIntentMode::Pinned => {
            let domain_id = requested_domain_id.ok_or_else(|| {
                AppError::InvalidInput("pinned 模式必须指定领域".to_string())
            })?;
            if !facts.domain_exists {
                return Err(not_found_domain(domain_id));
            }
            if facts.enabled_intent_count <= 0 {
                return Err(AppError::InvalidInput(
                    "该领域没有启用的意图，无法设为会话领域".to_string(),
                ));
            }
            Ok(SessionIntentDecision {
                mode,
                domain_id: Some(domain_id),
            })
        }
    }
}

/// Projects the stored columns into the wire state. A deleted domain keeps its
/// id (the selector can still show it) but resolves no name; that is not an
/// error.
fn session_intent_state_from(
    domain_id: Option<&str>,
    domain_name: Option<String>,
    mode: &str,
) -> SessionIntentState {
    SessionIntentState {
        domain_id: domain_id.map(str::to_string),
        domain_name,
        mode: mode.to_string(),
    }
}

fn domain_name_for(db: &Database, domain_id: Option<&str>) -> Result<Option<String>> {
    match domain_id {
        Some(id) => Ok(db.get_intent_domain(id)?.map(|domain| domain.name)),
        None => Ok(None),
    }
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
    ensure_name_available(
        db.list_intent_domains()?
            .iter()
            .map(|domain| (domain.id.as_str(), domain.name.as_str())),
        &name,
        None,
        format!("领域名称已存在：{name}"),
    )?;

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

    ensure_name_available(
        db.list_intent_domains()?
            .iter()
            .map(|domain| (domain.id.as_str(), domain.name.as_str())),
        &name,
        Some(&id),
        format!("领域名称已存在：{name}"),
    )?;

    domain.name = name;
    domain.description = description;
    domain.updated_at = now();
    db.update_intent_domain(&domain)?;
    Ok(domain.into())
}

#[tauri::command]
pub async fn delete_intent_domain(state: State<'_, AppState>, id: String) -> Result<()> {
    let result = {
        let db = state.db.lock().await;
        db.delete_intent_domain(&id)
    };
    if result.is_ok() {
        refresh_intent_index(state.inner());
    }
    result
}

#[tauri::command]
pub async fn set_active_intent_domain(state: State<'_, AppState>, id: String) -> Result<()> {
    let db = state.db.lock().await;
    db.set_active_intent_domain(&id)
}

#[tauri::command]
pub async fn get_session_intent_state(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<SessionIntentState> {
    let db = state.db.lock().await;
    let session = db
        .get_session(&session_id)?
        .ok_or_else(|| not_found_session(&session_id))?;
    let domain_name = domain_name_for(&db, session.intent_domain_id.as_deref())?;
    Ok(session_intent_state_from(
        session.intent_domain_id.as_deref(),
        domain_name,
        &session.intent_domain_mode,
    ))
}

#[tauri::command]
pub async fn set_session_intent_domain(
    state: State<'_, AppState>,
    session_id: String,
    domain_id: Option<String>,
) -> Result<SessionIntentState> {
    let db = state.db.lock().await;
    let session = db.get_session(&session_id)?;

    let requested_domain_id = domain_id.as_deref();
    let mode = SessionIntentMode::from_domain_id(requested_domain_id);
    let domain_exists = match requested_domain_id {
        Some(id) => db.get_intent_domain(id)?.is_some(),
        None => false,
    };
    let enabled_intent_count = match requested_domain_id {
        Some(id) => db.count_enabled_intents(id)?,
        None => 0,
    };

    let decision = decide_session_intent_change(
        SessionIntentFacts {
            session_id: &session_id,
            session_exists: session.is_some(),
            stored_domain_id: session
                .as_ref()
                .and_then(|record| record.intent_domain_id.as_deref()),
            domain_exists,
            enabled_intent_count,
        },
        mode,
        requested_domain_id,
    )?;

    db.set_session_intent_domain(&session_id, decision.domain_id, decision.mode.as_str())?;
    let domain_name = domain_name_for(&db, decision.domain_id)?;
    Ok(session_intent_state_from(
        decision.domain_id,
        domain_name,
        decision.mode.as_str(),
    ))
}

// ── Intents ──

#[tauri::command]
pub async fn get_recent_route_traces(
    state: State<'_, AppState>,
) -> Result<Vec<crate::llm::intent::RouteTrace>> {
    Ok(state.intent_router.recent_traces().await)
}

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
    intent.validate(true)?;
    let name = intent
        .name
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_string();

    let timestamp = now();
    let db = state.db.lock().await;
    if db.get_intent_domain(&domain_id)?.is_none() {
        return Err(not_found_domain(&domain_id));
    }

    let intents = db.list_intents(&domain_id)?;
    ensure_name_available(
        intents
            .iter()
            .map(|existing| (existing.id.as_str(), existing.name.as_str())),
        &name,
        None,
        format!("该领域内已存在名为「{name}」的意图"),
    )?;

    let mut record = intent.into_new_record(uuid::Uuid::new_v4().to_string(), domain_id, timestamp);
    record.fallback_intent = resolve_fallback_target(&intents, &record)?;
    db.insert_intent(&record)?;
    drop(db);
    refresh_intent_index(state.inner());
    Ok(record.into())
}

#[tauri::command]
pub async fn update_intent(
    state: State<'_, AppState>,
    id: String,
    intent: IntentInput,
) -> Result<IntentInfo> {
    intent.validate(false)?;

    let db = state.db.lock().await;
    let mut existing = db.get_intent(&id)?.ok_or_else(|| not_found_intent(&id))?;
    let intents = db.list_intents(&existing.domain_id)?;

    if let Some(name) = intent.name.as_deref().map(str::trim) {
        ensure_name_available(
            intents
                .iter()
                .map(|record| (record.id.as_str(), record.name.as_str())),
            name,
            Some(&id),
            format!("该领域内已存在名为「{name}」的意图"),
        )?;
    }

    intent.apply_to(&mut existing);
    existing.updated_at = now();
    existing.fallback_intent = resolve_fallback_target(&intents, &existing)?;
    db.update_intent(&existing)?;

    let updated = db.get_intent(&id)?.ok_or_else(|| not_found_intent(&id))?;
    drop(db);
    refresh_intent_index(state.inner());
    Ok(updated.into())
}

#[tauri::command]
pub async fn delete_intent(state: State<'_, AppState>, id: String) -> Result<()> {
    let result = {
        let db = state.db.lock().await;
        db.delete_intent(&id)
    };
    if result.is_ok() {
        refresh_intent_index(state.inner());
    }
    result
}

#[tauri::command]
pub async fn set_intent_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<()> {
    let timestamp = now();
    let db = state.db.lock().await;
    let result = db.set_intent_enabled(&id, enabled, &timestamp);
    drop(db);
    if result.is_ok() {
        refresh_intent_index(state.inner());
    }
    result
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
    drop(db);
    refresh_intent_index(state.inner());
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

    const TEST_TS: &str = "2026-11-04T00:00:00Z";

    fn mk_record() -> IntentRecord {
        IntentRecord {
            id: "intent-a".to_string(),
            domain_id: "domain-a".to_string(),
            name: "意图甲".to_string(),
            description: "旧描述".to_string(),
            examples: vec!["旧示例".to_string()],
            workflow: String::new(),
            allowed_tools: vec!["read_file".to_string()],
            default_agent: String::new(),
            scope: "global".to_string(),
            enabled: false,
            version: 1,
            confidence_threshold: 0.5,
            fallback_intent: String::new(),
            path: "deep".to_string(),
            auto_escalate: false,
            system_prompt_appendix: String::new(),
            model_id: String::new(),
            max_iterations: Some(7),
            sort_order: 42,
            created_at: TEST_TS.to_string(),
            updated_at: TEST_TS.to_string(),
        }
    }

    fn blank_input() -> IntentInput {
        IntentInput {
            name: None,
            description: None,
            examples: None,
            workflow: None,
            allowed_tools: None,
            default_agent: None,
            scope: None,
            enabled: None,
            confidence_threshold: None,
            fallback_intent: None,
            path: None,
            auto_escalate: None,
            system_prompt_appendix: None,
            model_id: None,
            max_iterations: None,
            sort_order: None,
        }
    }

    #[test]
    fn deep_intent_with_empty_tool_list_is_rejected() {
        let input = IntentInput {
            name: Some("深度意图".to_string()),
            path: Some("deep".to_string()),
            allowed_tools: Some(Vec::new()),
            ..blank_input()
        };

        let err = input.validate(true).unwrap_err();
        match err {
            AppError::InvalidInput(message) => {
                assert!(message.contains("deep"), "错误必须点名 path：{message}");
                assert!(
                    message.contains("allowed_tools"),
                    "错误必须点名 allowed_tools：{message}"
                );
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    #[test]
    fn deep_intent_with_tools_and_fast_intent_without_tools_pass() {
        let deep_with_tools = IntentInput {
            name: Some("深度意图".to_string()),
            path: Some("deep".to_string()),
            allowed_tools: Some(vec!["file_system".to_string()]),
            ..blank_input()
        };
        assert!(deep_with_tools.validate(true).is_ok());

        let fast_without_tools = IntentInput {
            name: Some("快速意图".to_string()),
            path: Some("fast".to_string()),
            allowed_tools: Some(Vec::new()),
            ..blank_input()
        };
        assert!(fast_without_tools.validate(true).is_ok());

        // An absent tool list says nothing about the runtime contract, so the
        // schema default (also empty) must not be blocked at validation time.
        let deep_without_tools_field = IntentInput {
            name: Some("深度意图".to_string()),
            path: Some("deep".to_string()),
            ..blank_input()
        };
        assert!(deep_without_tools_field.validate(true).is_ok());
    }

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
    fn unique_name_falls_back_to_a_real_uuid() {
        let mut existing = vec!["领域".to_string()];
        for n in 2..=21 {
            existing.push(format!("领域 ({n})"));
        }

        let name = unique_domain_name(&existing, "领域");
        assert!(!existing.contains(&name), "兜底名称不得与已有名称重复");

        let suffix = name
            .strip_prefix("领域 (")
            .and_then(|rest| rest.strip_suffix(')'))
            .expect("兜底名称必须保留「名称 (后缀)」形状");
        assert!(
            uuid::Uuid::parse_str(suffix).is_ok(),
            "兜底后缀必须是合法 uuid，实际：{suffix}"
        );
    }

    #[test]
    fn sparse_update_keeps_absent_fields() {
        let mut record = mk_record();
        let input = IntentInput {
            name: Some("新名字".to_string()),
            ..blank_input()
        };

        input.apply_to(&mut record);

        assert_eq!(record.name, "新名字");
        assert!(!record.enabled, "未提供 enabled 时不得重新启用");
        assert_eq!(record.description, "旧描述", "未提供 description 时不得清空");
        assert_eq!(record.examples, vec!["旧示例".to_string()]);
        assert_eq!(record.path, "deep");
        assert_eq!(record.confidence_threshold, 0.5);
        assert_eq!(record.max_iterations, Some(7));
        assert_eq!(record.sort_order, 42);
    }

    #[test]
    fn create_fills_absent_fields_with_schema_defaults() {
        let input = IntentInput {
            name: Some("新意图".to_string()),
            ..blank_input()
        };

        let record = input.into_new_record(
            "new-id".to_string(),
            "domain-a".to_string(),
            TEST_TS.to_string(),
        );

        assert_eq!(record.name, "新意图");
        assert_eq!(record.path, "fast");
        assert!(record.enabled);
        assert_eq!(record.confidence_threshold, 0.7);
        assert_eq!(record.scope, "global");
        assert_eq!(record.version, 1);
        assert_eq!(record.sort_order, 0);
        assert!(record.max_iterations.is_none());
        assert!(record.description.is_empty());
        assert!(record.examples.is_empty());
    }

    #[test]
    fn explicit_null_clears_max_iterations() {
        let explicit: IntentInput =
            serde_json::from_str(r#"{"max_iterations": null}"#).unwrap();
        assert_eq!(explicit.max_iterations, Some(None));

        let absent: IntentInput = serde_json::from_str("{}").unwrap();
        assert_eq!(absent.max_iterations, None);
    }

    #[test]
    fn fallback_resolves_by_name_and_by_id() {
        let mut target = mk_record();
        target.id = "target".to_string();
        target.name = "目标意图".to_string();

        let mut referencing = mk_record();
        referencing.id = "referencing".to_string();
        referencing.fallback_intent = "目标意图".to_string();
        assert_eq!(
            resolve_fallback_target(&[target.clone()], &referencing).unwrap(),
            "target"
        );

        referencing.fallback_intent = "target".to_string();
        assert_eq!(
            resolve_fallback_target(&[target.clone()], &referencing).unwrap(),
            "target"
        );

        referencing.fallback_intent = "不存在的意图".to_string();
        let err = resolve_fallback_target(&[target], &referencing).unwrap_err();
        match err {
            AppError::InvalidInput(message) => {
                assert!(message.contains("不存在的意图"), "错误必须点名该值：{message}");
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    #[test]
    fn empty_fallback_stays_empty() {
        let mut record = mk_record();
        record.fallback_intent = "   ".to_string();

        assert_eq!(resolve_fallback_target(&[], &record).unwrap(), "");
    }

    #[test]
    fn create_resolves_named_fallback_to_target_id() {
        let target = mk_record(); // id intent-a, name 意图甲
        let input = IntentInput {
            name: Some("新意图".to_string()),
            fallback_intent: Some("意图甲".to_string()),
            ..blank_input()
        };

        let mut record = input.into_new_record(
            "new-id".to_string(),
            "domain-a".to_string(),
            TEST_TS.to_string(),
        );
        record.fallback_intent = resolve_fallback_target(&[target], &record).unwrap();

        assert_eq!(
            record.fallback_intent, "intent-a",
            "名称形式的 fallback 必须落库为目标 id"
        );
    }

    #[test]
    fn update_resolves_named_fallback_to_target_id() {
        let target = mk_record();
        let mut existing = mk_record();
        existing.id = "referencing".to_string();
        existing.name = "引用者".to_string();
        existing.fallback_intent = String::new();

        let input = IntentInput {
            fallback_intent: Some("意图甲".to_string()),
            ..blank_input()
        };
        input.apply_to(&mut existing);
        existing.fallback_intent = resolve_fallback_target(&[target], &existing).unwrap();

        assert_eq!(
            existing.fallback_intent, "intent-a",
            "名称形式的 fallback 必须落库为目标 id"
        );
    }

    #[test]
    fn fallback_can_target_the_merged_name_of_itself() {
        let stored = mk_record(); // stored name is 意图甲
        let mut merged = stored.clone();
        merged.name = "改名后的意图".to_string();
        merged.fallback_intent = "改名后的意图".to_string();

        assert_eq!(
            resolve_fallback_target(&[stored], &merged).unwrap(),
            merged.id,
            "合并后的新名称必须解析为自身 id"
        );
    }

    #[test]
    fn duplicate_name_check_skips_the_record_being_updated() {
        let record = mk_record();
        let items = [(record.id.as_str(), record.name.as_str())];

        assert!(ensure_name_available(
            items,
            "意图甲",
            Some("intent-a"),
            "重复".to_string()
        )
        .is_ok());
        assert!(ensure_name_available(
            items,
            "意图甲",
            None,
            "重复".to_string()
        )
        .is_err());
    }

    // ── Session domain binding ──

    fn facts<'a>(session_exists: bool, stored: Option<&'a str>) -> SessionIntentFacts<'a> {
        SessionIntentFacts {
            session_id: "session-a",
            session_exists,
            stored_domain_id: stored,
            domain_exists: true,
            enabled_intent_count: 2,
        }
    }

    #[test]
    fn missing_session_is_rejected_as_not_found() {
        let mut facts = facts(false, None);
        facts.domain_exists = false;
        facts.enabled_intent_count = 0;

        let err =
            decide_session_intent_change(facts, SessionIntentMode::Auto, None).unwrap_err();
        match err {
            AppError::NotFound(message) => {
                assert!(message.contains("session-a"), "错误必须点名会话：{message}");
            }
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn auto_keeps_the_stored_domain_as_prior() {
        let decision = decide_session_intent_change(
            facts(true, Some("domain-a")),
            SessionIntentMode::Auto,
            None,
        )
        .unwrap();

        assert_eq!(decision.mode, SessionIntentMode::Auto);
        assert_eq!(
            decision.domain_id,
            Some("domain-a"),
            "切到自动不得清空先验"
        );
    }

    #[test]
    fn auto_without_a_prior_stays_empty() {
        let decision =
            decide_session_intent_change(facts(true, None), SessionIntentMode::Auto, None).unwrap();

        assert_eq!(decision.mode, SessionIntentMode::Auto);
        assert_eq!(decision.domain_id, None);
    }

    #[test]
    fn pinned_accepts_a_domain_with_enabled_intents() {
        let decision = decide_session_intent_change(
            facts(true, Some("domain-old")),
            SessionIntentMode::Pinned,
            Some("domain-b"),
        )
        .unwrap();

        assert_eq!(decision.mode, SessionIntentMode::Pinned);
        assert_eq!(
            decision.domain_id,
            Some("domain-b"),
            "pinned 必须写入用户选择的领域，而不是先验"
        );
    }

    #[test]
    fn pinned_unknown_domain_is_rejected_as_not_found() {
        let mut facts = facts(true, None);
        facts.domain_exists = false;

        let err = decide_session_intent_change(
            facts,
            SessionIntentMode::Pinned,
            Some("ghost-domain"),
        )
        .unwrap_err();
        match err {
            AppError::NotFound(message) => {
                assert!(message.contains("ghost-domain"), "错误必须点名领域：{message}");
            }
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn pinned_domain_without_enabled_intents_is_rejected() {
        let mut facts = facts(true, None);
        facts.enabled_intent_count = 0;

        let err = decide_session_intent_change(
            facts,
            SessionIntentMode::Pinned,
            Some("domain-empty"),
        )
        .unwrap_err();
        match err {
            AppError::InvalidInput(message) => {
                assert!(message.contains("启用"), "错误必须说明原因：{message}");
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    #[test]
    fn pinned_without_a_domain_id_is_rejected() {
        let err = decide_session_intent_change(
            facts(true, None),
            SessionIntentMode::Pinned,
            None,
        )
        .unwrap_err();

        assert!(
            matches!(err, AppError::InvalidInput(_)),
            "pinned 缺领域必须是校验错误，实际：{err:?}"
        );
    }

    #[test]
    fn state_serializes_snake_case_and_tolerates_a_deleted_domain() {
        let state = session_intent_state_from(Some("domain-a"), None, MODE_AUTO);
        let json = serde_json::to_value(&state).unwrap();

        assert_eq!(json["domain_id"], "domain-a", "已删除领域仍要回传 id");
        assert!(json["domain_name"].is_null(), "已删除领域没有名称，且不得报错");
        assert_eq!(json["mode"], "auto");
        assert!(json.get("domainName").is_none(), "DTO 必须是 snake_case");
    }
}
