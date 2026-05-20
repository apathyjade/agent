use std::path::Path;
use serde::Deserialize;
use serde_json::Value;

use crate::error::{AppError, Result};

/// Skill entry type - how the skill executes
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SkillEntry {
    BuiltinTool { tool_name: String },
    Script { interpreter: String, script_path: String },
    Wasm { module_path: String },
}

impl SkillEntry {
    pub fn entry_value(&self) -> String {
        match self {
            SkillEntry::BuiltinTool { tool_name } => tool_name.clone(),
            SkillEntry::Script { script_path, .. } => script_path.clone(),
            SkillEntry::Wasm { module_path } => module_path.clone(),
        }
    }
}

/// Parsed skill metadata from skill.yaml
#[derive(Debug, Clone, Deserialize)]
pub struct SkillMeta {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default = "default_version")]
    pub version: String,
    pub author: Option<String>,
    pub icon: Option<String>,
    pub tags: Option<Vec<String>>,
    pub entry: SkillEntry,
    pub config_schema: Option<Value>,
    pub timeout_secs: Option<u64>,

    /// The tool name sent to LLM (defaults to id if not specified)
    #[serde(default)]
    pub tool_name: String,

    /// Tool description sent to LLM (defaults to description)
    #[serde(default)]
    pub tool_description: String,

    /// JSON Schema for tool parameters
    pub tool_parameters: Option<Value>,
}

fn default_version() -> String {
    "0.1.0".to_string()
}

/// Parse a skill.yaml file and return SkillMeta
pub fn parse_skill_yaml(path: &Path) -> Result<SkillMeta> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| AppError::Io(e))?;

    let mut meta: SkillMeta = serde_yaml::from_str(&content)
        .map_err(|e| AppError::SkillValidation(format!("Invalid skill.yaml: {}", e)))?;

    // Apply defaults
    if meta.tool_name.is_empty() {
        meta.tool_name = meta.id.clone();
    }
    if meta.tool_description.is_empty() {
        meta.tool_description = meta.description.clone();
    }

    Ok(meta)
}
