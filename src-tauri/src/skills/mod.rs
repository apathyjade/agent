use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::db::models::SkillRecord;
use crate::db::repository::Database;
use crate::error::{AppError, Result};
use crate::tools::registry::ToolRegistry;
use crate::tools::r#trait::Tool;

// ── Re-export types used by commands ──
pub mod loader;
pub mod scanner;
pub use loader::{SkillEntry, SkillMeta};
pub use scanner::DiscoveredSkill;

/// Info returned to frontend (lightweight, no config values)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: Option<String>,
    pub icon: Option<String>,
    pub tags: Option<Vec<String>>,
    pub source: String, // "builtin" | "local" | "registry" | "scanned"
    pub agent_sources: Option<Vec<String>>,
    pub enabled: bool,
    pub installed_at: String,
    pub updated_at: String,
}

/// Detail returned for single skill view (includes config)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDetail {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: Option<String>,
    pub icon: Option<String>,
    pub tags: Option<Vec<String>>,
    pub source: String,
    pub source_path: Option<String>,
    pub entry_type: String,
    pub entry_value: String,
    pub config_schema: Option<Value>,
    pub config: Option<Value>,
    pub enabled: bool,
    pub agent_sources: Option<Vec<String>>,
    pub installed_at: String,
    pub updated_at: String,
}

/// Manages skill lifecycle: install, uninstall, toggle, configure, list, sync_builtins
pub struct SkillManager {
    db: Arc<Mutex<Database>>,
    tools: Arc<Mutex<ToolRegistry>>,
}

/// Built-in tool definitions - these are compiled into the binary
const BUILTIN_SKILLS: &[(&str, &str, &str, &str)] = &[
    ("calculator", "计算器", "基本的数学表达式计算", "calculator"),
    ("file_system", "文件系统", "读取、写入、列出文件和目录", "file_system"),
    ("web_search", "网页搜索", "通过网络搜索获取实时信息", "web_search"),
    ("code_executor", "代码执行", "执行 Python 和 JavaScript 代码", "code_executor"),
];

impl SkillManager {
    pub fn new(db: Arc<Mutex<Database>>, tools: Arc<Mutex<ToolRegistry>>) -> Self {
        Self { db, tools }
    }

    /// Install a skill from a local path (skill.yaml file)
    pub async fn install_from_path(&self, path: &str) -> Result<SkillInfo> {
        let yaml_path = Path::new(path);
        if !yaml_path.exists() {
            return Err(AppError::NotFound(format!(
                "Skill definition file not found at: {}",
                path
            )));
        }

        // Parse the yaml
        let meta = loader::parse_skill_yaml(yaml_path)?;

        // Check for duplicate
        {
            let db = self.db.lock().await;
            if db.skill_exists(&meta.id)? {
                return Err(AppError::SkillValidation(format!(
                    "Skill '{}' already exists. Uninstall it first or use a different id.",
                    meta.id
                )));
            }
            // Also check ToolRegistry
            let tools = self.tools.lock().await;
            if tools.is_registered(&meta.tool_name) {
                return Err(AppError::SkillValidation(format!(
                    "A tool with name '{}' is already registered.",
                    meta.tool_name
                )));
            }
        }

        // Copy skill directory to data dir
        let data_dir = Self::skills_data_dir();
        std::fs::create_dir_all(&data_dir)?;
        let skill_dir = data_dir.join(&meta.id);
        let src_dir = yaml_path.parent().unwrap_or(Path::new("."));
        Self::copy_dir(src_dir, &skill_dir)?;

        // Create tool from meta
        let tool: Arc<dyn Tool> = match &meta.entry {
            SkillEntry::BuiltinTool { .. } => {
                return Err(AppError::SkillValidation(
                    "Cannot install builtin tool type from YAML. Use 'script' entry type."
                        .to_string(),
                ));
            }
            SkillEntry::Script {
                interpreter,
                script_path,
            } => {
                let resolved_path = if Path::new(script_path).is_absolute() {
                    PathBuf::from(script_path)
                } else {
                    skill_dir.join(script_path)
                };
                Arc::new(crate::tools::script_tool::ScriptTool::new(
                    &meta.id,
                    &meta.tool_name,
                    &meta.description,
                    meta.tool_parameters.clone(),
                    interpreter,
                    &resolved_path.to_string_lossy(),
                    meta.timeout_secs.unwrap_or(30),
                ))
            }
            SkillEntry::Wasm { .. } => {
                return Err(AppError::SkillValidation(
                    "Wasm entry type is not yet supported".to_string(),
                ));
            }
        };

        // Register in ToolRegistry
        {
            let mut tools = self.tools.lock().await;
            tools.register_dynamic(&meta.tool_name, tool, true);
        }

        // Get current timestamp
        let now = chrono::Utc::now().to_rfc3339();

        // Create DB record
        let record = SkillRecord {
            id: meta.id.clone(),
            name: meta.name.clone(),
            description: meta.description.clone(),
            version: meta.version.clone(),
            author: meta.author,
            icon: meta.icon,
            tags: meta.tags.map(|t| serde_json::to_string(&t).unwrap_or_default()),
            source_type: "local".to_string(),
            source_path: Some(skill_dir.to_string_lossy().to_string()),
            entry_type: "script".to_string(),
            entry_value: meta.entry.entry_value(),
            config_schema: meta.config_schema.map(|v| v.to_string()),
            config: None,
            enabled: true,
            agent_sources: None,
            installed_at: now.clone(),
            updated_at: now,
        };

        {
            let db = self.db.lock().await;
            db.insert_skill(&record)?;
        }

        Ok(Self::record_to_info(&record))
    }

