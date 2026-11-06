//! 记忆管理。
//!
//! 记忆持久化在 SQLite，同时可选用 Rig 的 embedding 服务构建内存向量索引
//! 做语义检索。没有 API key 或 embedding 失败时自动退化为 SQLite 关键词
//! 检索，功能不中断，只是召回质量下降。

use std::sync::Arc;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::core::db::models::MemoryRecord;
use crate::core::db::repository::Database;
use crate::core::error::{AppError, Result};

// 与意图召回共用的 Rig embedding 边界。
use crate::llm::embedding::EmbeddingService;

pub mod seeds;
pub mod vector_index;

use vector_index::InMemoryVectorIndex;

/// 返回给前端的轻量记忆信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryInfo {
    pub id: String,
    pub content: String,
    pub memory_type: String,
    pub scope: String,
    pub source: String,
    pub relevance: f64,
    pub tags: Option<Vec<String>>,
    pub created_at: String,
    pub updated_at: String,
    pub last_accessed_at: String,
    pub access_count: i32,
}

impl From<MemoryRecord> for MemoryInfo {
    fn from(r: MemoryRecord) -> Self {
        Self {
            id: r.id,
            content: r.content,
            memory_type: r.memory_type,
            scope: r.scope,
            source: r.source,
            relevance: r.relevance,
            tags: r.tags.as_ref().map(|t| {
                t.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            }),
            created_at: r.created_at,
            updated_at: r.updated_at,
            last_accessed_at: r.last_accessed_at,
            access_count: r.access_count,
        }
    }
}

/// 创建记忆的入参；缺省值由 `default_*` 函数补齐。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMemoryParams {
    pub content: String,
    #[serde(default = "default_memory_type")]
    pub memory_type: String,
    #[serde(default = "default_scope")]
    pub scope: String,
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default = "default_relevance")]
    pub relevance: f64,
    pub tags: Option<Vec<String>>,
}

fn default_memory_type() -> String { "fact".to_string() }
fn default_scope() -> String { "global".to_string() }
fn default_source() -> String { "manual".to_string() }
fn default_relevance() -> f64 { 1.0 }

/// 更新记忆的入参；`None` 表示该字段保持不变。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateMemoryParams {
    pub content: Option<String>,
    pub memory_type: Option<String>,
    pub scope: Option<String>,
    pub relevance: Option<f64>,
    pub tags: Option<Option<Vec<String>>>,
}

/// 记忆管理器：增删改查、检索与上下文注入。
///
/// 写入路径会尽量为记忆生成 embedding 并同步进内存向量索引；读取路径
/// （[`MemoryManager::retrieve_relevant`]）优先走向量检索，索引为空或
/// embedding 不可用时退化为 SQLite 关键词检索。
///
/// 向量索引不持久化：进程重启后首次检索会从 SQLite 懒加载重建。
pub struct MemoryManager {
    db: Arc<Mutex<Database>>,
    embedder: Option<Arc<EmbeddingService>>,
    vector_index: Arc<Mutex<InMemoryVectorIndex>>,
}

impl MemoryManager {
    /// 用共享数据库句柄与可选的 embedding 服务构造管理器。
    ///
    /// `embedder` 为 `None` 时所有检索都走关键词回退路径。
    pub fn new(db: Arc<Mutex<Database>>, embedder: Option<Arc<EmbeddingService>>) -> Self {
        Self {
            db,
            embedder,
            vector_index: Arc::new(Mutex::new(InMemoryVectorIndex::new())),
        }
    }

    /// 返回当前可用的 embedding 服务（已配置且标记为可用）。
    fn active_embedder(&self) -> Option<&EmbeddingService> {
        self.embedder
            .as_deref()
            .filter(|embedder| embedder.available())
    }

    /// 从数据库懒加载重建向量索引；已有条目或无 embedding 服务时直接返回。
    ///
    /// 重建期间会先释放数据库锁再发起网络请求，避免长时间占用。
    async fn ensure_index_built(&self) -> Result<()> {
        let Some(embedder) = self.active_embedder() else {
            return Ok(()); // 没有可用的 embedding 服务
        };

        let mut index = self.vector_index.lock().await;
        if !index.is_empty() {
            return Ok(()); // 已经构建过
        }

        let db = self.db.lock().await;
        let records = db.list_memories()?;
        drop(db); // 网络调用前先释放锁

        for record in &records {
            if let Some(vec_f32) = embedder.embed(&record.content).await {
                index.insert(record.id.clone(), record.content.clone(), vec_f32);
            }
        }

        log::info!("MemoryManager: rebuilt index with {} entries", index.len());
        Ok(())
    }

