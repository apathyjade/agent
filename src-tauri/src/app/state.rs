//! 应用全局状态（Tauri managed state）。
//!
//! [`AppState`] 是每个 IPC command 拿到的唯一入口：数据库、配置、provider
//! 注册表、工具注册表、各 feature manager 都在这里集中构造一次，避免各命令
//! 各自 new 出互相看不见的副本。
//!
//! 这里只做依赖装配，不含业务规则。构造顺序有依赖关系，见 [`AppState::new`]。

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;

use crate::app::config::AppConfig;
use crate::agent::critic::agent::CriticAgent;
use crate::core::db::repository::Database;
use crate::features::session::lifecycle::LifecycleManager;
use crate::llm::embedding::EmbeddingService;
use crate::llm::intent::classifier::LlmReranker;
use crate::llm::intent::router::IntentRouter;
use crate::llm::intent::scope::{CatalogTool, ResourceCatalog};
use crate::llm::intent::RouterConfig;
use crate::llm::model::{ModelConfig, ModelProvider};
use crate::llm::provider::ProviderRegistry;
use crate::features::mcp::config::on_demand_config;
use crate::features::mcp::McpServerManager;
use crate::features::memory::MemoryManager;
use crate::agent::orchestrator::agent::OrchestratorAgent;
use crate::agent::orchestrator::dispatcher::Dispatcher;
use crate::agent::orchestrator::plan_types::ExecutionHandle;
use crate::features::persona::PersonaManager;
use crate::features::skills::SkillsService;
use crate::runtime::toolchain::RuntimeServices;
use crate::agent::tools::registry::{ToolRegistry, ToolResolver};
use crate::agent::workers::code_editor::CodeEditorWorker;
use crate::agent::workers::code_explorer::CodeExplorerWorker;
use crate::agent::workers::mcp_bridge::MCPBridgeWorker;
use crate::agent::workers::memory::MemoryWorker;
use crate::agent::workers::shell::ShellWorker;
use crate::agent::workers::thinker::ThinkerWorker;
use crate::agent::workers::web::WebWorker;

/// 整个应用共享的运行时状态。
///
/// 所有可变部分都用 `Arc<Mutex<..>>` 包装：Tauri 会跨线程、跨命令并发调用，
/// 用 `tokio::sync::Mutex` 而不是 `std::sync::Mutex`，因为多数持有者需要跨
/// `.await` 持有锁（例如先查库再调 LLM）。
pub struct AppState {
    /// 用于向前端 emit 事件、解析资源目录。
    pub app_handle: AppHandle,
    /// SQLite 连接。所有读写都必须先拿到这把锁。
    pub db: Arc<Mutex<Database>>,
    /// 内存中的配置副本，落盘通过 `save()`。
    pub config: Arc<Mutex<AppConfig>>,
    /// 已注册的 provider（持有真实 API key 与 HTTP client）。
    pub providers: Arc<Mutex<ProviderRegistry>>,
    /// 工具注册表，MCP 动态工具也注册在这里。
    pub tools: Arc<Mutex<ToolRegistry>>,
    /// skills 的安装、更新与市场查询。
    pub skills: SkillsService,
    /// 记忆的增删改查与召回。
    pub memory: MemoryManager,
    /// 人设的增删改查与自动匹配。
    pub persona: PersonaManager,
    /// MCP 连接池与工具注入。
    pub mcp: Arc<McpServerManager>,
    /// 工具按需解析钩子（MCP 按需连接）。
    pub tool_resolver: Arc<dyn ToolResolver>,
    /// 运行时检测、安装、版本切换。
    pub toolchain: RuntimeServices,
    /// 会话生命周期（摘要、归档、标题）。
    pub lifecycle: LifecycleManager,
    /// 意图路由器，持有内存索引。
    pub intent_router: Arc<IntentRouter>,
    /// 分层 agent 编排器。
    pub orchestrator: Arc<OrchestratorAgent>,
    /// 运行中的执行句柄（session_id → ExecutionHandle）。
    /// 深度执行注册进来，`pause/resume/cancel` 通过它控制运行中的计划。
    pub active_executions: Arc<Mutex<HashMap<String, ExecutionHandle>>>,
    /// 人工确认通道，供 `respond_tool_approval` 命令使用。
    pub approval: Arc<crate::agent::tools::approval::ToolApprovalService>,
    /// 回合日志收集器：回合进行中只存内存，回合结束才落库。
    pub turn_logs: Arc<TurnLogBook>,
}

