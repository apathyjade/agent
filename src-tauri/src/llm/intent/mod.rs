pub mod classifier;
pub mod index;
pub mod probe;
pub mod router;

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::core::db::models::Message;

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
    /// Domain that owns the selected intent.
    pub domain_id: String,
    /// Display name of [`Self::domain_id`].
    ///
    /// Empty for the synthetic fallback: no domain row was consulted, so
    /// callers can distinguish "no domain was chosen" from a real
    /// `builtin.software_dev` hit (which carries a non-empty name).
    pub domain_name: String,
    pub config: IntentConfig,
    pub path: PathSelection,
    /// Final decision confidence. `0.0` for the plain-chat fallback and for
    /// "nothing matched" routings.
    pub confidence: f32,
}

/// One routing request: the message, recent context, session prior, and the
/// project root the read-only probe may inspect.
#[derive(Debug, Clone, Copy)]
pub struct RouteContext<'a> {
    pub session_id: &'a str,
    pub content: &'a str,
    /// Recent messages, excluding the current one.
    pub recent_messages: &'a [Message],
    /// The session's previous domain. `None` means there is no prior.
    pub prior_domain: Option<&'a str>,
    /// When pinned, candidates are restricted to the prior domain.
    pub pinned: bool,
    /// Project root used by the optional read-only probe.
    pub project_root: Option<&'a Path>,
}

/// Which stage produced the final decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionSource {
    /// Embedding recall named the intent.
    Embedding,
    /// The LLM rerank named the intent.
    Llm,
    /// The data-driven example/bigram scorer named the intent.
    Rule,
    /// The probe-augmented rerank named the intent.
    Probe,
    /// The synthetic plain-chat result.
    Fallback,
}

/// One candidate considered during routing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentCandidate {
    pub intent_id: String,
    pub name: String,
    pub domain_id: String,
    pub score: f32,
    pub signal: DecisionSource,
}

/// Evidence collected while routing, used for observability and tests.
#[derive(Debug, Clone, Default)]
pub struct DecisionEvidence {
    pub embedding_top1: Option<f32>,
    pub embedding_margin: Option<f32>,
    pub llm_confidence: Option<f32>,
    pub rule_score: Option<f32>,
    pub llm_calls: usize,
    pub probe_used: bool,
    pub elapsed_ms: u64,
}

/// A completed routing decision, including why it was made.
#[derive(Debug, Clone)]
pub struct RouteDecision {
    pub chosen: IntentResult,
    pub confidence: f32,
    /// True when the chosen intent did not clear its confidence threshold or
    /// the embedding floor/margin. Execution still proceeds with `chosen`.
    pub uncertain: bool,
    pub source: DecisionSource,
    pub candidates: Vec<IntentCandidate>,
    pub evidence: DecisionEvidence,
}

/// Execution recipe derived from the chosen intent.
#[derive(Debug, Clone)]
pub struct ExecutionPolicy {
    pub path: PathSelection,
    pub allowed_tools: Option<Vec<String>>,
    pub model_id: String,
    pub system_prompt_appendix: Option<String>,
    pub max_iterations: Option<usize>,
}

/// Router configuration snapshotted from `IntentAppConfig` at startup.
#[derive(Debug, Clone)]
pub struct RouterConfig {
    pub enabled: bool,
    pub embedding_floor: f32,
    pub embedding_margin: f32,
    pub prior_boost: f32,
    pub llm_rerank_enabled: bool,
    pub llm_timeout_ms: u64,
    pub probe_enabled: bool,
    pub probe_timeout_ms: u64,
    pub second_rerank_min_budget_ms: u64,
    pub routing_budget_ms: u64,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            embedding_floor: 0.55,
            embedding_margin: 0.05,
            prior_boost: 0.04,
            llm_rerank_enabled: true,
            llm_timeout_ms: 2500,
            probe_enabled: true,
            probe_timeout_ms: 1500,
            second_rerank_min_budget_ms: 1200,
            routing_budget_ms: 6000,
        }
    }
}

/// One routing decision kept in the in-process ring buffer.
#[derive(Debug, Clone, Serialize)]
pub struct RouteTrace {
    pub at: String,
    pub session_id: String,
    pub message_excerpt: String,
    pub candidates: Vec<IntentCandidate>,
    pub chosen_intent_id: String,
    pub chosen_intent_name: String,
    pub confidence: f32,
    pub uncertain: bool,
    pub source: DecisionSource,
    pub elapsed_ms: u64,
    pub llm_calls: usize,
    pub probe_used: bool,
}
