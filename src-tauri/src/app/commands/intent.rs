//! 意图管理的 IPC 命令。
//!
//! 契约见设计文档 §7。每个写命令自己写 `updated_at`：repository 层不读时钟。
//!
//! 前端从不发送整条数据库记录。`IntentInput` 只携带可编辑字段，且每个字段
//! 都是可选的：**创建时缺省表示用 schema 默认值，更新时缺省表示「保留已
//! 存储的值」**。`id`、`domain_id`、`version` 与时间戳始终由后端持有。

use tauri::State;

use crate::app::state::AppState;
use crate::core::db::models::{IntentDomainRecord, IntentRecord};
use crate::core::db::repository::Database;
use crate::core::error::{AppError, Result};
use crate::features::intent_management::json::{parse_intent_array, to_json_array, to_records};

// ── DTO（传输对象） ──

/// 领域的传输形状，逐字段镜像 [`IntentDomainRecord`]。
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

/// 意图的传输形状，逐字段镜像 [`IntentRecord`]。
///
/// `examples` 与 `allowed_tools` 是真正的数组，前端不需要再解析 JSON 字符串。
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

/// 会话领域绑定的传输形状，由聊天头部的领域选择器渲染。
///
/// 领域行被删除后仍保留 `domain_id`，让前端还能描述已存储的先验。
#[derive(Debug, serde::Serialize)]
pub struct SessionIntentState {
    pub domain_id: Option<String>,
    pub domain_name: Option<String>,
    /// `auto`（id 只是路由先验）或 `pinned`（用户显式选择）。
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

/// 把「存在」的字段反序列化成 `Some`，包括显式 JSON `null`，
/// 这样 `Option<Option<T>>` 才能区分「缺省」与「存在但为 null」。
fn deserialize_some<'de, T, D>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    T: serde::Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// 前端提交的可编辑意图字段。
///
/// 每个字段都是可选的：创建时缺省字段取 schema 默认值，更新时缺省字段
/// 保留已存储值。`name` 是唯一在创建时必填、在更新时「存在才校验」的字段。
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
    /// `Some(None)` 表示清空该值；`None` 表示保留已存储值。
    #[serde(default, deserialize_with = "deserialize_some")]
    pub max_iterations: Option<Option<usize>>,
    pub sort_order: Option<i64>,
}

impl IntentInput {
    /// 校验路由器依赖的字段。错误消息会点名出问题的字段，而不是让一条坏
    /// 数据写进 SQLite。
    ///
    /// `require_name` 在创建时为 true（无名称的意图非法），在更新时为
    /// false（缺省表示「保留已存储的名称」）。
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

        // 深度路由的消息在 `resolve_tools` 之前就离开了本命令，因此空的工具
        // 列表会被静默解释成「整个注册表都可用」。数据模型不能承诺一个运行
        // 时根本施加不了的限制。
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

    /// 把存在的字段拷贝到已有记录上；缺省字段保留已存储值。
    /// 身份、`version` 与 `created_at` 不动。
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

    /// 构造一条新记录：缺省字段回退到 schema 默认值，存在的字段优先。
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

// ── 辅助函数 ──

/// `wanted` 未被占用时原样返回，否则追加第一个空闲的 ` (n)` 后缀（n 从 2
/// 开始）。尝试 20 次后改用 uuid 后缀，保证返回值永远是可用的唯一领域名。
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

/// 拒绝被其他记录占用的名称。`items` 产出 `(id, name)` 对；
/// `except_id` 用于跳过正在更新的那条记录。
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

/// 把非空的 `fallback_intent` 解析成目标意图的 id（支持按 id 或按名称）。
/// 空值保持为空。`intents` 是已存储列表，`merged` 是即将写入的记录，因此
/// 改名后可以指向自己的新名字。匹配不到任何意图时返回点名该值的
/// `InvalidInput`。
///
/// 调用方必须把返回的 id 写回记录：分类器只用存储值去比对 `intent.id`，
/// 原样存一个名称会静默退化成 `clarify`。
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

/// 写入后重建进程内意图索引。旧快照会继续服务，直到 `warm_up` 原子替换它。
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

// ── 会话领域绑定 ──

/// 会话领域绑定的两种模式值。
///
/// 这两个字符串会落库，也会经 IPC 回传前端，**取值不可更改**。`session.rs`
/// 里的路由逻辑同样使用它们，避免各处重复字面量。
pub(crate) const MODE_AUTO: &str = "auto";
pub(crate) const MODE_PINNED: &str = "pinned";

/// 前端请求的绑定模式。`None` 表示「自动」，而不是「不要领域」：
/// 已存储的 id 会继续作为路由先验保留。
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

/// 决策所需的已存储状态，全部用普通值表示，便于脱离数据库做单元测试。
#[derive(Debug, Clone, Copy)]
pub(crate) struct SessionIntentFacts<'a> {
    pub session_id: &'a str,
    pub session_exists: bool,
    /// 当前已存储的先验；切回 `auto` 时原样复用。
    pub stored_domain_id: Option<&'a str>,
    /// **请求的**领域行是否存在。
    pub domain_exists: bool,
    /// **请求的**领域中已启用的意图数量。
    pub enabled_intent_count: i64,
}