impl AppState {
    /// 资源使用反馈接收端：把运行时上报的样本写入意图路由器。
    pub fn usage_sink(
        &self,
    ) -> Arc<dyn crate::agent::orchestrator::runtime::ScopeUsageSink> {
        Arc::new(RouterUsageSink {
            router: self.intent_router.clone(),
        })
    }

    /// 构造全部子系统。只应在启动时调用一次。
    ///
    /// 顺序约束：
    /// - 内置意图领域必须在 `db` 被 `Arc` 共享、路由器建立之前写入，
    ///   否则首轮路由看到的是空索引；`core` 不能依赖 `features`，所以
    ///   这一步只能放在这里而不是 `Database::new()`。
    /// - `embedding` 同时供记忆召回与意图召回使用，必须早于两者构造。
    /// - `intent_router` 建好后立刻异步 `warm_up`，避免首条消息等待建索引。
    pub fn new(app_handle: &AppHandle) -> crate::core::error::Result<Self> {
        let db = Database::new()?;
        // 种子数据必须在 db 被共享、router 被构造之前写入。
        crate::features::intent_management::seeds::seed_builtin_intents(&db)?;

        let config = AppConfig::load()?;
        // 配置里的 `api_key` 在 keychain 写入成功时是空串，真实 key 只在
        // 操作系统钥匙串里。注册表必须拿解析后的副本，否则每次重启后所有
        // 模型都会以空 key 发请求。
        let models = resolved_models(&config.models);
        // 审批服务必须先于 provider 与工具注册表构造：两者都要拿到同一个通道。
        let approval = Arc::new(
            crate::agent::tools::approval::ToolApprovalService::new(
                app_handle.clone(),
                crate::llm::provider::ToolPolicy::default(),
            ),
        );
        let providers_arc = Arc::new(Mutex::new(ProviderRegistry::new_with_approval(
            &models,
            Some(approval.clone()),
        )));
        // 内置工具的启用状态来自配置；MCP 工具在注册时自行决定，不受影响。
        let (tools_arc, builtin_catalog) = {
            let mut registry = ToolRegistry::new();
            registry.apply_builtin_enabled(&config.enabled_tools);
            registry.set_gate(approval.clone());
            let catalog = registry.catalog();
            (Arc::new(Mutex::new(registry)), catalog)
        };
        let db_arc = Arc::new(Mutex::new(db));

        let embedding = build_embedding_service(&models, config.intent.embedding_model_id.as_deref());
        let memory = MemoryManager::new(db_arc.clone(), Some(embedding.clone()));

        let intent_router = build_intent_router(
            &config.intent,
            db_arc.clone(),
            providers_arc.clone(),
            embedding,
            builtin_catalog,
        );

        let persona = PersonaManager::new(db_arc.clone());
        let lifecycle = LifecycleManager::new(db_arc.clone(), providers_arc.clone());

        let toolchain = RuntimeServices::new(config.runtime_options())?;
        let mcp = Arc::new(
            McpServerManager::new(tools_arc.clone())
                .with_runtime_manager(toolchain.manager.clone()),
        );
        // 配置要在解析器之前变成共享句柄：解析器按需查找 MCP 服务器配置。
        let config = Arc::new(Mutex::new(config));
        let tool_resolver: Arc<dyn ToolResolver> = Arc::new(McpOnDemandResolver {
            config: config.clone(),
            mcp: mcp.clone(),
            tools: tools_arc.clone(),
        });

        let orchestrator = Arc::new(build_orchestrator(
            providers_arc.clone(),
            db_arc.clone(),
            tools_arc.clone(),
            tool_resolver.clone(),
        ));

        Ok(Self {
            app_handle: app_handle.clone(),
            db: db_arc,
            config,
            providers: providers_arc,
            tools: tools_arc,
            skills: SkillsService::new(),
            memory,
            persona,
            mcp,
            tool_resolver,
            toolchain,
            lifecycle,
            intent_router,
            orchestrator,
            active_executions: Arc::new(Mutex::new(HashMap::new())),
            approval,
            turn_logs: Arc::new(TurnLogBook::new(app_handle.clone())),
        })
    }
}

