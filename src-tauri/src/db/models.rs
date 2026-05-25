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
pub struct SkillRecord {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: Option<String>,
    pub icon: Option<String>,
    pub tags: Option<String>,
    pub source_type: String,
    pub source_path: Option<String>,
    pub entry_type: String,
    pub entry_value: String,
    pub config_schema: Option<String>,
    pub config: Option<String>,
    pub enabled: bool,
    pub agent_sources: Option<String>, // JSON array of agent source names, e.g. ["claude-code","opencode"]
    pub installed_at: String,
    pub updated_at: String,
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
