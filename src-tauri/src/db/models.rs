use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub model_id: String,
    pub system_prompt: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
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