/// 单回合最多保留的条目数。
const TURN_LOG_MAX_ENTRIES: usize = 200;
/// 单条 `detail` 序列化后的字节上限。
const TURN_LOG_MAX_DETAIL_BYTES: usize = 8 * 1024;
/// 整份 `entries_json` 的字节上限。
const TURN_LOG_MAX_JSON_BYTES: usize = 512 * 1024;

/// 回合日志的内存缓冲。`emit` 为 false 时只落库、不推实时事件。
struct TurnLogBuffer {
    session_id: String,
    question: String,
    path: String,
    model_id: Option<String>,
    persona_name: Option<String>,
    /// 该回合关联的待确认计划（用于 `cancel_plan` 定位回合）。
    plan_id: Option<String>,
    started_at: DateTime<Utc>,
    emit: bool,
    entries: Vec<crate::core::db::models::TurnLogEntry>,
}

/// 回合日志收集器：`begin` 建缓冲，`push` 追加并推事件，`finish` 落库并清理。
///
/// 用 `std::sync::Mutex` 而不是 tokio 锁：临界区只做内存操作，绝不能跨
/// `.await` 持锁；`finish` 先取出并克隆条目，释放锁后才 await 写库。
pub struct TurnLogBook {
    /// 为 `None` 时（单测）不推事件。
    app: Option<AppHandle>,
    buffers: StdMutex<HashMap<String, TurnLogBuffer>>,
}

impl TurnLogBook {
    /// 生产环境构造：实时事件发往前端。
    pub fn new(app: AppHandle) -> Self {
        Self {
            app: Some(app),
            buffers: StdMutex::new(HashMap::new()),
        }
    }

    /// 无事件通道的构造，仅供单测使用。
    #[cfg(test)]
    pub fn headless() -> Self {
        Self {
            app: None,
            buffers: StdMutex::new(HashMap::new()),
        }
    }

    /// 开始（或复用）一个回合，返回 `turn_id`。
    ///
    /// 同一 `turn_id` 已存在时直接复用，不覆盖已有条目：前端在
    /// `decompose_message` → 确认 → `send_message_stream` 之间复用同一个 id。
    /// 缺省时后端生成 UUID。`emit` 只在新建缓冲时生效。
    pub fn begin(
        &self,
        session_id: &str,
        turn_id: Option<&str>,
        question: &str,
        emit: bool,
    ) -> String {
        let mut buffers = self
            .buffers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if let Some(id) = turn_id {
            if buffers.contains_key(id) {
                return id.to_string();
            }
        }

        let id = turn_id
            .map(str::to_string)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        buffers.insert(
            id.clone(),
            TurnLogBuffer {
                session_id: session_id.to_string(),
                question: truncate_chars(question, 200),
                path: "unknown".to_string(),
                model_id: None,
                persona_name: None,
                plan_id: None,
                started_at: Utc::now(),
                emit,
                entries: Vec::new(),
            },
        );
        drop(buffers);

        // 回合的第一条永远是「收到提问」，完整正文只进 detail。
        self.push(
            &id,
            "question",
            "info",
            "收到提问",
            &truncate_chars(question, 60),
            Some(json!(question)),
        );
        id
    }

