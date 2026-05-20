use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::db::models::SkillRecord;
use crate::db::repository::Database;
use crate::error::{AppError, Result};

// ── Re-export types used by commands ──
pub mod loader;
pub mod market;
pub mod scanner;
pub use loader::{SkillEntry, SkillMeta};

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
    pub source: String, // "local" | "registry"
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

/// Manages skill lifecycle: install, uninstall, toggle, configure, list.
/// This is an independent module for managing skills.sh ecosystem skills
/// on the local machine. It does NOT interact with the project's ToolRegistry.
pub struct SkillManager {
    db: Arc<Mutex<Database>>,
}

impl SkillManager {
    pub fn new(db: Arc<Mutex<Database>>) -> Self {
        Self { db }
    }

    /// Install a skill from a local path (skill.yaml file).
    /// Copies files to data dir and creates a DB record.
    /// Does NOT register the skill as a tool in the agent's ToolRegistry.
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

        // Check for duplicate in DB only
        {
            let db = self.db.lock().await;
            if db.skill_exists(&meta.id)? {
                return Err(AppError::SkillValidation(format!(
                    "Skill '{}' already exists. Uninstall it first or use a different id.",
                    meta.id
                )));
            }
        }

        // Copy skill directory to data dir
        let data_dir = Self::skills_data_dir();
        std::fs::create_dir_all(&data_dir)?;
        let skill_dir = data_dir.join(&meta.id);
        let src_dir = yaml_path.parent().unwrap_or(Path::new("."));
        Self::copy_dir(src_dir, &skill_dir)?;

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

    /// Uninstall a skill: remove DB record and installed files.
    pub async fn uninstall(&self, id: &str) -> Result<()> {
        let db = self.db.lock().await;
        let skill = db
            .get_skill(id)?
            .ok_or_else(|| AppError::NotFound(format!("Skill '{}' not found", id)))?;

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

    /// Toggle skill enabled/disabled (DB-only, independent of ToolRegistry)
    pub async fn toggle(&self, id: &str, enabled: bool) -> Result<()> {
        let db = self.db.lock().await;
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

    /// List all skills from DB (filters out legacy builtin records)
    pub async fn list(&self) -> Result<Vec<SkillInfo>> {
        let db = self.db.lock().await;
        let records = db.list_skills()?;
        let records: Vec<&SkillRecord> = records.iter().filter(|r| r.source_type != "builtin").collect();
        Ok(records.iter().map(|r| Self::record_to_info(r)).collect())
    }

    /// Get skill detail (with config)
    pub async fn get_detail(&self, id: &str) -> Result<SkillDetail> {
        let db = self.db.lock().await;
        let record = db
            .get_skill(id)?
            .ok_or_else(|| AppError::NotFound(format!("Skill '{}' not found", id)))?;
        Self::record_to_detail(&record)
    }

    /// Clean up legacy builtin skill records from the DB (migration from before v2 decoupling).
    pub async fn cleanup_legacy_builtins(&self) -> Result<()> {
        let db = self.db.lock().await;
        db.delete_skills_by_source_type("builtin")?;
        log::info!("Cleaned up legacy builtin skill records from DB");
        Ok(())
    }

    /// Reconcile the database with the actual state of the skills data directory.
    /// Scans known skill directories, auto-adds skills found on disk but missing
    /// from DB, and auto-removes DB records whose source_path no longer exists.
    pub async fn reconcile(&self) -> Result<scanner::ReconcileResult> {
        let data_dir = Self::skills_data_dir();
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));

        // Collect directories to scan (deduplicated)
        let mut scan_paths: Vec<PathBuf> = Vec::new();
        for rel in scanner::HOME_SKILL_DIRS {
            let p = home.join(rel);
            if !scan_paths.iter().any(|x| *x == p) {
                scan_paths.push(p);
            }
        }
        // Always include our managed directory if not already in list
        if !scan_paths.iter().any(|x| *x == data_dir) {
            scan_paths.push(data_dir.clone());
        }

        // Get current DB records
        let db = self.db.lock().await;
        let db_skills = db.list_skills()?;
        drop(db);

        // Scan all directories
        let dir_refs: Vec<&Path> = scan_paths.iter().map(|p| p.as_path()).collect();
        let disk_skills = scanner::scan_dirs(&dir_refs)?;

        let mut added = Vec::new();
        let mut removed = Vec::new();
        let now = chrono::Utc::now().to_rfc3339();

        // 1. Auto-add skills on disk that are missing from DB
        for skill in &disk_skills {
            if db_skills.iter().any(|s| s.id == skill.id) {
                continue;
            }

            log::info!("Reconcile: skill '{}' found on disk but missing from DB — adding", skill.id);

            let (entry_type, entry_value) = if skill.format == "yaml" {
                let yaml_path = skill.path.join("skill.yaml");
                match loader::parse_skill_yaml(&yaml_path) {
                    Ok(meta) => ("script".to_string(), meta.entry.entry_value()),
                    Err(e) => {
                        log::warn!("Reconcile: failed to parse skill.yaml for '{}': {}", skill.id, e);
                        continue;
                    }
                }
            } else {
                ("skill.md".to_string(), skill.path.join("SKILL.md").to_string_lossy().to_string())
            };

            let record = crate::db::models::SkillRecord {
                id: skill.id.clone(),
                name: skill.name.clone(),
                description: skill.description.clone(),
                version: skill.version.clone(),
                author: skill.author.clone(),
                icon: skill.icon.clone(),
                tags: skill.tags.clone().map(|t| serde_json::to_string(&t).unwrap_or_default()),
                source_type: "local".to_string(),
                source_path: Some(skill.path.to_string_lossy().to_string()),
                entry_type,
                entry_value,
                config_schema: None,
                config: None,
                enabled: true,
                agent_sources: None,
                installed_at: now.clone(),
                updated_at: now.clone(),
            };

            let db = self.db.lock().await;
            if db.insert_skill(&record).is_ok() {
                added.push(skill.id.clone());
            }
        }

        // 2. Auto-remove DB records whose actual source_path no longer exists
        let db = self.db.lock().await;
        for db_skill in &db_skills {
            if let Some(ref sp) = db_skill.source_path {
                let disk_path = Path::new(sp);
                if !disk_path.exists() {
                    log::info!("Reconcile: skill '{}' in DB but directory missing at '{}' — removing", db_skill.id, sp);
                    if db.delete_skill(&db_skill.id).is_ok() {
                        removed.push(db_skill.id.clone());
                    }
                }
            }
        }
        drop(db);

        Ok(scanner::ReconcileResult { added, removed })
    }

    // ── Helpers ──

    fn skills_data_dir() -> PathBuf {
        let mut path = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("agent");
        path.push("skills");
        path
    }

    pub fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
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