    /// Uninstall a skill
    pub async fn uninstall(&self, id: &str) -> Result<()> {
        let db = self.db.lock().await;
        let skill = db
            .get_skill(id)?
            .ok_or_else(|| AppError::NotFound(format!("Skill '{}' not found", id)))?;

        if skill.source_type == "builtin" {
            return Err(AppError::SkillValidation(format!(
                "Cannot uninstall built-in skill '{}'. You can disable it instead.",
                id
            )));
        }

        // For script skills, entry_value is the tool name
        let tool_name = &skill.entry_value;

        {
            let mut tools = self.tools.lock().await;
            tools.unregister(tool_name);
        }

        // Delete DB record
        db.delete_skill(id)?;

        // Remove skill directory if local
        if let Some(path) = &skill.source_path {
            let dir = Path::new(path);
            if dir.exists() {
                std::fs::remove_dir_all(dir)?;
            }
        }

        Ok(())
    }

    /// Toggle skill enabled/disabled
    pub async fn toggle(&self, id: &str, enabled: bool) -> Result<()> {
        let db = self.db.lock().await;
        let skill = db
            .get_skill(id)?
            .ok_or_else(|| AppError::NotFound(format!("Skill '{}' not found", id)))?;

        let tool_name = &skill.entry_value;

        {
            let mut tools = self.tools.lock().await;
            tools.toggle(tool_name, enabled)?;
        }

        db.update_skill_enabled(id, enabled)?;
        Ok(())
    }

    /// Update skill configuration
    pub async fn configure(&self, id: &str, config: Value) -> Result<()> {
        let db = self.db.lock().await;
        let skill = db
            .get_skill(id)?
            .ok_or_else(|| AppError::NotFound(format!("Skill '{}' not found", id)))?;

        // If there's a config_schema, do basic validation
        if skill.config_schema.is_some() {
            // Basic validation: config must be an object
            if !config.is_object() {
                return Err(AppError::SkillValidation(
                    "Config must be a JSON object".to_string(),
                ));
            }
        }

        let config_str = config.to_string();
        db.update_skill_config(id, &config_str)?;
        Ok(())
    }

    /// List all skills
    pub async fn list(&self) -> Result<Vec<SkillInfo>> {
        let db = self.db.lock().await;
        let records = db.list_skills()?;
        Ok(records.iter().map(Self::record_to_info).collect())
    }

    /// Get skill detail (with config)
    pub async fn get_detail(&self, id: &str) -> Result<SkillDetail> {
        let db = self.db.lock().await;
        let record = db
            .get_skill(id)?
            .ok_or_else(|| AppError::NotFound(format!("Skill '{}' not found", id)))?;
        Self::record_to_detail(&record)
    }

    /// Sync built-in skills: ensure all built-in tools have SkillRecord entries
    pub async fn sync_builtins(&self) -> Result<()> {
        let db = self.db.lock().await;
        let now = chrono::Utc::now().to_rfc3339();

        for (id, name, description, tool_name) in BUILTIN_SKILLS {
            if !db.skill_exists(id)? {
                let record = SkillRecord {
                    id: id.to_string(),
                    name: name.to_string(),
                    description: description.to_string(),
                    version: "1.0.0".to_string(),
                    author: None,
                    icon: None,
                    tags: None,
                    source_type: "builtin".to_string(),
                    source_path: None,
                    entry_type: "builtintool".to_string(),
                    entry_value: tool_name.to_string(),
                    config_schema: None,
                    config: None,
                    enabled: true,
                    agent_sources: None,
                    installed_at: now.clone(),
                    updated_at: now.clone(),
                };
                db.insert_skill(&record)?;
            }
        }

        Ok(())
    }