    /// 创建一条记忆。
    ///
    /// 先写 SQLite（失败则整体失败），再尽力生成 embedding 并写入向量
    /// 索引；embedding 失败不回滚，只是该条记忆暂时无法被语义检索命中。
    pub async fn create(&self, params: CreateMemoryParams) -> Result<MemoryInfo> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        if params.content.trim().is_empty() {
            return Err(AppError::InvalidInput("Memory content cannot be empty".to_string()));
        }

        let valid_types = ["fact", "preference", "project_context", "user_info", "conversation_summary"];
        if !valid_types.contains(&params.memory_type.as_str()) {
            return Err(AppError::InvalidInput(format!(
                "Invalid memory_type '{}'. Must be one of: {:?}",
                params.memory_type, valid_types
            )));
        }

        let tags_str = params.tags.map(|t| t.join(","));

        let record = MemoryRecord {
            id: id.clone(),
            content: params.content,
            memory_type: params.memory_type,
            scope: params.scope,
            source: params.source,
            relevance: params.relevance.clamp(0.0, 1.0),
            tags: tags_str,
            created_at: now.clone(),
            updated_at: now.clone(),
            last_accessed_at: now,
            access_count: 0,
        };

        let db = self.db.lock().await;
        db.insert_memory(&record)?;
        drop(db);

        // 生成 embedding 并写入向量索引（尽力而为）。
        if let Some(embedder) = self.active_embedder() {
            if let Some(vec_f32) = embedder.embed(&record.content).await {
                let mut index = self.vector_index.lock().await;
                index.insert(record.id.clone(), record.content.clone(), vec_f32);
            }
        }

