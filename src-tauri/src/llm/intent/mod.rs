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

/// Which stage of the routing cascade produced the decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteSource {
    /// The LLM classifier named the intent.
    Llm,
    /// The data-driven example/bigram scorer named the intent.
    Scorer,
    /// The in-domain fallback chain resolved the intent.
    Fallback,
}

/// One routing request: the message plus the session's domain prior.
#[derive(Debug, Clone, Copy)]
pub struct RouteRequest<'a> {
    pub content: &'a str,
    /// The session's previous domain. `None` means there is no prior.
    pub prior_domain: Option<&'a str>,
    /// When pinned, the router never widens beyond the prior domain.
    pub pinned: bool,
}

/// A completed routing decision, including why it was made.
#[derive(Debug, Clone)]
pub struct RouteOutcome {
    pub intent: IntentResult,
    pub source: RouteSource,
    /// True when the router searched beyond the prior domain.
    pub widened: bool,
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
    /// `builtin.software_dev` hit (which carries a non-empty name). Pair this
    /// with [`RouteOutcome::source`] when the request must be reported.
    pub domain_name: String,
    pub config: IntentConfig,
    pub path: PathSelection,
    /// Classification score of the detected intent. After a confidence-based
    /// reroute this still carries the pre-reroute score (that is why the
    /// reroute happened), not the fallback intent's own score. `0.0` for the
    /// plain-chat fallback and for "nothing matched" routings.
    pub confidence: f32,
}
