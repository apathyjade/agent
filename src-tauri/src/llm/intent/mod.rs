pub mod classifier;
pub mod router;

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

/// Result of the full routing decision (intent row projected for the caller).
#[derive(Debug, Clone)]
pub struct IntentResult {
    /// Stable intent id (`builtin.software_dev.<slug>` or a uuid).
    pub id: String,
    /// Human-readable display name from the intent row.
    pub name: String,
    pub config: IntentConfig,
    pub path: PathSelection,
    /// Score that produced this decision, `0.0` for the plain-chat fallback.
    pub confidence: f32,
}