    /// 追加一条日志；`detail` 会按 8KB 上限截断。
    ///
    /// 实时事件里的 `detail` 一律置空：前端展开明细时再按 `turn_id` 拉完整记录。
    pub fn push(
        &self,
        turn_id: &str,
        stage: &str,
        level: &str,
        title: &str,
        summary: &str,
        detail: Option<Value>,
    ) {
        let (payload, emit) = {
            let mut buffers = self
                .buffers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(buffer) = buffers.get_mut(turn_id) else {
                return;
            };
            // 超过条数上限后直接丢弃：截断提示会在 finish 时补一条。
            if buffer.entries.len() >= TURN_LOG_MAX_ENTRIES {
                return;
            }

            let now = Utc::now();
            let entry = crate::core::db::models::TurnLogEntry {
                seq: buffer.entries.len() as u32 + 1,
                at: now.to_rfc3339(),
                stage: stage.to_string(),
                level: level.to_string(),
                title: title.to_string(),
                summary: summary.to_string(),
                elapsed_ms: Some(
                    (now - buffer.started_at).num_milliseconds().max(0) as u64,
                ),
                detail: sanitize_detail(detail),
            };
            let mut event_entry = entry.clone();
            event_entry.detail = None;
            buffer.entries.push(entry);

            (
                json!({
                    "session_id": buffer.session_id,
                    "turn_id": turn_id,
                    "entry": event_entry,
                }),
                buffer.emit,
            )
        };

        if emit {
            if let Some(app) = &self.app {
                let _ = app.emit("session_log", payload);
            }
        }
    }

    /// 记录本次回合的模型与人设，供摘要展示。
    pub fn set_meta(&self, turn_id: &str, model_id: Option<&str>, persona: Option<&str>) {
        let mut buffers = self
            .buffers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(buffer) = buffers.get_mut(turn_id) {
            if let Some(model_id) = model_id {
                buffer.model_id = Some(model_id.to_string());
            }
            if let Some(persona) = persona {
                buffer.persona_name = Some(persona.to_string());
            }
        }
    }

    /// 记录本回合的执行路径（fast/deep/planned/autonomous）。
    pub fn set_path(&self, turn_id: &str, path: &str) {
        let mut buffers = self
            .buffers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(buffer) = buffers.get_mut(turn_id) {
            buffer.path = path.to_string();
        }
    }

    /// 把待确认计划绑定到回合，`cancel_plan` 据此找到要关闭的回合。
    pub fn bind_plan(&self, turn_id: &str, plan_id: &str) {
        let mut buffers = self
            .buffers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(buffer) = buffers.get_mut(turn_id) {
            buffer.plan_id = Some(plan_id.to_string());
        }
    }

    /// 按会话查找进行中的回合（同一会话同时只应有一个）。
    pub fn turn_for_session(&self, session_id: &str) -> Option<String> {
        let buffers = self
            .buffers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        buffers
            .iter()
            .find(|(_, buffer)| buffer.session_id == session_id)
            .map(|(id, _)| id.clone())
    }

    /// 按计划查找关联回合。
    pub fn turn_for_plan(&self, plan_id: &str) -> Option<String> {
        let buffers = self
            .buffers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        buffers
            .iter()
            .find(|(_, buffer)| buffer.plan_id.as_deref() == Some(plan_id))
            .map(|(id, _)| id.clone())
    }

