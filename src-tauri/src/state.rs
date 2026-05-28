use std::collections::HashMap;
use std::sync::Arc;
use tauri::AppHandle;
use tokio::sync::Mutex;

use crate::api::provider::ProviderRegistry;
use crate::config::{AppConfig, ModelProvider};
use crate::critic::agent::CriticAgent;
use crate::db::repository::Database;
use crate::environment::alias::AliasManager;
use crate::environment::resolver::VersionResolver;
use crate::environment::RuntimeManager;
use crate::environment::registry::RuntimeRegistry;
use crate::orchestrator::plan_types::ExecutionHandle;
use crate::intent::router::IntentRouter;
use crate::lifecycle::LifecycleManager;
use crate::mcp::McpServerManager;
use crate::memory::MemoryManager;
use crate::orchestrator::agent::OrchestratorAgent;
use crate::orchestrator::dispatcher::Dispatcher;
use crate::persona::PersonaManager;
use crate::skills::SkillManager;
use crate::tools::registry::ToolRegistry;
use crate::workers::code_editor::CodeEditorWorker;
use crate::workers::code_explorer::CodeExplorerWorker;
use crate::workers::mcp_bridge::MCPBridgeWorker;
use crate::workers::memory::MemoryWorker;
use crate::workers::shell::ShellWorker;
use crate::workers::web::WebWorker;
use crate::workers::thinker::ThinkerWorker;

pub struct AppState {
    pub app_handle: AppHandle,
    pub db: Arc<Mutex<Database>>,
    pub config: Arc<Mutex<AppConfig>>,
    pub providers: Arc<Mutex<ProviderRegistry>>,
    pub tools: Arc<Mutex<ToolRegistry>>,
    pub skills: Arc<Mutex<SkillManager>>,
    pub memory: MemoryManager,
    pub persona: PersonaManager,
    pub mcp: McpServerManager,
    pub runtime_manager: Arc<RuntimeManager>,
    pub runtime_registry: Arc<RuntimeRegistry>,
    pub version_resolver: Arc<VersionResolver>,
    pub alias_manager: Arc<AliasManager>,
    pub lifecycle: LifecycleManager,
    pub intent_router: Arc<IntentRouter>,
    pub orchestrator: Arc<OrchestratorAgent>,
    /// 运行中的执行句柄（session_id → ExecutionHandle）
    pub active_executions: Arc<Mutex<HashMap<String, ExecutionHandle>>>,
}

impl AppState {
    pub fn new(app_handle: &AppHandle) -> crate::error::Result<Self> {
        let db = Database::new()?;
        let config = AppConfig::load()?;
        let providers = ProviderRegistry::new(&config);
        let providers_arc = Arc::new(Mutex::new(providers));
        let tools = ToolRegistry::new();

        let db_arc = Arc::new(Mutex::new(db));
        let tools_arc = Arc::new(Mutex::new(tools));

        let skills = SkillManager::new(db_arc.clone(), tools_arc.clone());
        // Pass the first available OpenAI API key for memory embedding.
        // Embedding is best-effort — if no key is available the memory
        // manager falls back to keyword search.
        let openai_key = config.models
            .iter()
            .find(|m| m.provider == ModelProvider::OpenAI && !m.api_key.is_empty())
            .map(|m| m.api_key.clone())
            .or_else(|| std::env::var("OPENAI_API_KEY").ok());
        let memory = MemoryManager::new(db_arc.clone(), openai_key);
        let persona = PersonaManager::new(db_arc.clone());
        let lifecycle = LifecycleManager::new(db_arc.clone(), providers_arc.clone());

        // Runtime manager: stores runtimes at configured path (or default)
        let runtime_dir = match &config.runtime_install_dir {
            Some(path) => std::path::PathBuf::from(path),
            None => dirs::data_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("agent")
                .join("runtimes"),
        };
        // Initialize HTTP client with proxy settings for runtime downloads
        crate::environment::http_client::init_http_client(config.download_proxy.as_deref());

        let runtime_manager = Arc::new(RuntimeManager::new(runtime_dir));
        let runtime_registry = Arc::new(RuntimeRegistry::new());
        let version_resolver = Arc::new(VersionResolver::new(runtime_registry.clone()));
        let alias_manager = Arc::new(AliasManager::new());

        let mcp = McpServerManager::new(tools_arc.clone())
            .with_runtime_manager(runtime_manager.clone());

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
            skills: Arc::new(Mutex::new(skills)),
            memory,
            persona,
            mcp,
            runtime_manager,
            runtime_registry,
            version_resolver,
            alias_manager,
            lifecycle,
            intent_router,
            orchestrator,
            active_executions: Arc::new(Mutex::new(HashMap::new())),
        })
    }
}
