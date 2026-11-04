use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::core::error::{AppError, Result};
use crate::llm::intent::IntentRouterConfig;
use crate::llm::model::{ModelConfig, ModelProvider};
use crate::mcp::config::McpServerConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub models: Vec<ModelConfig>,
    pub enabled_tools: Vec<String>,
    #[serde(default)]
    pub mcp_servers: Vec<McpServerConfig>,
    #[serde(default)]
    pub workflow_vars: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub workflow_secrets: std::collections::HashMap<String, String>,
    /// Custom install directory for built-in runtimes.
    /// If None, defaults to <app_data_dir>/agent/runtimes/
    #[serde(default)]
    pub runtime_install_dir: Option<String>,
    /// Optional HTTP proxy URL for downloading runtimes.
    /// e.g. "http://127.0.0.1:7890"
    #[serde(default)]
    pub download_proxy: Option<String>,
    /// Active version manager per runtime type.
    /// Key = runtime_type string (e.g. "node"), Value = manager id (e.g. "fnm").
    #[serde(default)]
    pub active_managers: std::collections::HashMap<String, String>,
    /// Persisted default version per runtime type.
    /// Key = runtime_type string (e.g. "node"), Value = exact version (e.g. "20.18.3").
    #[serde(default)]
    pub runtime_defaults: std::collections::HashMap<String, String>,
    /// Intent routing configuration.
    #[serde(default)]
    pub intent_routing: IntentRouterConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            models: vec![
                ModelConfig {
                    id: "default-openai".to_string(),
                    name: "gpt-5.5".to_string(),
                    display_name: "GPT-5.5".to_string(),
                    provider: ModelProvider::OpenAI,
                    api_key: String::new(),
                    base_url: None,
                    is_default: true,
                    enabled: true,
                    context_window: Some(1_000_000),
                    max_tokens: Some(128_000),
                },
                ModelConfig {
                    id: "default-ollama".to_string(),
                    name: "llama3.3".to_string(),
                    display_name: "Llama 3.3 (Ollama)".to_string(),
                    provider: ModelProvider::Ollama,
                    api_key: String::new(),
                    base_url: None,
                    is_default: false,
                    enabled: false,
                    context_window: Some(131072),
                    max_tokens: Some(32768),
                },
            ],
            enabled_tools: vec![
                "calculator".to_string(),
                "file_system".to_string(),
                "web_search".to_string(),
                "code_executor".to_string(),
            ],
            mcp_servers: vec![],
            workflow_vars: std::collections::HashMap::new(),
            workflow_secrets: std::collections::HashMap::new(),
            runtime_install_dir: None,
            download_proxy: None,
            active_managers: std::collections::HashMap::new(),
            runtime_defaults: std::collections::HashMap::new(),
            intent_routing: IntentRouterConfig::default(),
        }
    }
}

impl AppConfig {
    /// Build the plain settings value consumed by the standalone toolchain.
    pub fn runtime_options(&self) -> crate::runtime::toolchain::RuntimeOptions {
        crate::runtime::toolchain::RuntimeOptions {
            install_dir: self
                .runtime_install_dir
                .as_ref()
                .map(std::path::PathBuf::from),
            download_proxy: self.download_proxy.clone(),
            defaults: self.runtime_defaults.clone(),
        }
    }

    pub fn load() -> Result<Self> {
        let config_path = Self::config_path();
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            let config: AppConfig = serde_json::from_str(&content)
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
            Ok(config)
        } else {
            let config = Self::default();
            config.save()?;
            Ok(config)
        }
    }

    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_path();
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        std::fs::write(&config_path, content)?;
        Ok(())
    }

    fn config_path() -> PathBuf {
        let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("agent");
        path.push("config.json");
        path
    }

    pub fn get_model(&self, id: &str) -> Option<&ModelConfig> {
        self.models.iter().find(|m| m.id == id)
    }

    pub fn get_enabled_models(&self) -> Vec<&ModelConfig> {
        self.models.iter().filter(|m| m.enabled).collect()
    }

    pub fn get_default_model(&self) -> Option<&ModelConfig> {
        crate::llm::model::default_model(&self.models)
    }

    pub fn add_model(&mut self, model: ModelConfig) {
        self.models.push(model);
    }

    pub fn remove_model(&mut self, id: &str) -> bool {
        let len = self.models.len();
        self.models.retain(|m| m.id != id);
        self.models.len() < len
    }

    pub fn update_model(&mut self, id: &str, updates: ModelConfig) -> bool {
        if let Some(model) = self.models.iter_mut().find(|m| m.id == id) {
            *model = updates;
            true
        } else {
            false
        }
    }

    pub fn set_default_model(&mut self, id: &str) -> bool {
        for model in &mut self.models {
            model.is_default = model.id == id;
        }
        self.models.iter().any(|m| m.id == id)
    }
}