    /// 结束回合：截断超限内容、写库、推 `session_log_done`、移除缓冲。
    ///
    /// 重复调用幂等：第二次找不到缓冲即静默返回 `false`。写库失败只记
    /// `warn`，不影响回答返回。
    pub async fn finish(
        &self,
        db: &Arc<Mutex<crate::core::db::repository::Database>>,
        turn_id: &str,
        status: &str,
        path: &str,
    ) -> bool {
        let record = {
            let mut buffers = self
                .buffers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(mut buffer) = buffers.remove(turn_id) else {
                return false;
            };

            enforce_limits(&mut buffer.entries);
            let finished_at = Utc::now();
            let path = if path.is_empty() || path == "unknown" {
                buffer.path.clone()
            } else {
                path.to_string()
            };

            (
                crate::core::db::models::TurnLogRecord {
                    turn_id: turn_id.to_string(),
                    session_id: buffer.session_id.clone(),
                    status: status.to_string(),
                    path,
                    question: buffer.question,
                    model_id: buffer.model_id,
                    persona_name: buffer.persona_name,
                    started_at: buffer.started_at.to_rfc3339(),
                    finished_at: Some(finished_at.to_rfc3339()),
                    total_ms: Some(
                        (finished_at - buffer.started_at).num_milliseconds().max(0) as u64,
                    ),
                    entry_count: buffer.entries.len() as i64,
                    entries: buffer.entries,
                },
                buffer.emit,
            )
        };

        let (record, emit) = record;

        if emit {
            if let Some(app) = &self.app {
                let _ = app.emit(
                    "session_log_done",
                    json!({
                        "session_id": record.session_id,
                        "turn_id": record.turn_id,
                        "status": record.status,
                    }),
                );
            }
        }

        let db = db.lock().await;
        if let Err(e) = db.insert_turn_log(&record) {
            log::warn!("failed to persist turn log {}: {}", record.turn_id, e);
        }
        true
    }
}

/// 字符级截断（避免切断 UTF-8）。
fn truncate_chars(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    text.chars().take(limit).collect()
}

/// 单条 `detail` 超过 8KB 时替换为截断说明，保留前 2000 字预览。
fn sanitize_detail(detail: Option<Value>) -> Option<Value> {
    let value = detail?;
    let size = serde_json::to_vec(&value).map(|v| v.len()).unwrap_or(0);
    if size <= TURN_LOG_MAX_DETAIL_BYTES {
        return Some(value);
    }
    let preview = match &value {
        Value::String(text) => truncate_chars(text, 2000),
        other => truncate_chars(&other.to_string(), 2000),
    };
    Some(json!({
        "truncated": true,
        "bytes": size,
        "preview": preview,
    }))
}

/// 截断提示条目：`seq` 为 0，表示不是真实的执行步骤。
fn truncation_notice() -> crate::core::db::models::TurnLogEntry {
    crate::core::db::models::TurnLogEntry {
        seq: 0,
        at: Utc::now().to_rfc3339(),
        stage: "notice".to_string(),
        level: "warn".to_string(),
        title: "日志已截断".to_string(),
        summary: "内容过多，只保留最近的记录".to_string(),
        elapsed_ms: None,
        detail: None,
    }
}

/// 施加条数与体积上限：超限丢最旧的条目，并追加一条 notice。
fn enforce_limits(entries: &mut Vec<crate::core::db::models::TurnLogEntry>) {
    let mut truncated = false;

    if entries.len() > TURN_LOG_MAX_ENTRIES {
        entries.truncate(TURN_LOG_MAX_ENTRIES - 1);
        truncated = true;
    }

    loop {
        let size = serde_json::to_vec(entries).map(|v| v.len()).unwrap_or(0);
        if size <= TURN_LOG_MAX_JSON_BYTES || entries.is_empty() {
            break;
        }
        let drop_n = (entries.len() / 4).max(1);
        entries.drain(..drop_n);
        truncated = true;
    }

    if truncated {
        entries.push(truncation_notice());
        while serde_json::to_vec(entries).map(|v| v.len()).unwrap_or(0) > TURN_LOG_MAX_JSON_BYTES
            && entries.len() > 1
        {
            entries.remove(0);
        }
    }
}

