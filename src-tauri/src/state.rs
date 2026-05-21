use std::sync::Arc;
use tauri::AppHandle;
use tokio::sync::Mutex;

use crate::api::provider::ProviderRegistry;
use crate::config::AppConfig;
use crate::db::repository::Database;
use crate::mcp::McpServerManager;
use crate::skills::SkillManager;
use crate::tools::registry::ToolRegistry;

pub struct AppState {
    pub app_handle: AppHandle,
    pub db: Arc<Mutex<Database>>,
    pub config: Arc<Mutex<AppConfig>>,
    pub providers: Arc<Mutex<ProviderRegistry>>,
    pub tools: Arc<Mutex<ToolRegistry>>,
    pub skills: Arc<Mutex<SkillManager>>,
    pub mcp: McpServerManager,
}

impl AppState {
    pub fn new(app_handle: &AppHandle) -> crate::error::Result<Self> {
        let db = Database::new()?;
        let config = AppConfig::load()?;
        let providers = ProviderRegistry::new(&config);
        let tools = ToolRegistry::new();

        let db_arc = Arc::new(Mutex::new(db));
        let tools_arc = Arc::new(Mutex::new(tools));

        let skills = SkillManager::new(db_arc.clone(), tools_arc.clone());
        let mcp = McpServerManager::new(tools_arc.clone());

        Ok(Self {
            app_handle: app_handle.clone(),
            db: db_arc,
            config: Arc::new(Mutex::new(config)),
            providers: Arc::new(Mutex::new(providers)),
            tools: tools_arc,
            skills: Arc::new(Mutex::new(skills)),
            mcp,
        })
    }
}
