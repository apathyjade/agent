pub mod classifier;
pub mod router;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Path selection: which agent system handles this message.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathSelection {
    /// Use existing agent/loop.rs (fast, simple)
    Fast,
    /// Use OrchestratorAgent (deep reasoning, task decomposition)
    Deep,
}

/// Serializable config for one intent's behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentConfig {
    #[serde(default)]
    pub system_prompt_appendix: Option<String>,
    #[serde(default)]
    pub enabled_tools: Option<Vec<String>>,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub max_iterations: Option<usize>,
    /// If true, this intent triggers autonomous session mode.
    #[serde(default)]
    pub auto_escalate: bool,
}

/// Result from LLM classification.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ClassificationResult {
    pub intent: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default = "default_true")]
    pub auto_escalate: bool,
    #[serde(default = "default_max_iter")]
    pub max_iterations: usize,
}

fn default_true() -> bool { true }
fn default_max_iter() -> usize { 10 }

/// Result of the full routing decision (intent + its config merged).
#[derive(Debug, Clone)]
pub struct IntentResult {
    pub name: String,
    pub config: IntentConfig,
    pub path: PathSelection,
}

impl IntentResult {
    /// Determine path selection based on intent name.
    /// `chat` → Fast, everything else → Deep.
    pub fn path_for_intent(name: &str) -> PathSelection {
        match name {
            "chat" => PathSelection::Fast,
            _ => PathSelection::Deep,
        }
    }
}

/// Top-level config section for intent routing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentRouterConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub classifier_model_id: Option<String>,
    #[serde(default)]
    pub intents: std::collections::HashMap<String, IntentConfig>,
}

impl Default for IntentRouterConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            classifier_model_id: None,
            intents: default_intents(),
        }
    }
}

fn default_intents() -> std::collections::HashMap<String, IntentConfig> {
    let mut m = std::collections::HashMap::new();
    m.insert(
        "code".to_string(),
        IntentConfig {
            system_prompt_appendix: Some(
                "You are a senior engineer focused on writing production-quality code \
                 with proper error handling, testing, and best practices."
                    .to_string(),
            ),
            enabled_tools: Some(vec![
                "code_executor".to_string(),
                "file_system".to_string(),
            ]),
            model_id: None,
            max_iterations: None,
            auto_escalate: true,
        },
    );
    m.insert(
        "research".to_string(),
        IntentConfig {
            system_prompt_appendix: Some(
                "You are a research assistant. Base your answers on search results \
                 and cite sources when possible."
                    .to_string(),
            ),
            enabled_tools: Some(vec!["web_search".to_string()]),
            model_id: None,
            max_iterations: None,
            auto_escalate: true,
        },
    );
    m.insert(
        "auto".to_string(),
        IntentConfig {
            system_prompt_appendix: Some(
                "You are an autonomous agent capable of multi-step planning and execution. \
                 Break down the task, use tools as needed, and complete all steps."
                    .to_string(),
            ),
            enabled_tools: None,
            model_id: None,
            max_iterations: Some(20),
            auto_escalate: true,
        },
    );
    m.insert(
        "chat".to_string(),
        IntentConfig {
            system_prompt_appendix: None,
            enabled_tools: None,
            model_id: None,
            max_iterations: None,
            auto_escalate: false,
        },
    );
    m.insert(
        "deep_think".to_string(),
        IntentConfig {
            system_prompt_appendix: Some(
                "You are a deep reasoning agent. Analyze problems step by step, \
                 explore multiple approaches, and provide thorough, well-reasoned responses."
                    .to_string(),
            ),
            enabled_tools: None,
            model_id: None,
            max_iterations: Some(20),
            auto_escalate: true,
        },
    );
    m
}