/// MCP 按需连接解析器。
///
/// 当执行器遇到尚未注册的工具时，按 `<server>__<tool>` 前缀找到声明了
/// `launch_on_demand` 的服务器并连接；连接成功后工具会被注册进注册表。
///
/// 连接流程内部会锁注册表，因此本实现必须只在**未持有注册表锁**时被调用
/// （见 `ToolResolver` 的文档）。
struct McpOnDemandResolver {
    config: Arc<Mutex<AppConfig>>,
    mcp: Arc<McpServerManager>,
    tools: Arc<Mutex<ToolRegistry>>,
}

#[async_trait::async_trait]
impl ToolResolver for McpOnDemandResolver {
    async fn resolve(&self, name: &str) -> bool {
        // 可能刚被其他调用注册好，先查一次，避免重复连接。
        {
            let tools = self.tools.lock().await;
            if tools.canonical_name(name).is_some() {
                return true;
            }
        }

        // 复制配置后立即释放配置锁：连接可能耗时，不能长时间占住配置。
        let config = {
            let guard = self.config.lock().await;
            on_demand_config(&guard.mcp_servers, name).cloned()
        };
        let Some(config) = config else {
            return false;
        };

        // 已连接但工具仍未注册，说明该服务器没有这个工具，重连也没有意义。
        if self.mcp.is_connected(&config.id).await {
            return false;
        }

        match self.mcp.connect_if_absent(&config).await {
            Ok(()) => {
                let tools = self.tools.lock().await;
                tools.canonical_name(name).is_some()
            }
            Err(e) => {
                log::warn!(
                    "On-demand MCP connect failed for '{}': {}",
                    config.name,
                    e
                );
                false
            }
        }
    }
}

/// 资源使用反馈接收端：把运行时上报的样本写入意图路由器。
///
/// 运行时是同步上下文，路由器用 tokio 锁，因此转发到异步任务执行。
struct RouterUsageSink {
    router: Arc<IntentRouter>,
}

impl crate::agent::orchestrator::runtime::ScopeUsageSink for RouterUsageSink {
    fn record_usage(&self, scoped: Vec<String>, used: Vec<String>) {
        let router = self.router.clone();
        tauri::async_runtime::spawn(async move {
            router.record_usage(scoped, used).await;
        });
    }
}

/// 从钥匙串补全每个模型的 API key。
///
/// `resolve_api_key` 优先使用配置里的明文 key（keychain 不可用时的回落），
/// 为空时才查操作系统钥匙串。注册表与 embedding 都必须使用这份副本。
fn resolved_models(models: &[ModelConfig]) -> Vec<ModelConfig> {
    models
        .iter()
        .cloned()
        .map(|mut model| {
            model.api_key = crate::core::keychain::resolve_api_key(&model.id, &model.api_key);
            model
        })
        .collect()
}

/// 选取 embedding 使用的 OpenAI key；没有带 key 的 OpenAI 模型时返回 `None`。
///
/// 不要求模型处于启用状态：`enabled` 只决定它能否用于聊天，key 本身仍可用于
/// embedding，保持改动前的选型语义。
fn embedding_api_key(models: &[ModelConfig]) -> Option<String> {
    models
        .iter()
        .find(|model| model.provider == ModelProvider::OpenAI && !model.api_key.is_empty())
        .map(|model| model.api_key.clone())
}

/// 构造共享的 embedding 客户端。
///
/// 优先用配置里第一个启用且带 key 的 OpenAI 模型，其次读环境变量。都没有时
/// 仍然返回一个客户端：记忆召回与意图召回会优雅降级（跳过向量路径），而不是
/// 让整个应用起不来。
fn build_embedding_service(
    models: &[ModelConfig],
    model_id: Option<&str>,
) -> Arc<EmbeddingService> {
    let openai_key = embedding_api_key(models).or_else(|| std::env::var("OPENAI_API_KEY").ok());
    Arc::new(EmbeddingService::new(openai_key, model_id))
}

