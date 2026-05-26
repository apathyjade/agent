use std::sync::Arc;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::db::models::MemoryRecord;
use crate::db::repository::Database;
use crate::error::{AppError, Result};

// Rig imports for embedding support
use rig::client::EmbeddingsClient;
use rig::embeddings::EmbeddingModel;

pub mod seeds;
pub mod vector_index;

use vector_index::InMemoryVectorIndex;

/// Lightweight memory info returned to the frontend.
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

/// Parameters for creating a new memory.
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

/// Parameters for updating a memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateMemoryParams {
    pub content: Option<String>,
    pub memory_type: Option<String>,
    pub scope: Option<String>,
    pub relevance: Option<f64>,
    pub tags: Option<Option<Vec<String>>>,
}

/// Manages memory entries — CRUD, retrieval, and context injection.
///
/// When an OpenAI API key is available, memories are also embedded and
/// stored in an in-memory vector index for semantic retrieval.  If no API
/// key is provided (or embedding fails), the system gracefully degrades
/// to SQLite keyword search.
pub struct MemoryManager {
    db: Arc<Mutex<Database>>,
    embedder: Option<rig::providers::openai::EmbeddingModel>,
    vector_index: Arc<Mutex<InMemoryVectorIndex>>,
}

impl MemoryManager {
    /// Create a new memory manager.
    ///
    /// If `openai_api_key` is `Some`, the manager will attempt to initialise
    /// a Rig embedding model and rebuild the vector index from existing
    /// memories on first `retrieve_relevant` call.
    pub fn new(db: Arc<Mutex<Database>>, openai_api_key: Option<String>) -> Self {
        let embedder = openai_api_key.and_then(|key| {
            match rig::providers::openai::Client::new(&key) {
                Ok(client) => {
                    let model = client.embedding_model(
                        rig::providers::openai::TEXT_EMBEDDING_3_SMALL,
                    );
                    log::info!("MemoryManager: embedding model initialised");
                    Some(model)
                }
                Err(e) => {
                    log::warn!("MemoryManager: failed to init embedder: {}", e);
                    None
                }
            }
        });

        Self {
            db,
            embedder,
            vector_index: Arc::new(Mutex::new(InMemoryVectorIndex::new())),
        }
    }

    /// Lazily rebuild the vector index from the database.
    async fn ensure_index_built(&self) -> Result<()> {
        let embedder = match &self.embedder {
            Some(e) => e,
            None => return Ok(()), // no embedder available
        };

        let mut index = self.vector_index.lock().await;
        if !index.is_empty() {
            return Ok(()); // already built
        }

        let db = self.db.lock().await;
        let records = db.list_memories()?;
        drop(db); // release lock before network calls

        for record in &records {
                match embedder.embed_text(&record.content).await {
                    Ok(embedding) => {
                        let vec_f32: Vec<f32> = embedding.vec.iter().map(|&x| x as f32).collect();
                        index.insert(record.id.clone(), record.content.clone(), vec_f32);
                    }
                    Err(e) => {
                        log::warn!("MemoryManager: failed to embed '{}': {}", record.id, e);
                    }
                }
            }

            log::info!("MemoryManager: rebuilt index with {} entries", index.len());
        Ok(())
    }

    /// Create a new memory entry.
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

        // Compute embedding and add to vector index (best-effort)
        if let Some(ref embedder) = self.embedder {
            match embedder.embed_text(&record.content).await {
                Ok(embedding) => {
                    let vec_f32: Vec<f32> = embedding.vec.iter().map(|&x| x as f32).collect();
                    let mut index = self.vector_index.lock().await;
                    index.insert(record.id.clone(), record.content.clone(), vec_f32);
                }
                Err(e) => {
                    log::warn!("MemoryManager: failed to embed new memory: {}", e);
                }
            }
        }

        Ok(MemoryInfo::from(record))
    }


    /// Get a single memory by ID.
    pub async fn get(&self, id: &str) -> Result<MemoryInfo> {
        let db = self.db.lock().await;
        let record = db.get_memory(id)?
            .ok_or_else(|| AppError::NotFound(format!("Memory '{}' not found", id)))?;
        // Bump access
        db.touch_memory(id)?;
        Ok(MemoryInfo::from(record))
    }

    /// List all memories.
    pub async fn list(&self) -> Result<Vec<MemoryInfo>> {
        let db = self.db.lock().await;
        let records = db.list_memories()?;
        Ok(records.into_iter().map(MemoryInfo::from).collect())
    }

    /// Search memories by keyword and optional filters.
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

    /// Retrieve memories relevant to a context string (for agent injection).
    ///
    /// Uses semantic search when the vector index is available, otherwise
    /// falls back to SQLite keyword search.
    pub async fn retrieve_relevant(&self, context: &str, limit: i64) -> Result<Vec<MemoryInfo>> {
        if context.trim().is_empty() {
            return Ok(vec![]);
        }

        self.ensure_index_built().await?;

        // Semantic search path
        if let Some(ref embedder) = self.embedder {
            let index = self.vector_index.lock().await;
            if !index.is_empty() {
                match embedder.embed_text(context).await {
                    Ok(query_emb) => {
                        let query_vec: Vec<f32> = query_emb.vec.iter().map(|&x| x as f32).collect();
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
                            // Bump access
                            for r in &records {
                                let _ = db.touch_memory(&r.id);
                            }
                            return Ok(records);
                        }
                    }
                    Err(e) => {
                        log::warn!("MemoryManager: embedding failed, falling back: {}", e);
                    }
                }
            }
        }

        // Fallback to SQLite keyword search
        let db = self.db.lock().await;
        let records = db.retrieve_relevant(context, limit)?;
        for r in &records {
            let _ = db.touch_memory(&r.id);
        }
        Ok(records.into_iter().map(MemoryInfo::from).collect())
    }

    /// Update an existing memory.
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

        // Re-embed if content changed
        if content_changed {
            if let Some(ref embedder) = self.embedder {
                match embedder.embed_text(&record.content).await {
                    Ok(embedding) => {
                        let vec_f32: Vec<f32> = embedding.vec.iter().map(|&x| x as f32).collect();
                        let mut index = self.vector_index.lock().await;
                        index.insert(record.id.clone(), record.content.clone(), vec_f32);
                    }
                    Err(e) => {
                        log::warn!("MemoryManager: failed to re-embed '{}': {}", record.id, e);
                    }
                }
            }
        }

        Ok(MemoryInfo::from(record))
    }

    /// Delete a memory by ID.
    pub async fn delete(&self, id: &str) -> Result<()> {
        let db = self.db.lock().await;
        // Check existence first
        let _ = db.get_memory(id)?
            .ok_or_else(|| AppError::NotFound(format!("Memory '{}' not found", id)))?;
        db.delete_memory(id)?;
        drop(db);

        // Remove from vector index
        let mut index = self.vector_index.lock().await;
        index.remove(id);

        Ok(())
    }

    /// Seed the database with built-in default memories if the table is empty.
    /// Called once on first launch to give the agent useful baseline knowledge.
    pub async fn seed_defaults(&self) -> Result<usize> {
        let records = seeds::default_seed_memories();
        let db = self.db.lock().await;

        // Check if table is already populated
        let existing = db.list_memories()?;
        if !existing.is_empty() {
            return Ok(0); // Already seeded
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

    /// Build a system-prompt fragment from the most relevant memories.
    /// This is what gets injected into the agent's context.
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