        Ok(MemoryInfo::from(record))
    }


    /// 按 id 获取一条记忆。
    ///
    /// 作为读取副作用会递增 `access_count` 并刷新 `last_accessed_at`。
    pub async fn get(&self, id: &str) -> Result<MemoryInfo> {
        let db = self.db.lock().await;
        let record = db.get_memory(id)?
            .ok_or_else(|| AppError::NotFound(format!("Memory '{}' not found", id)))?;
        // 记录一次访问
        db.touch_memory(id)?;
        Ok(MemoryInfo::from(record))
    }

    /// 列出全部记忆（不改变访问统计）。
    pub async fn list(&self) -> Result<Vec<MemoryInfo>> {
        let db = self.db.lock().await;
        let records = db.list_memories()?;
        Ok(records.into_iter().map(MemoryInfo::from).collect())
    }

    /// 按关键词与可选类型/作用域过滤做 SQL 检索。
    ///
    /// 这是纯数据库查询，不触发 embedding，也不影响向量索引。
    pub async fn search(
        &self,
        query: &str,
        memory_type: Option<&str>,
        scope: Option<&str>,
    ) -> Result<Vec<MemoryInfo>> {
        let db = self.db.lock().await;
        let records = db.search_memories(query, memory_type, scope)?;
        Ok(records.into_iter().map(MemoryInfo::from).collect())
    }

    /// 召回与给定上下文相关的记忆，供 agent 注入。
    ///
    /// 有可用向量索引时按余弦相似度取 top-k；否则回退到 SQLite 关键词
    /// 检索。两条路径都会递增命中记忆的访问统计。
    pub async fn retrieve_relevant(&self, context: &str, limit: i64) -> Result<Vec<MemoryInfo>> {
        if context.trim().is_empty() {
            return Ok(vec![]);
        }

        self.ensure_index_built().await?;

        // 语义检索路径
        if let Some(embedder) = self.active_embedder() {
            let index = self.vector_index.lock().await;
            if !index.is_empty() {
                if let Some(query_vec) = embedder.embed(context).await {
                    let results = index.search(&query_vec, limit as usize);
                    if !results.is_empty() {
                        let ids: Vec<&str> = results.iter().map(|(id, _)| id.as_str()).collect();
                        let db = self.db.lock().await;
                        let mut records = Vec::with_capacity(results.len());
                        for id in ids {
                            if let Some(record) = db.get_memory(id)? {
                                records.push(MemoryInfo::from(record));
                            }
                        }
                        // 记录访问
                        for r in &records {
                            let _ = db.touch_memory(&r.id);
                        }
                        return Ok(records);
                    }
                }
            }
        }

        // 回退到 SQLite 关键词检索
        let db = self.db.lock().await;
        let records = db.retrieve_relevant(context, limit)?;
        for r in &records {
            let _ = db.touch_memory(&r.id);
        }
        Ok(records.into_iter().map(MemoryInfo::from).collect())
    }

    /// 更新一条记忆。
    ///
    /// 内容变更时会重新生成 embedding 并覆盖向量索引中的旧条目；内容不变
    /// 则不动索引。
    pub async fn update(&self, id: &str, params: UpdateMemoryParams) -> Result<MemoryInfo> {
        let db = self.db.lock().await;
        let mut record = db.get_memory(id)?
            .ok_or_else(|| AppError::NotFound(format!("Memory '{}' not found", id)))?;

        let content_updated = params.content.is_some();
        if let Some(content) = params.content {
            if content.trim().is_empty() {
                return Err(AppError::InvalidInput("Memory content cannot be empty".to_string()));
            }
            record.content = content;
        }
        if let Some(memory_type) = params.memory_type {
            let valid_types = ["fact", "preference", "project_context", "user_info", "conversation_summary"];
            if !valid_types.contains(&memory_type.as_str()) {
                return Err(AppError::InvalidInput(format!(
                    "Invalid memory_type '{}'. Must be one of: {:?}",
                    memory_type, valid_types
                )));
            }
            record.memory_type = memory_type;
        }
        if let Some(scope) = params.scope {
            record.scope = scope;
        }
        if let Some(relevance) = params.relevance {
            record.relevance = relevance.clamp(0.0, 1.0);
        }
        if let Some(tags) = params.tags {
            record.tags = tags.map(|t| t.join(","));
        }
        record.updated_at = Utc::now().to_rfc3339();

        let content_changed = content_updated;
        db.update_memory(&record)?;
        drop(db);

        // 内容变化时重新生成 embedding
        if content_changed {
            if let Some(embedder) = self.active_embedder() {
                if let Some(vec_f32) = embedder.embed(&record.content).await {
                    let mut index = self.vector_index.lock().await;
                    index.insert(record.id.clone(), record.content.clone(), vec_f32);
                }
            }
        }

        Ok(MemoryInfo::from(record))
    }

    /// 按 id 删除记忆，并同步从向量索引移除。
    pub async fn delete(&self, id: &str) -> Result<()> {
        let db = self.db.lock().await;
        // 先确认存在，避免静默删除不存在的 id
        let _ = db.get_memory(id)?
            .ok_or_else(|| AppError::NotFound(format!("Memory '{}' not found", id)))?;
        db.delete_memory(id)?;
        drop(db);

        // 从向量索引移除
        let mut index = self.vector_index.lock().await;
        index.remove(id);

        Ok(())
    }

    /// 表为空时写入内置默认记忆。
    ///
    /// 只在首次启动生效，已有任何记忆时返回 0，避免覆盖用户数据。
    pub async fn seed_defaults(&self) -> Result<usize> {
        let records = seeds::default_seed_memories();
        let db = self.db.lock().await;

        // 表中已有数据则跳过
        let existing = db.list_memories()?;
        if !existing.is_empty() {
            return Ok(0); // 已经初始化过
        }

        let mut count = 0;
        for record in &records {
            match db.insert_memory(record) {
                Ok(()) => count += 1,
                Err(e) => log::warn!("Failed to seed memory '{}': {}", record.id, e),
            }
        }

        if count > 0 {
            log::info!("Seeded {} built-in memories", count);
        }
        Ok(count)
    }

    /// 把最相关的记忆拼成系统提示词片段，即注入 agent 上下文的内容。
    ///
    /// 没有命中任何记忆时返回 `None`，调用方无需拼接空片段。
    pub async fn build_context_prompt(&self, user_message: &str, max_memories: i64) -> Result<Option<String>> {
        let relevant = self.retrieve_relevant(user_message, max_memories).await?;
        if relevant.is_empty() {
            return Ok(None);
        }

        let mut lines = vec![
            "\n<remembered_context>".to_string(),
            "The following information has been remembered from previous interactions:".to_string(),
        ];

        for mem in &relevant {
            let icon = match mem.memory_type.as_str() {
                "preference" => "⚙️",
                "user_info" => "👤",
                "project_context" => "📁",
                "conversation_summary" => "📝",
                _ => "💡",
            };
            lines.push(format!("{icon} ({}) {}", mem.memory_type, mem.content));
        }

        lines.push("</remembered_context>".to_string());
        Ok(Some(lines.join("\n")))
    }
}