/// 构造意图路由器，并异步预热内存索引。
fn build_intent_router(
    intent: &crate::app::config::IntentAppConfig,
    db: Arc<Mutex<Database>>,
    providers: Arc<Mutex<ProviderRegistry>>,
    embedding: Arc<EmbeddingService>,
    builtin_tools: Vec<crate::agent::tools::base::ToolInfo>,
) -> Arc<IntentRouter> {
    let router_config = RouterConfig {
        enabled: intent.enabled,
        llm_rerank_enabled: intent.llm_rerank_enabled,
        llm_timeout_ms: intent.llm_timeout_ms,
        probe_enabled: intent.probe_enabled,
        probe_timeout_ms: intent.probe_timeout_ms,
        routing_budget_ms: intent.routing_budget_ms,
    };
    let reranker = Arc::new(LlmReranker::new(
        providers,
        intent.classifier_model_id.clone(),
    ));
    let router = Arc::new(IntentRouter::new(
        db,
        Some(embedding),
        reranker,
        router_config,
    ));

    // 启动时先用内置工具装配资源目录；MCP 连接/断开后由 manager 再刷新。
    router.set_catalog(ResourceCatalog::from_tools(
        builtin_tools
            .into_iter()
            .map(|info| {
                CatalogTool::new(info.name, info.source)
                    .with_enabled(info.enabled)
                    .with_cost(crate::llm::prompt::composer::estimate_tokens(&info.description))
            }),
    ));

    let warm_router = router.clone();
    tauri::async_runtime::spawn(async move {
        warm_router.warm_up().await;
    });

    router
}

/// 构造编排器并注册全部 worker。
///
/// 新增 worker 必须在这里注册，否则 planner 生成的计划会在调度阶段报
/// 「未知 agent」。
fn build_orchestrator(
    providers: Arc<Mutex<ProviderRegistry>>,
    db: Arc<Mutex<Database>>,
    tools: Arc<Mutex<ToolRegistry>>,
    resolver: Arc<dyn ToolResolver>,
) -> OrchestratorAgent {
    let mut dispatcher = Dispatcher::new();
    dispatcher.register(Box::new(ThinkerWorker::new(providers.clone())));
    dispatcher.register(Box::new(CodeExplorerWorker::new(providers.clone())));
    dispatcher.register(Box::new(CodeEditorWorker::new(providers.clone())));
    dispatcher.register(Box::new(ShellWorker::new(providers.clone())));
    dispatcher.register(Box::new(WebWorker));
    dispatcher.register(Box::new(MemoryWorker::new(db)));
    dispatcher.register(Box::new(
        MCPBridgeWorker::new(providers.clone(), tools).with_resolver(resolver),
    ));

    let critic = Arc::new(CriticAgent::new(providers));
    OrchestratorAgent::new(dispatcher, critic)
}

#[cfg(test)]
mod tests {
    use super::{embedding_api_key, resolved_models};
    use crate::llm::model::{ModelConfig, ModelProvider};

    fn model(id: &str, provider: ModelProvider, api_key: &str, enabled: bool) -> ModelConfig {
        ModelConfig {
            id: id.to_string(),
            name: id.to_string(),
            display_name: id.to_string(),
            provider,
            api_key: api_key.to_string(),
            base_url: None,
            is_default: false,
            enabled,
            context_window: None,
            max_tokens: None,
        }
    }

    #[test]
    fn plaintext_key_survives_resolution() {
        let models = resolved_models(&[model("m", ModelProvider::DeepSeek, "plain", true)]);

        assert_eq!(models[0].api_key, "plain");
    }

