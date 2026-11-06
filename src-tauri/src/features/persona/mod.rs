//! Persona（人格）管理。
//!
//! Persona 是一组可复用的角色设定：系统提示词、温度、回复风格与偏好模型。
//! 本模块只负责数据库 CRUD 与「为某条消息挑选 persona」的解析逻辑，不直接
//! 调用 LLM；向量检索与记忆注入由调用方组装。

use std::sync::Arc;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

pub mod seeds;

use crate::core::db::models::PersonaRecord;
use crate::core::db::repository::Database;
use crate::core::error::Result;

/// 返回给前端的轻量 persona 信息。
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

/// 创建 persona 的入参；除 `name` 与 `system_prompt` 外都可缺省。
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

/// 更新 persona 的入参；`None` 表示该字段保持不变。
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

/// persona 解析结果，说明本次会话的 persona 是如何选出来的。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PersonaResolution {
    /// 用户显式指定
    Manual(PersonaInfo),
    /// 系统自动识别（或沿用当前活跃 persona）
    Auto(PersonaInfo),
    /// 使用默认 persona
    Default(PersonaInfo),
    /// 没有匹配到任何 persona
    None,
}

/// Persona 的数据库访问层。
///
/// 所有方法都会短暂持有数据库互斥锁；解析流程（[`PersonaManager::resolve`]）
/// 内部会多次加锁，因此不要在调用方已持有锁时再调用。
pub struct PersonaManager {
    db: Arc<Mutex<Database>>,
}

impl PersonaManager {
    /// 用共享数据库句柄构造管理器。
    pub fn new(db: Arc<Mutex<Database>>) -> Self {
        Self { db }
    }

