pub mod router;

use serde::{Deserialize, Serialize};

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
    #[serde(default)]
    pub reclassify_triggers: Vec<ReclassifyTrigger>,
    /// If true, this intent triggers autonomous session mode (Phase 2+).
    #[serde(default)]
    pub auto_escalate: bool,
}

/// A rule that triggers reclassification when tool result content matches.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReclassifyTrigger {
    pub from: String,
    pub pattern: String,
    #[serde(rename = "to")]
    pub to_intent: String,
}

/// Result of classification.
#[derive(Debug, Clone)]
pub struct IntentResult {
    pub name: String,
    pub config: IntentConfig,
    pub matched_rule: Option<String>,
}

/// A single classification rule loaded from config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentRule {
    pub pattern: String,
    pub intent: String,
    #[serde(default = "default_priority")]
    pub priority: i32,
}

fn default_priority() -> i32 {
    10
}

/// Top-level config section for intent routing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentRouterConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub classifier_model_id: Option<String>,
    #[serde(default = "default_rules")]
    pub rules: Vec<IntentRule>,
    #[serde(default)]
    pub intents: std::collections::HashMap<String, IntentConfig>,
}

fn default_rules() -> Vec<IntentRule> {
    vec![
        IntentRule {
            pattern: r"(?i)\b(code|implement|write|refactor|fix|debug|compile|test|function|class|api|endpoint)\b".to_string(),
            intent: "code".to_string(),
            priority: 10,
        },
        IntentRule {
            pattern: r"(?i)\b(search|research|find|look up|investigate|what is|how does|explain|compare)\b".to_string(),
            intent: "research".to_string(),
            priority: 10,
        },
    ]
}

impl Default for IntentRouterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            classifier_model_id: None,
            rules: default_rules(),
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
            reclassify_triggers: vec![ReclassifyTrigger {
                from: "code".to_string(),
                pattern: r"search results|found|according to|citation".to_string(),
                to_intent: "research".to_string(),
            }],
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
            reclassify_triggers: vec![ReclassifyTrigger {
                from: "research".to_string(),
                pattern: r"```|fn |function |class |def |impl ".to_string(),
                to_intent: "code".to_string(),
            }],
            auto_escalate: true,
        },
    );
    m
}