    /// Scan all known agent directories for discoverable skills.
    /// Returns a list of discovered skills with deduplication and import status.
    pub async fn scan_local(&self) -> Result<Vec<DiscoveredSkill>> {
        // Get list of already-imported skill IDs
        let imported_ids: Vec<String> = {
            let db = self.db.lock().await;
            db.list_skills()?.into_iter().map(|s| s.id).collect()
        };

        // Run scanner
        let mut discovered = crate::skills::scanner::scan_all(None)?;

        // Mark already-imported skills
        for skill in discovered.iter_mut() {
            if imported_ids.contains(&skill.id) {
                skill.already_imported = true;
            }
        }

        Ok(discovered)
    }

    /// Import a skill discovered by scanning agent directories.
    /// `discovered_path` is the directory containing skill.yaml.
    /// `agent_sources` is the list of agent names where this skill was found.
    pub async fn import_scanned(&self, _discovered_id: &str, discovered_path: &str, agent_sources: Vec<String>) -> Result<SkillInfo> {
        // Look for skill.yaml in the discovered directory
        let dir = Path::new(discovered_path);
        let skill_yaml = dir.join("skill.yaml");

        if !skill_yaml.exists() {
            return Err(AppError::SkillValidation(format!(
                "No skill.yaml found in '{}'. Only skill.yaml format can be imported.",
                discovered_path
            )));
        }

        // Use existing install_from_path logic
        let info = self.install_from_path(&skill_yaml.to_string_lossy()).await?;

        // Update the DB record with agent_sources and mark as scanned
        let sources_json = serde_json::to_string(&agent_sources)
            .unwrap_or_default();
        {
            let db = self.db.lock().await;
            db.update_skill_source_type(&info.id, "scanned")?;
            db.update_skill_agent_sources(&info.id, &sources_json)?;
        }

        // Re-read the updated record
        let db = self.db.lock().await;
        let final_record = db.get_skill(&info.id)?
            .ok_or_else(|| AppError::NotFound("Skill not found after import".to_string()))?;

        Ok(Self::record_to_info(&final_record))
    }

    // ── Helpers ──

    fn skills_data_dir() -> PathBuf {
        let mut path = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("agent");
        path.push("skills");
        path
    }

    fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
        if !dst.exists() {
            std::fs::create_dir_all(dst)?;
        }
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());
            if file_type.is_dir() {
                Self::copy_dir(&src_path, &dst_path)?;
            } else {
                std::fs::copy(&src_path, &dst_path)?;
            }
        }
        Ok(())
    }

    fn record_to_info(record: &SkillRecord) -> SkillInfo {
        SkillInfo {
            id: record.id.clone(),
            name: record.name.clone(),
            description: record.description.clone(),
            version: record.version.clone(),
            author: record.author.clone(),
            icon: record.icon.clone(),
            tags: record
                .tags
                .as_ref()
                .and_then(|t| serde_json::from_str(t).ok()),
            source: record.source_type.clone(),
            agent_sources: record
                .agent_sources
                .as_ref()
                .and_then(|s| serde_json::from_str(s).ok()),
            enabled: record.enabled,
            installed_at: record.installed_at.clone(),
            updated_at: record.updated_at.clone(),
        }
    }

    fn record_to_detail(record: &SkillRecord) -> Result<SkillDetail> {
        Ok(SkillDetail {
            id: record.id.clone(),
            name: record.name.clone(),
            description: record.description.clone(),
            version: record.version.clone(),
            author: record.author.clone(),
            icon: record.icon.clone(),
            tags: record
                .tags
                .as_ref()
                .and_then(|t| serde_json::from_str(t).ok()),
            source: record.source_type.clone(),
            source_path: record.source_path.clone(),
            entry_type: record.entry_type.clone(),
            entry_value: record.entry_value.clone(),
            config_schema: record
                .config_schema
                .as_ref()
                .and_then(|s| serde_json::from_str(s).ok()),
            config: record
                .config
                .as_ref()
                .and_then(|s| serde_json::from_str(s).ok()),
            enabled: record.enabled,
            agent_sources: record
                .agent_sources
                .as_ref()
                .and_then(|s| serde_json::from_str(s).ok()),
            installed_at: record.installed_at.clone(),
            updated_at: record.updated_at.clone(),
        })
    }
}
