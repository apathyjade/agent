use std::sync::Arc;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

pub mod seeds;

use crate::db::models::PersonaRecord;
use crate::db::repository::Database;
use crate::error::Result;

/// Lightweight persona info returned to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonaInfo {
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

impl From<PersonaRecord> for PersonaInfo {
    fn from(r: PersonaRecord) -> Self {
        Self {
            id: r.id,
            name: r.name,
            title: r.title,
            emoji: r.emoji,
            description: r.description,
            system_prompt: r.system_prompt,
            temperature: r.temperature,
            response_style: r.response_style,
            model_provider: r.model_provider,
            model_name: r.model_name,
            is_default: r.is_default,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePersonaParams {
    pub name: String,
    pub title: Option<String>,
    pub emoji: Option<String>,
    pub description: Option<String>,
    pub system_prompt: String,
    pub temperature: Option<f64>,
    pub response_style: Option<String>,
    pub model_provider: Option<String>,
    pub model_name: Option<String>,
    pub is_default: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePersonaParams {
    pub name: Option<String>,
    pub title: Option<String>,
    pub emoji: Option<String>,
    pub description: Option<String>,
    pub system_prompt: Option<String>,
    pub temperature: Option<f64>,
    pub response_style: Option<String>,
    pub model_provider: Option<String>,
    pub model_name: Option<String>,
    pub is_default: Option<bool>,
}

/// Persona resolution result — how a persona was selected for a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PersonaResolution {
    /// User explicitly specified
    Manual(PersonaInfo),
    /// System auto-detected (or continued from active)
    Auto(PersonaInfo),
    /// Using default persona
    Default(PersonaInfo),
    /// No persona matched
    None,
}

pub struct PersonaManager {
    db: Arc<Mutex<Database>>,
}

impl PersonaManager {
    pub fn new(db: Arc<Mutex<Database>>) -> Self {
        Self { db }
    }

    /// Seed default personas if none exist.
    pub async fn seed_defaults(&self) -> Result<usize> {
        let default_personas = crate::persona::seeds::default_personas();
        let db = self.db.lock().await;
        let existing = db.list_personas()?;
        if !existing.is_empty() {
            return Ok(0);
        }
        let mut count = 0;
        for p in &default_personas {
            match db.insert_persona(p) {
                Ok(()) => count += 1,
                Err(e) => log::warn!("Failed to seed persona '{}': {}", p.name, e),
            }
        }
        if count > 0 {
            log::info!("Seeded {} default personas", count);
        }
        Ok(count)
    }

    // ── CRUD ──

    pub async fn create(&self, params: CreatePersonaParams) -> Result<PersonaInfo> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        if params.name.trim().is_empty() {
            return Err(crate::error::AppError::InvalidInput("Persona name cannot be empty".to_string()));
        }
        if params.system_prompt.trim().is_empty() {
            return Err(crate::error::AppError::InvalidInput("Persona system_prompt cannot be empty".to_string()));
        }

        let valid_styles = ["concise", "verbose", "academic"];
        let style = params.response_style.as_deref().unwrap_or("concise");
        if !valid_styles.contains(&style) {
            return Err(crate::error::AppError::InvalidInput(format!(
                "Invalid response_style '{}'. Must be: {:?}", style, valid_styles
            )));
        }

        let db = self.db.lock().await;
        let is_default = params.is_default.unwrap_or(false);
        if is_default {
            db.clear_persona_default()?;
        }

        let record = PersonaRecord {
            id: id.clone(),
            name: params.name.trim().to_string(),
            title: params.title.unwrap_or_default(),
            emoji: params.emoji.unwrap_or_else(|| "\u{1f9d1}\u{200d}\u{1f4bb}".to_string()),
            description: params.description.unwrap_or_default(),
            system_prompt: params.system_prompt,
            temperature: params.temperature.unwrap_or(0.3).clamp(0.0, 2.0),
            response_style: style.to_string(),
            model_provider: params.model_provider.unwrap_or_default(),
            model_name: params.model_name.unwrap_or_default(),
            is_default,
            created_at: now.clone(),
            updated_at: now,
        };

        db.insert_persona(&record)?;
        Ok(PersonaInfo::from(record))
    }

    pub async fn list(&self) -> Result<Vec<PersonaInfo>> {
        let db = self.db.lock().await;
        let records = db.list_personas()?;
        Ok(records.into_iter().map(PersonaInfo::from).collect())
    }

    pub async fn get(&self, id: &str) -> Result<PersonaInfo> {
        let db = self.db.lock().await;
        let record = db.get_persona(id)?
            .ok_or_else(|| crate::error::AppError::NotFound(format!("Persona '{}' not found", id)))?;
        Ok(PersonaInfo::from(record))
    }

    pub async fn get_by_name(&self, name: &str) -> Result<Option<PersonaInfo>> {
        let db = self.db.lock().await;
        let record = db.get_persona_by_name(name)?;
        Ok(record.map(PersonaInfo::from))
    }

    pub async fn get_default(&self) -> Result<Option<PersonaInfo>> {
        let db = self.db.lock().await;
        let record = db.get_default_persona()?;
        Ok(record.map(PersonaInfo::from))
    }

    pub async fn update(&self, id: &str, params: UpdatePersonaParams) -> Result<PersonaInfo> {
        let db = self.db.lock().await;
        let mut record = db.get_persona(id)?
            .ok_or_else(|| crate::error::AppError::NotFound(format!("Persona '{}' not found", id)))?;

        if let Some(name) = params.name {
            if name.trim().is_empty() {
                return Err(crate::error::AppError::InvalidInput("Persona name cannot be empty".to_string()));
            }
            record.name = name.trim().to_string();
        }
        if let Some(title) = params.title { record.title = title; }
        if let Some(emoji) = params.emoji { record.emoji = emoji; }
        if let Some(desc) = params.description { record.description = desc; }
        if let Some(sp) = params.system_prompt {
            if sp.trim().is_empty() {
                return Err(crate::error::AppError::InvalidInput("Persona system_prompt cannot be empty".to_string()));
            }
            record.system_prompt = sp;
        }
        if let Some(temp) = params.temperature { record.temperature = temp.clamp(0.0, 2.0); }
        if let Some(style) = params.response_style {
            let valid = ["concise", "verbose", "academic"];
            if !valid.contains(&style.as_str()) {
                return Err(crate::error::AppError::InvalidInput(format!(
                    "Invalid response_style '{}'", style
                )));
            }
            record.response_style = style;
        }
        if let Some(prov) = params.model_provider { record.model_provider = prov; }
        if let Some(model) = params.model_name { record.model_name = model; }
        if let Some(is_default) = params.is_default {
            if is_default {
                db.clear_persona_default()?;
            }
            record.is_default = is_default;
        }

        record.updated_at = Utc::now().to_rfc3339();
        db.update_persona(&record)?;
        Ok(PersonaInfo::from(record))
    }

    pub async fn delete(&self, id: &str) -> Result<()> {
        let db = self.db.lock().await;
        let _ = db.get_persona(id)?
            .ok_or_else(|| crate::error::AppError::NotFound(format!("Persona '{}' not found", id)))?;
        db.delete_persona(id)?;
        Ok(())
    }

    // ── Memory linking ──

    pub async fn link_memory(&self, persona_id: &str, memory_id: &str) -> Result<()> {
        let db = self.db.lock().await;
        db.link_memory_to_persona(persona_id, memory_id)?;
        Ok(())
    }

    pub async fn unlink_memory(&self, persona_id: &str, memory_id: &str) -> Result<()> {
        let db = self.db.lock().await;
        db.unlink_memory_from_persona(persona_id, memory_id)?;
        Ok(())
    }

    pub async fn get_linked_memory_ids(&self, persona_id: &str) -> Result<Vec<String>> {
        let db = self.db.lock().await;
        db.get_persona_memory_ids(persona_id)
    }

    // ── Project binding ──

    pub async fn bind_project(&self, persona_id: &str, project_path: &str, auto_select: bool) -> Result<()> {
        let db = self.db.lock().await;
        db.bind_persona_to_project(persona_id, project_path, auto_select)?;
        Ok(())
    }

    pub async fn unbind_project(&self, persona_id: &str, project_path: &str) -> Result<()> {
        let db = self.db.lock().await;
        db.unbind_persona_from_project(persona_id, project_path)?;
        Ok(())
    }

    // ── Resolution (Persona Selection) ──

    /// Try to extract a persona name from the beginning of a user message.
    /// Supports: "Alex, xxx", "Alex: xxx", "/persona Alex", "Alex：xxx"
    pub fn extract_persona_from_message(msg: &str) -> Option<String> {
        let msg = msg.trim();
        // /persona <name>
        if let Some(rest) = msg.strip_prefix("/persona ") {
            let name = rest.trim().split_whitespace().next()?;
            if !name.is_empty() { return Some(name.to_string()); }
        }
        // "Name, ..." or "Name: ..." or "Name，..."
        for sep in [", ", ": ", "\u{ff0c}", "\u{ff1a}"] {
            if let Some(idx) = msg.find(sep) {
                let candidate = &msg[..idx].trim();
                if !candidate.is_empty() && candidate.len() <= 30 {
                    if candidate.chars().next().map_or(false, |c| c.is_uppercase()) {
                        return Some(candidate.to_string());
                    }
                }
            }
        }
        None
    }

    /// Try to auto-detect the best persona for a project based on file/language detection.
    pub async fn detect_for_project(&self, project_path: &str) -> Result<Option<PersonaInfo>> {
        let db = self.db.lock().await;

        // 1. Check explicit project bindings
        let bound = db.get_personas_for_project(project_path)?;
        if let Some(p) = bound.into_iter().find(|p| p.is_default) {
            return Ok(Some(PersonaInfo::from(p)));
        }

        // 2. Score personas by name/title matching project context keywords
        let all = db.list_personas()?;
        let path_lower = project_path.to_lowercase();

        let mut scored: Vec<(i32, PersonaRecord)> = all.into_iter().filter_map(|p| {
            let lower_name = p.name.to_lowercase();
            let lower_title = p.title.to_lowercase();
            let score = if path_lower.contains("rust") && (lower_name.contains("rust") || lower_title.contains("rust")) {
                10
            } else if (path_lower.contains("node") || path_lower.contains("js") || path_lower.contains("ts")
                || path_lower.contains("react") || path_lower.contains("vue"))
                && (lower_name.contains("type") || lower_title.contains("type")
                    || lower_title.contains("front") || lower_title.contains("web"))
            {
                8
            } else if p.is_default {
                5
            } else {
                0
            };
            if score > 0 { Some((score, p)) } else { None }
        }).collect();

        scored.sort_by(|a, b| b.0.cmp(&a.0));

        if scored.len() == 1 {
            return Ok(Some(PersonaInfo::from(scored[0].1.clone())));
        }
        if scored.len() >= 2 && scored[0].0 - scored[1].0 >= 3 {
            return Ok(Some(PersonaInfo::from(scored[0].1.clone())));
        }

        Ok(None)
    }

    /// Resolve which persona to use.
    /// Priority: manual → active session → project detect → default → none.
    pub async fn resolve(
        &self,
        message: &str,
        project_path: Option<&str>,
        active_persona_id: Option<&str>,
    ) -> PersonaResolution {
        // 1. Manual from message prefix
        if let Some(name) = Self::extract_persona_from_message(message) {
            if let Ok(Some(p)) = self.get_by_name(&name).await {
                return PersonaResolution::Manual(p);
            }
        }

        // 2. Continue active persona from session
        if let Some(active_id) = active_persona_id {
            if let Ok(p) = self.get(active_id).await {
                return PersonaResolution::Auto(p);
            }
        }

        // 3. Project-based detection
        if let Some(path) = project_path {
            if let Ok(Some(p)) = self.detect_for_project(path).await {
                return PersonaResolution::Auto(p);
            }
        }

        // 4. Default fallback
        if let Ok(Some(p)) = self.get_default().await {
            return PersonaResolution::Default(p);
        }

        PersonaResolution::None
    }
}
