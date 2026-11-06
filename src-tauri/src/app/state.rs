//! 应用全局状态（Tauri managed state）。
//!
//! [`AppState`] 是每个 IPC command 拿到的唯一入口：数据库、配置、provider
//! 注册表、工具注册表、各 feature manager 都在这里集中构造一次，避免各命令
//! 各自 new 出互相看不见的副本。
//!
//! 这里只做依赖装配，不含业务规则。构造顺序有依赖关系，见 [`AppState::new`]。

use std::collections::HashMap;
use std::sync::Arc;
use tauri::AppHandle;
use tokio::sync::Mutex;

use crate::app::config::AppConfig;
use crate::agent::critic::agent::CriticAgent;
use crate::core::db::repository::Database;
use crate::features::session::lifecycle::LifecycleManager;
use crate::llm::embedding::EmbeddingService;
use crate::llm::intent::classifier::LlmReranker;
use crate::llm::intent::router::IntentRouter;
use crate::llm::intent::RouterConfig;
use crate::llm::model::{ModelConfig, ModelProvider};
use crate::llm::provider::ProviderRegistry;
use crate::features::mcp::McpServerManager;
use crate::features::memory::MemoryManager;
use crate::agent::orchestrator::agent::OrchestratorAgent;
use crate::agent::orchestrator::dispatcher::Dispatcher;
use crate::agent::orchestrator::plan_types::ExecutionHandle;
use crate::features::persona::PersonaManager;
use crate::features::skills::SkillsService;
use crate::runtime::toolchain::RuntimeServices;
use crate::agent::tools::registry::ToolRegistry;
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
    pub mcp: McpServerManager,
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
}

impl AppState {
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
        let tools_arc = Arc::new(Mutex::new({
            let mut registry = ToolRegistry::new();
            registry.apply_builtin_enabled(&config.enabled_tools);
            registry.set_gate(approval.clone());
            registry
        }));
        let db_arc = Arc::new(Mutex::new(db));

        let embedding = build_embedding_service(&models, config.intent.embedding_model_id.as_deref());
        let memory = MemoryManager::new(db_arc.clone(), Some(embedding.clone()));

        let intent_router = build_intent_router(
            &config.intent,
            db_arc.clone(),
            providers_arc.clone(),
            embedding,
        );

        let persona = PersonaManager::new(db_arc.clone());
        let lifecycle = LifecycleManager::new(db_arc.clone(), providers_arc.clone());

        let toolchain = RuntimeServices::new(config.runtime_options())?;
        let mcp = McpServerManager::new(tools_arc.clone())
            .with_runtime_manager(toolchain.manager.clone());

        let orchestrator = Arc::new(build_orchestrator(
            providers_arc.clone(),
            db_arc.clone(),
            tools_arc.clone(),
        ));

        Ok(Self {
            app_handle: app_handle.clone(),
            db: db_arc,
            config: Arc::new(Mutex::new(config)),
            providers: providers_arc,
            tools: tools_arc,
            skills: SkillsService::new(),
            memory,
            persona,
            mcp,
            toolchain,
            lifecycle,
            intent_router,
            orchestrator,
            active_executions: Arc::new(Mutex::new(HashMap::new())),
            approval,
        })
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
) -> Arc<IntentRouter> {
    let router_config = RouterConfig {
        enabled: intent.enabled,
        embedding_floor: intent.embedding_floor,
        embedding_margin: intent.embedding_margin,
        prior_boost: intent.prior_boost,
        llm_rerank_enabled: intent.llm_rerank_enabled,
        llm_timeout_ms: intent.llm_timeout_ms,
        probe_enabled: intent.probe_enabled,
        probe_timeout_ms: intent.probe_timeout_ms,
        second_rerank_min_budget_ms: intent.second_rerank_min_budget_ms,
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
) -> OrchestratorAgent {
    let mut dispatcher = Dispatcher::new();
    dispatcher.register(Box::new(ThinkerWorker::new(providers.clone())));
    dispatcher.register(Box::new(CodeExplorerWorker::new(providers.clone())));
    dispatcher.register(Box::new(CodeEditorWorker::new(providers.clone())));
    dispatcher.register(Box::new(ShellWorker::new(providers.clone())));
    dispatcher.register(Box::new(WebWorker));
    dispatcher.register(Box::new(MemoryWorker::new(db)));
    dispatcher.register(Box::new(MCPBridgeWorker::new(providers.clone(), tools)));

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
}
