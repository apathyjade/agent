use serde::{Deserialize, Serialize};

/// A virtual persona — a named identity with its own system prompt, memories, and tool config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonaRecord {
    pub id: String,
    pub name: String,
    pub title: String,
    pub emoji: String,
    pub description: String,
    pub system_prompt: String,
    pub temperature: f64,
    pub response_style: String,
    pub model_provider: String,
    pub model_name: String,
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// A project — a named directory on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    pub created_at: String,
    pub updated_at: String,
}

/// A session — a series of messages with a title, model, and optional system prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub model_id: String,
    pub system_prompt: Option<String>,
    /// Optional persona bound to this session
    pub persona_id: Option<String>,
    /// JSON config for per-session settings (e.g. enabled_tools)
    pub config: Option<String>,
    /// Source of the title: "manual" | "auto_generated"
    pub title_source: String,
    /// Whether this session has been archived
    pub archived: bool,
    pub created_at: String,
    pub updated_at: String,
    /// Session mode: "chat" | "autonomous"
    #[serde(default)]
    pub mode: String,
    /// JSON-serialized ExecStatus
    #[serde(default)]
    pub execution_status: String,
    /// Active execution plan ID (if any)
    #[serde(default)]
    pub active_plan_id: Option<String>,
    /// Optional project this session belongs to
    #[serde(default)]
    pub project_id: Option<String>,
    /// Domain the session last belonged to. In `auto` mode this is only a prior
    /// for routing, not a constraint on which domain a message may use.
    #[serde(default)]
    pub intent_domain_id: Option<String>,
    /// Session domain mode: "auto" | "pinned".
    #[serde(default = "default_intent_domain_mode")]
    pub intent_domain_mode: String,
}

/// Default value of [`Session::intent_domain_mode`], also applied when reading
/// sessions persisted before the field existed.
fn default_intent_domain_mode() -> String {
    "auto".to_string()
}

/// A summary of a range of messages within a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub session_id: String,
    pub message_start_id: String,
    pub message_end_id: String,
    pub summary: String,
    pub key_points: Option<String>,  // JSON array string
    pub original_token_count: i32,
    pub summary_token_count: i32,
    pub model_used: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub tool_calls: Option<String>,
    pub tool_call_id: Option<String>,
    pub tokens: Option<i32>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Setting {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemPrompt {
    pub id: String,
    pub name: String,
    pub content: String,
    pub is_default: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundProjectModel {
    pub id: String,
    pub path: String,
    pub name: String,
    pub auto_sync: bool,
    pub last_scan: Option<String>,
    pub requirements: Option<String>,  // JSON string
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeVersionCache {
    pub runtime_type: String,
    pub version: String,
    pub display_name: String,
    pub url: String,
    pub lts: Option<String>,
    pub is_stable: bool,
    pub release_date: Option<String>,
    pub file_size: Option<i64>,
    pub fetched_at: String,
}

/// A structured memory entry remembered by the agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: String,
    /// The memory content text
    pub content: String,
    /// Category: "fact", "preference", "project_context", "user_info", "conversation_summary"
    pub memory_type: String,
    /// Scope: "global" | "conversation:<id>" | "project:<id>"
    pub scope: String,
    /// Source: "manual" | "auto_extracted" | "conversation:<id>"
    pub source: String,
    /// Relevance score 0.0–1.0
    pub relevance: f64,
    /// Comma-separated tags for simple categorization
    pub tags: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_accessed_at: String,
    pub access_count: i32,
}

/// DB record for execution_plans table
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlanRecord {
    pub id: String,
    pub session_id: String,
    pub source: String,
    pub goal: Option<String>,
    pub plan_json: String,
    pub status: String,
    pub created_at: String,
    pub finished_at: Option<String>,
}

/// DB record for execution_plan_steps table
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStepRecord {
    pub id: String,
    pub plan_id: String,
    pub step_index: i32,
    pub label: String,
    pub step_type: String,
    pub status: String,
    pub result_json: Option<String>,
    pub error: Option<String>,
    pub started_at: Option<String>,
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRunRecord {
    pub id: String,
    pub workflow_name: String,
    pub status: String, // "running", "completed", "failed", "cancelled"
    pub step_results: Option<String>, // JSON HashMap<String, Value>
    pub step_progress: Option<String>,
    pub error: Option<String>,
    pub trigger_type: String, // "manual" or "scheduled"
    pub started_at: String,
    pub finished_at: Option<String>,
}

/// DB record for intent_domains table
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentDomainRecord {
    pub id: String,
    pub name: String,
    pub description: String,
    pub is_active: bool,
    pub source: String,
    pub version: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// DB record for intents table.
/// `examples` and `allowed_tools` are stored as JSON text in SQLite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentRecord {
    pub id: String,
    pub domain_id: String,
    pub name: String,
    pub description: String,
    pub examples: Vec<String>,
    pub workflow: String,
    pub allowed_tools: Vec<String>,
    pub default_agent: String,
    pub scope: String,
    pub enabled: bool,
    pub version: i64,
    pub confidence_threshold: f64,
    pub fallback_intent: String,
    pub path: String,
    pub auto_escalate: bool,
    pub system_prompt_appendix: String,
    pub model_id: String,
    pub max_iterations: Option<usize>,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_session() -> Session {
        Session {
            id: "session-a".to_string(),
            title: "测试会话".to_string(),
            model_id: "model-a".to_string(),
            system_prompt: None,
            persona_id: None,
            config: None,
            title_source: "manual".to_string(),
            archived: false,
            created_at: "2026-11-05T00:00:00Z".to_string(),
            updated_at: "2026-11-05T00:00:00Z".to_string(),
            mode: "chat".to_string(),
            execution_status: r#"{"type":"idle"}"#.to_string(),
            active_plan_id: None,
            project_id: None,
            intent_domain_id: Some("domain-a".to_string()),
            intent_domain_mode: "pinned".to_string(),
        }
    }

    #[test]
    fn session_json_round_trips_intent_domain_fields() {
        let session = sample_session();
        let json = serde_json::to_string(&session).unwrap();
        let restored: Session = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.intent_domain_id.as_deref(), Some("domain-a"));
        assert_eq!(restored.intent_domain_mode, "pinned");
        assert_eq!(restored.id, session.id);
    }

    #[test]
    fn session_json_without_intent_fields_still_deserializes() {
        let mut value = serde_json::to_value(sample_session()).unwrap();
        let object = value.as_object_mut().unwrap();
        object.remove("intent_domain_id");
        object.remove("intent_domain_mode");

        let restored: Session = serde_json::from_value(value).unwrap();

        assert!(
            restored.intent_domain_id.is_none(),
            "缺少 intent_domain_id 的旧 JSON 必须默认为 None"
        );
        assert_eq!(
            restored.intent_domain_mode, "auto",
            "缺少 intent_domain_mode 的旧 JSON 必须默认为 auto"
        );
    }
}