    /// 首次启动时写入内置 persona；已有任何 persona 时直接返回 0。
    pub async fn seed_defaults(&self) -> Result<usize> {
        let default_personas = crate::features::persona::seeds::default_personas();
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

    // ── 增删改查 ──

    /// 创建 persona。
    ///
    /// 校验名称与系统提示词非空、`response_style` 属于
    /// `{concise, verbose, academic}`；温度会被夹到 `[0.0, 2.0]`。
    /// 指定 `is_default` 时会先清除其他默认标记。
    pub async fn create(&self, params: CreatePersonaParams) -> Result<PersonaInfo> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        if params.name.trim().is_empty() {
            return Err(crate::core::error::AppError::InvalidInput("Persona name cannot be empty".to_string()));
        }
        if params.system_prompt.trim().is_empty() {
            return Err(crate::core::error::AppError::InvalidInput("Persona system_prompt cannot be empty".to_string()));
        }

        let valid_styles = ["concise", "verbose", "academic"];
        let style = params.response_style.as_deref().unwrap_or("concise");
        if !valid_styles.contains(&style) {
            return Err(crate::core::error::AppError::InvalidInput(format!(
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

    /// 列出全部 persona。
    pub async fn list(&self) -> Result<Vec<PersonaInfo>> {
        let db = self.db.lock().await;
        let records = db.list_personas()?;
        Ok(records.into_iter().map(PersonaInfo::from).collect())
    }

    /// 按 id 获取 persona，不存在时返回 `NotFound`。
    pub async fn get(&self, id: &str) -> Result<PersonaInfo> {
        let db = self.db.lock().await;
        let record = db.get_persona(id)?
            .ok_or_else(|| crate::core::error::AppError::NotFound(format!("Persona '{}' not found", id)))?;
        Ok(PersonaInfo::from(record))
    }

    /// 按名称获取 persona；重名时返回数据库找到的第一个。
    pub async fn get_by_name(&self, name: &str) -> Result<Option<PersonaInfo>> {
        let db = self.db.lock().await;
        let record = db.get_persona_by_name(name)?;
        Ok(record.map(PersonaInfo::from))
    }

    /// 获取当前默认 persona。
    pub async fn get_default(&self) -> Result<Option<PersonaInfo>> {
        let db = self.db.lock().await;
        let record = db.get_default_persona()?;
        Ok(record.map(PersonaInfo::from))
    }

    /// 更新 persona，只覆盖入参中给出值的字段。
    ///
    /// 与 [`PersonaManager::create`] 相同的校验规则；设置 `is_default`
    /// 为 true 时会先清除其他默认标记。
    pub async fn update(&self, id: &str, params: UpdatePersonaParams) -> Result<PersonaInfo> {
        let db = self.db.lock().await;
        let mut record = db.get_persona(id)?
            .ok_or_else(|| crate::core::error::AppError::NotFound(format!("Persona '{}' not found", id)))?;

        if let Some(name) = params.name {
            if name.trim().is_empty() {
                return Err(crate::core::error::AppError::InvalidInput("Persona name cannot be empty".to_string()));
            }
            record.name = name.trim().to_string();
        }
        if let Some(title) = params.title { record.title = title; }
        if let Some(emoji) = params.emoji { record.emoji = emoji; }
        if let Some(desc) = params.description { record.description = desc; }
        if let Some(sp) = params.system_prompt {
            if sp.trim().is_empty() {
                return Err(crate::core::error::AppError::InvalidInput("Persona system_prompt cannot be empty".to_string()));
            }
            record.system_prompt = sp;
        }
        if let Some(temp) = params.temperature { record.temperature = temp.clamp(0.0, 2.0); }
        if let Some(style) = params.response_style {
            let valid = ["concise", "verbose", "academic"];
            if !valid.contains(&style.as_str()) {
                return Err(crate::core::error::AppError::InvalidInput(format!(
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

    /// 删除 persona；不存在时返回 `NotFound`。
    pub async fn delete(&self, id: &str) -> Result<()> {
        let db = self.db.lock().await;
        let _ = db.get_persona(id)?
            .ok_or_else(|| crate::core::error::AppError::NotFound(format!("Persona '{}' not found", id)))?;
        db.delete_persona(id)?;
        Ok(())
    }

    // ── 记忆关联 ──

    /// 把一条记忆关联到 persona。
    pub async fn link_memory(&self, persona_id: &str, memory_id: &str) -> Result<()> {
        let db = self.db.lock().await;
        db.link_memory_to_persona(persona_id, memory_id)?;
        Ok(())
    }

    /// 解除 persona 与一条记忆的关联。
    pub async fn unlink_memory(&self, persona_id: &str, memory_id: &str) -> Result<()> {
        let db = self.db.lock().await;
        db.unlink_memory_from_persona(persona_id, memory_id)?;
        Ok(())
    }

    /// 获取 persona 关联的所有记忆 id。
    pub async fn get_linked_memory_ids(&self, persona_id: &str) -> Result<Vec<String>> {
        let db = self.db.lock().await;
        db.get_persona_memory_ids(persona_id)
    }

    // ── 项目绑定 ──

    /// 把 persona 绑定到项目路径；`auto_select` 表示进入该项目时自动选用。
    pub async fn bind_project(&self, persona_id: &str, project_path: &str, auto_select: bool) -> Result<()> {
        let db = self.db.lock().await;
        db.bind_persona_to_project(persona_id, project_path, auto_select)?;
        Ok(())
    }

    /// 解除 persona 与项目的绑定。
    pub async fn unbind_project(&self, persona_id: &str, project_path: &str) -> Result<()> {
        let db = self.db.lock().await;
        db.unbind_persona_from_project(persona_id, project_path)?;
        Ok(())
    }

    // ── 解析（persona 选择） ──

    /// 尝试从用户消息开头提取 persona 名称。
    /// 支持：`Alex, xxx`、`Alex: xxx`、`/persona Alex`、`Alex：xxx`。
    pub fn extract_persona_from_message(msg: &str) -> Option<String> {
        let msg = msg.trim();
        // 显式指令：/persona <name>
        if let Some(rest) = msg.strip_prefix("/persona ") {
            let name = rest.split_whitespace().next()?;
            if !name.is_empty() { return Some(name.to_string()); }
        }
        // 形式："Name, ..." / "Name: ..." / "Name，..."
        for sep in [", ", ": ", "\u{ff0c}", "\u{ff1a}"] {
            if let Some(idx) = msg.find(sep) {
                let candidate = &msg[..idx].trim();
                if !candidate.is_empty() && candidate.len() <= 30
                && candidate.chars().next().is_some_and(|c| c.is_uppercase() || !c.is_ascii()) {
                    return Some(candidate.to_string());
                }
            }
        }
        None
    }

    /// 根据项目路径与文件/语言特征尝试自动识别最合适的 persona。
    ///
    /// 先看显式项目绑定，再按名称/标题里的关键词打分；只有唯一最高分或
    /// 领先第二名至少 3 分时才返回结果，避免模棱两可的选择。
    pub async fn detect_for_project(&self, project_path: &str) -> Result<Option<PersonaInfo>> {
        let db = self.db.lock().await;

        // 1. 显式项目绑定
        let bound = db.get_personas_for_project(project_path)?;
        if let Some(p) = bound.into_iter().find(|p| p.is_default) {
            return Ok(Some(PersonaInfo::from(p)));
        }

        // 2. 按名称/标题与项目上下文关键词打分
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

    /// 解析本次应使用的 persona。
    ///
    /// 优先级：消息里的手动指定 → 消息前缀匹配 → 当前活跃 persona →
    /// 项目识别 → 默认 persona → 无。
    pub async fn resolve(
        &self,
        message: &str,
        project_path: Option<&str>,
        active_persona_id: Option<&str>,
    ) -> PersonaResolution {
        // 1. 消息前缀里的显式名称
        if let Some(name) = Self::extract_persona_from_message(message) {
            if let Ok(Some(p)) = self.get_by_name(&name).await {
                return PersonaResolution::Manual(p);
            }
        }

        // 1b. 无分隔符的前缀匹配（例如中文「张三介绍一下...」），
        //     取消息开头能匹配到的最长 persona 名称。
        let trimmed = message.trim();
        if let Ok(personas) = self.list().await {
            let mut best: Option<PersonaInfo> = None;
            for p in &personas {
                let pname = p.name.trim();
                if !pname.is_empty() && trimmed.starts_with(pname) {
                    match &best {
                        Some(current) if pname.len() > current.name.len() => best = Some(p.clone()),
                        None => best = Some(p.clone()),
                        _ => {}
                    }
                }
            }
            if let Some(p) = best {
                return PersonaResolution::Manual(p);
            }
        }

        // 2. 沿用会话当前的 persona
        if let Some(active_id) = active_persona_id {
            if let Ok(p) = self.get(active_id).await {
                return PersonaResolution::Auto(p);
            }
        }

        // 3. 按项目识别
        if let Some(path) = project_path {
            if let Ok(Some(p)) = self.detect_for_project(path).await {
                return PersonaResolution::Auto(p);
            }
        }

        // 4. 回退到默认 persona
        if let Ok(Some(p)) = self.get_default().await {
            return PersonaResolution::Default(p);
        }

        PersonaResolution::None
    }
}