    #[test]
    fn embedding_key_requires_an_openai_model_with_a_key() {
        assert_eq!(
            embedding_api_key(&[model("o", ModelProvider::OpenAI, "key", true)]).as_deref(),
            Some("key")
        );
        assert_eq!(
            embedding_api_key(&[model("o", ModelProvider::OpenAI, "key", false)]).as_deref(),
            Some("key"),
            "停用只影响聊天，key 仍可用于 embedding"
        );
        assert_eq!(
            embedding_api_key(&[model("d", ModelProvider::DeepSeek, "key", true)]),
            None
        );
        assert_eq!(
            embedding_api_key(&[model("o", ModelProvider::OpenAI, "", true)]),
            None
        );
    }

    // ── 回合日志 ──

    use super::{
        enforce_limits, sanitize_detail, TurnLogBook, TURN_LOG_MAX_ENTRIES,
        TURN_LOG_MAX_JSON_BYTES,
    };
    use crate::core::db::models::TurnLogEntry;
    use std::sync::Arc;

    fn entry(seq: u32, detail: Option<serde_json::Value>) -> TurnLogEntry {
        TurnLogEntry {
            seq,
            at: "2026-01-01T00:00:00Z".to_string(),
            stage: "execute".to_string(),
            level: "info".to_string(),
            title: "调用工具".to_string(),
            summary: "成功".to_string(),
            elapsed_ms: Some(10),
            detail,
        }
    }

    #[test]
    fn enforce_limits_caps_entry_count_and_appends_notice() {
        let mut entries: Vec<TurnLogEntry> = (1..=250).map(|seq| entry(seq, None)).collect();
        enforce_limits(&mut entries);

        assert!(entries.len() <= TURN_LOG_MAX_ENTRIES, "条数必须收敛到上限");
        let last = entries.last().expect("至少保留一条");
        assert_eq!(last.stage, "notice");
        assert_eq!(last.title, "日志已截断");
    }

    #[test]
    fn enforce_limits_caps_serialized_size() {
        let big = serde_json::json!({ "text": "x".repeat(20_000) });
        let mut entries: Vec<TurnLogEntry> = (1..=100).map(|seq| entry(seq, Some(big.clone()))).collect();
        enforce_limits(&mut entries);

        let size = serde_json::to_vec(&entries).map(|v| v.len()).unwrap_or(0);
        assert!(size <= TURN_LOG_MAX_JSON_BYTES, "整份 entries_json 必须 ≤512KB");
        assert_eq!(entries.last().expect("至少保留一条").stage, "notice");
    }

    #[test]
    fn sanitize_detail_truncates_oversized_payload() {
        let detail = sanitize_detail(Some(serde_json::json!({ "text": "x".repeat(20_000) })))
            .expect("仍应保留截断说明");
        assert_eq!(detail["truncated"], serde_json::json!(true));
    }

    #[tokio::test]
    async fn finish_is_idempotent_and_persists_once() {
        let book = TurnLogBook::headless();
        let db = Arc::new(tokio::sync::Mutex::new(
            crate::core::db::repository::Database::new_test().unwrap(),
        ));

        let turn_id = book.begin("s1", Some("t1"), "你好", false);
        book.push(&turn_id, "answer", "info", "输出回答", "2 字", None);

        assert!(book.finish(&db, &turn_id, "completed", "fast").await);
        assert!(!book.finish(&db, &turn_id, "completed", "fast").await, "重复 finish 必须静默返回");

        let record = db
            .lock()
            .await
            .get_turn_log("t1")
            .unwrap()
            .expect("必须写库");
        assert_eq!(record.status, "completed");
        assert_eq!(record.entry_count, 2, "含 question 与 answer 两条");
    }

    #[tokio::test]
    async fn begin_reuses_existing_turn() {
        let book = TurnLogBook::headless();
        let first = book.begin("s1", Some("t1"), "你好", false);
        let second = book.begin("s1", Some("t1"), "你好", false);
        assert_eq!(first, second);

        assert_eq!(book.turn_for_session("s1").as_deref(), Some("t1"));
    }
}