/// 一次成功变更要持久化的内容。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SessionIntentDecision<'a> {
    pub mode: SessionIntentMode,
    /// `None` 会抹掉先验，所以 `auto` 返回已存储的 id 而不是 `None`。
    pub domain_id: Option<&'a str>,
}

/// 决定会话领域变更应该写什么。
///
/// - 会话不存在 → `NotFound`；
/// - `pinned` 要求领域存在且至少有一个启用意图，与
///   `set_active_intent_domain` 的不变量一致；
/// - `auto` 永远成功，并保留已存储领域作为先验。
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

/// 把已存储的列投影成传输状态。
///
/// 领域被删除后仍保留 id（选择器还能显示它），但解析不到名称；这不算错误。
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

// ── 领域 ──

/// 列出全部意图领域，按 `sort_order` 排序，含内置领域。
#[tauri::command]
pub async fn list_intent_domains(state: State<'_, AppState>) -> Result<Vec<IntentDomainInfo>> {
    let db = state.db.lock().await;
    Ok(db
        .list_intent_domains()?
        .into_iter()
        .map(IntentDomainInfo::from)
        .collect())
}

/// 取当前激活的领域；没有激活项时返回 `Ok(None)`。
#[tauri::command]
pub async fn get_active_intent_domain(
    state: State<'_, AppState>,
) -> Result<Option<IntentDomainInfo>> {
    let db = state.db.lock().await;
    Ok(db.get_active_intent_domain()?.map(IntentDomainInfo::from))
}

/// 新建用户领域。新领域**不会**自动激活，`is_active` 恒为 false。
///
/// `name` 会 trim；空名或重名都会被拒绝。
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

/// 修改领域名称与描述。重名检查会跳过领域自身。
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

/// 删除领域及其意图；成功后重建内存索引。
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

/// 设置全局激活领域（影响新建会话的默认领域继承）。
#[tauri::command]
pub async fn set_active_intent_domain(state: State<'_, AppState>, id: String) -> Result<()> {
    let db = state.db.lock().await;
    db.set_active_intent_domain(&id)
}

/// 读取会话当前的领域绑定状态，供聊天头部选择器渲染。
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

/// 设置会话的领域绑定。
///
/// `domain_id` 的语义：
/// - `Some(id)` → `pinned`，要求领域存在且至少有一个启用意图；
/// - `None` → 切回 `auto`，**不清空**已存储的领域，它继续作为路由先验。
///
/// 返回写入后的绑定状态。
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

// ── 意图 ──

/// 最近的意图路由轨迹（内存环形缓冲），用于排查「为什么走了这条路径」。
#[tauri::command]
pub async fn get_recent_route_traces(
    state: State<'_, AppState>,
) -> Result<Vec<crate::llm::intent::RouteTrace>> {
    Ok(state.intent_router.recent_traces().await)
}

/// 列出某个领域下的全部意图（含已禁用的），按 `sort_order` 排序。
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

/// 按 id 读取单个意图；不存在时返回 `NotFound`。
#[tauri::command]
pub async fn get_intent(state: State<'_, AppState>, id: String) -> Result<IntentInfo> {
    let db = state.db.lock().await;
    let intent = db.get_intent(&id)?.ok_or_else(|| not_found_intent(&id))?;
    Ok(intent.into())
}

/// 在指定领域下新建意图。
///
/// `intent` 是稀疏输入：缺省字段取 schema 默认值。名称必填且领域内唯一；
/// `fallback_intent` 可以写 id 或名称，落库前统一解析成目标 id。
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

/// 更新意图。
///
/// `intent` 是稀疏输入：缺省字段保留已存储值，`Some(None)` 才表示清空
/// `max_iterations`。名称存在时做唯一性校验（跳过自身）。
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

/// 删除意图；成功后重建内存索引。
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

/// 启用或禁用单个意图；禁用后它不参与路由召回。
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

// ── 导入 / 导出 ──

/// 从 JSON 导入一个意图领域。
///
/// `json` 是意图数组；`name` 是期望的领域名，重名时自动追加 ` (n)` 后缀。
/// 领域与意图在同一个事务里写入，导入内容不含启用意图时整批拒绝。
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

    // 在碰数据库之前就拒绝：一个导入进来却无法激活的领域，比没有导入更糟。
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

/// 把一个领域及其意图导出为 JSON 数组字符串（不含领域元数据）。
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

        // 缺省的工具列表并未对运行时契约做出任何承诺，因此 schema 默认值
        // （同样是空列表）不应在校验阶段被拦截。
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

    // ── 会话领域绑定 ──

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
