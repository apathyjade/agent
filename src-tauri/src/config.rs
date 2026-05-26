use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::error::{AppError, Result};
use crate::intent::IntentRouterConfig;
use crate::mcp::config::McpServerConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelProvider {
    OpenAI,
    Anthropic,
    Google,
    Groq,
    DeepSeek,
    Zhipu,
    Moonshot,
    SiliconFlow,
    Ollama,
    LMStudio,
    Custom,
}

impl std::fmt::Display for ModelProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelProvider::OpenAI => write!(f, "openai"),
            ModelProvider::Anthropic => write!(f, "anthropic"),
            ModelProvider::Google => write!(f, "google"),
            ModelProvider::Groq => write!(f, "groq"),
            ModelProvider::DeepSeek => write!(f, "deepseek"),
            ModelProvider::Zhipu => write!(f, "zhipu"),
            ModelProvider::Moonshot => write!(f, "moonshot"),
            ModelProvider::SiliconFlow => write!(f, "siliconflow"),
            ModelProvider::Ollama => write!(f, "ollama"),
            ModelProvider::LMStudio => write!(f, "lmstudio"),
            ModelProvider::Custom => write!(f, "custom"),
        }
    }
}

impl std::str::FromStr for ModelProvider {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "openai" => Ok(ModelProvider::OpenAI),
            "anthropic" => Ok(ModelProvider::Anthropic),
            "google" => Ok(ModelProvider::Google),
            "groq" => Ok(ModelProvider::Groq),
            "deepseek" => Ok(ModelProvider::DeepSeek),
            "zhipu" => Ok(ModelProvider::Zhipu),
            "moonshot" => Ok(ModelProvider::Moonshot),
            "siliconflow" => Ok(ModelProvider::SiliconFlow),
            "ollama" => Ok(ModelProvider::Ollama),
            "lmstudio" => Ok(ModelProvider::LMStudio),
            "custom" => Ok(ModelProvider::Custom),
            _ => Err(format!("Unknown provider: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub provider: ModelProvider,
    pub api_key: String,
    pub base_url: Option<String>,
    pub is_default: bool,
    pub enabled: bool,
    pub context_window: Option<u32>,
    pub max_tokens: Option<u32>,
}

impl ModelConfig {
    pub fn default_base_url(&self) -> String {
        match self.provider {
            ModelProvider::OpenAI => "https://api.openai.com/v1/chat/completions".to_string(),
            ModelProvider::Anthropic => "https://api.anthropic.com/v1/messages".to_string(),
            ModelProvider::Google => "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions".to_string(),
            ModelProvider::Groq => "https://api.groq.com/openai/v1/chat/completions".to_string(),
            ModelProvider::DeepSeek => "https://api.deepseek.com/v1/chat/completions".to_string(),
            ModelProvider::Zhipu => "https://open.bigmodel.cn/api/paas/v4/chat/completions".to_string(),
            ModelProvider::Moonshot => "https://api.moonshot.cn/v1/chat/completions".to_string(),
            ModelProvider::SiliconFlow => "https://api.siliconflow.cn/v1/chat/completions".to_string(),
            ModelProvider::Ollama => "http://localhost:11434/v1/chat/completions".to_string(),
            ModelProvider::LMStudio => "http://localhost:1234/v1/chat/completions".to_string(),
            ModelProvider::Custom => String::new(),
        }
    }

    pub fn effective_base_url(&self) -> String {
        self.base_url.clone().unwrap_or_else(|| self.default_base_url())
    }

    pub fn is_compatible_with_openai_api(&self) -> bool {
        matches!(
            self.provider,
            ModelProvider::OpenAI
                | ModelProvider::Google
                | ModelProvider::Groq
                | ModelProvider::DeepSeek
                | ModelProvider::Zhipu
                | ModelProvider::Moonshot
                | ModelProvider::SiliconFlow
                | ModelProvider::Ollama
                | ModelProvider::LMStudio
                | ModelProvider::Custom
        )
    }
}

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
            intent_routing: IntentRouterConfig::default(),
        }
    }
}

impl AppConfig {
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
        self.models.iter().find(|m| m.is_default && m.enabled)
            .or_else(|| self.models.iter().find(|m| m.enabled))
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
