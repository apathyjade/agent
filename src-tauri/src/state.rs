use std::collections::HashMap;
use std::sync::Arc;
use tauri::AppHandle;
use tokio::sync::Mutex;

use crate::config::AppConfig;
use crate::agent::critic::agent::CriticAgent;
use crate::core::db::repository::Database;
use crate::features::session::lifecycle::LifecycleManager;
use crate::llm::intent::router::IntentRouter;
use crate::llm::model::ModelProvider;
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

pub struct AppState {
    pub app_handle: AppHandle,
    pub db: Arc<Mutex<Database>>,
    pub config: Arc<Mutex<AppConfig>>,
    pub providers: Arc<Mutex<ProviderRegistry>>,
    pub tools: Arc<Mutex<ToolRegistry>>,
    pub skills: SkillsService,
    pub memory: MemoryManager,
    pub persona: PersonaManager,
    pub mcp: McpServerManager,
    pub toolchain: RuntimeServices,
    pub lifecycle: LifecycleManager,
    pub intent_router: Arc<IntentRouter>,
    pub orchestrator: Arc<OrchestratorAgent>,
    /// 运行中的执行句柄（session_id → ExecutionHandle）
    pub active_executions: Arc<Mutex<HashMap<String, ExecutionHandle>>>,
}

impl AppState {
    pub fn new(app_handle: &AppHandle) -> crate::core::error::Result<Self> {
        let db = Database::new()?;
        let config = AppConfig::load()?;
        let providers = ProviderRegistry::new(&config.models);
        let providers_arc = Arc::new(Mutex::new(providers));
        let tools = ToolRegistry::new();

        let db_arc = Arc::new(Mutex::new(db));
        let tools_arc = Arc::new(Mutex::new(tools));

        let skills = SkillsService::new();
        // Pass the first available OpenAI API key for memory embedding.
        // Embedding is best-effort — if no key is available the memory
        // manager falls back to keyword search.
        let openai_key = config
            .models
            .iter()
            .find(|m| m.provider == ModelProvider::OpenAI && !m.api_key.is_empty())
            .map(|m| m.api_key.clone())
            .or_else(|| std::env::var("OPENAI_API_KEY").ok());
        let memory = MemoryManager::new(db_arc.clone(), openai_key);
        let persona = PersonaManager::new(db_arc.clone());
        let lifecycle = LifecycleManager::new(db_arc.clone(), providers_arc.clone());

        let toolchain = RuntimeServices::new(config.runtime_options())?;

        let mcp = McpServerManager::new(tools_arc.clone())
            .with_runtime_manager(toolchain.manager.clone());

        let intent_router = Arc::new(IntentRouter::new(&config.intent_routing));

        // Build OrchestratorAgent with registered workers
        let mut dispatcher = Dispatcher::new();
        dispatcher.register(Box::new(ThinkerWorker::new(providers_arc.clone())));
        dispatcher.register(Box::new(CodeExplorerWorker::new(providers_arc.clone())));
        dispatcher.register(Box::new(CodeEditorWorker::new(providers_arc.clone())));
        dispatcher.register(Box::new(ShellWorker::new(providers_arc.clone())));
        dispatcher.register(Box::new(WebWorker));
        dispatcher.register(Box::new(MemoryWorker::new(db_arc.clone())));
        dispatcher.register(Box::new(MCPBridgeWorker::new(
            providers_arc.clone(),
            tools_arc.clone(),
        )));
        let critic = Arc::new(CriticAgent::new(providers_arc.clone()));
        let orchestrator = Arc::new(OrchestratorAgent::new(dispatcher, critic));

        Ok(Self {
            app_handle: app_handle.clone(),
            db: db_arc,
            config: Arc::new(Mutex::new(config)),
            providers: providers_arc,
            tools: tools_arc,
            skills,
            memory,
            persona,
            mcp,
            toolchain,
            lifecycle,
            intent_router,
            orchestrator,
            active_executions: Arc::new(Mutex::new(HashMap::new())),
        })
    }
}
