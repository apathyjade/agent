use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::core::error::{AppError, Result};
use crate::llm::model::{ModelConfig, ModelProvider};
use crate::features::mcp::config::McpServerConfig;

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
    /// Application-level intent routing switches. Per-intent configuration
    /// lives in the `intents` table, not here.
    #[serde(default)]
    pub intent: IntentAppConfig,
}

/// Application-level intent routing switches.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentAppConfig {
    /// Master switch: when false every message is handled as plain chat.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Model used for classification; `None` uses the default model.
    #[serde(default)]
    pub classifier_model_id: Option<String>,
    /// Embedding model used for intent recall. `None` means
    /// `text-embedding-3-small`.
    #[serde(default)]
    pub embedding_model_id: Option<String>,
    /// Minimum raw cosine score for an embedding decision to be treated as
    /// certain. Calibrated by the routing eval set.
    #[serde(default = "default_embedding_floor")]
    pub embedding_floor: f32,
    /// Minimum gap between top1 and top2 raw cosine scores.
    #[serde(default = "default_embedding_margin")]
    pub embedding_margin: f32,
    /// Ranking-only boost for intents in the session's prior domain.
    #[serde(default = "default_prior_boost")]
    pub prior_boost: f32,
    /// Whether the LLM rerank step may run when embedding is not decisive.
    #[serde(default = "default_true")]
    pub llm_rerank_enabled: bool,
    #[serde(default = "default_llm_timeout_ms")]
    pub llm_timeout_ms: u64,
    /// Whether the read-only probe may run after a low-confidence LLM result.
    #[serde(default = "default_true")]
    pub probe_enabled: bool,
    #[serde(default = "default_probe_timeout_ms")]
    pub probe_timeout_ms: u64,
    /// Minimum remaining budget required to start the probe plus its rerank.
    #[serde(default = "default_second_rerank_min_budget_ms")]
    pub second_rerank_min_budget_ms: u64,
    /// Hard wall-clock budget for one routing decision.
    #[serde(default = "default_routing_budget_ms")]
    pub routing_budget_ms: u64,
}

fn default_embedding_floor() -> f32 {
    0.55
}

fn default_embedding_margin() -> f32 {
    0.05
}

fn default_prior_boost() -> f32 {
    0.04
}

fn default_llm_timeout_ms() -> u64 {
    2500
}

fn default_probe_timeout_ms() -> u64 {
    1500
}

fn default_second_rerank_min_budget_ms() -> u64 {
    1200
}

fn default_routing_budget_ms() -> u64 {
    6000
}

impl Default for IntentAppConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            classifier_model_id: None,
            embedding_model_id: None,
            embedding_floor: default_embedding_floor(),
            embedding_margin: default_embedding_margin(),
            prior_boost: default_prior_boost(),
            llm_rerank_enabled: true,
            llm_timeout_ms: default_llm_timeout_ms(),
            probe_enabled: true,
            probe_timeout_ms: default_probe_timeout_ms(),
            second_rerank_min_budget_ms: default_second_rerank_min_budget_ms(),
            routing_budget_ms: default_routing_budget_ms(),
        }
    }
}

fn default_true() -> bool {
    true
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
            intent: IntentAppConfig::default(),
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

#[cfg(test)]
mod tests {
    use super::AppConfig;

    #[test]
    fn old_config_json_with_intent_routing_still_loads() {
        // `intent_routing` no longer exists; `AppConfig` has no
        // `deny_unknown_fields`, so the key must be ignored rather than
        // turning a user's config into a startup error.
        let json = r#"{
            "models": [],
            "enabled_tools": [],
            "intent_routing": {
                "enabled": true,
                "classifier_model_id": null,
                "intents": { "code": { "auto_escalate": true } }
            }
        }"#;

        let config: AppConfig = serde_json::from_str(json).expect("旧配置必须仍然能加载");

        assert!(config.intent.enabled, "缺失的 intent 段必须默认开启");
        assert_eq!(config.intent.classifier_model_id, None);
    }

    #[test]
    fn empty_intent_section_defaults_to_enabled() {
        let json = r#"{
            "models": [],
            "enabled_tools": [],
            "intent": {}
        }"#;

        let config: AppConfig = serde_json::from_str(json).expect("intent 段缺省字段必须可加载");

        assert!(config.intent.enabled);
        assert_eq!(config.intent.classifier_model_id, None);
    }

    #[test]
    fn empty_intent_section_uses_routing_defaults() {
        let json = r#"{
            "models": [],
            "enabled_tools": [],
            "intent": {}
        }"#;

        let config: AppConfig = serde_json::from_str(json).expect("intent 段缺省字段必须可加载");
        let intent = config.intent;

        assert_eq!(intent.embedding_model_id, None);
        assert!((intent.embedding_floor - 0.55).abs() < f32::EPSILON);
        assert!((intent.embedding_margin - 0.05).abs() < f32::EPSILON);
        assert!((intent.prior_boost - 0.04).abs() < f32::EPSILON);
        assert!(intent.llm_rerank_enabled);
        assert_eq!(intent.llm_timeout_ms, 2500);
        assert!(intent.probe_enabled);
        assert_eq!(intent.probe_timeout_ms, 1500);
        assert_eq!(intent.second_rerank_min_budget_ms, 1200);
        assert_eq!(intent.routing_budget_ms, 6000);
    }
}
