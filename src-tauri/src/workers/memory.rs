use async_trait::async_trait;
use chrono::Utc;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::db::models::MemoryRecord;
use crate::db::repository::Database;
use crate::error::Result;
use crate::workers::{SubTask, WorkerAgent, WorkerKind, WorkerResult};

/// A worker that stores and retrieves memories directly via the SQLite database.
///
/// Unlike the high-level [`crate::memory::MemoryManager`], this worker operates
/// at the storage layer — it does not use embeddings or vector search.
///
/// ## Commands
///
/// - `@@store("content")` — store a new memory
/// - `@@store("content", "tag1,tag2")` — store with tags
/// - `@@search("query")` — search memories by FTS5 full-text match
/// - `@@search("query", 5)` — search with custom limit (default 10)
/// - `@@list()` — list recent memories (default 10)
/// - `@@list(20)` — list with custom limit
pub struct MemoryWorker {
    db: Arc<Mutex<Database>>,
}

impl MemoryWorker {
    pub fn new(db: Arc<Mutex<Database>>) -> Self {
        Self { db }
    }

    /// Parse and execute @@store, @@search, and @@list commands from the instruction.
    async fn execute_tools(&self, instruction: &str) -> String {
        let mut result = String::new();

        for line in instruction.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if trimmed.starts_with("@@store(") && trimmed.ends_with(')') {
                self.handle_store(&trimmed[8..trimmed.len() - 1], &mut result).await;
            } else if trimmed.starts_with("@@search(") && trimmed.ends_with(')') {
                self.handle_search(&trimmed[9..trimmed.len() - 1], &mut result).await;
            } else if trimmed.starts_with("@@list") && trimmed.ends_with(')') {
                self.handle_list(&trimmed[7..trimmed.len() - 1], &mut result).await;
            }
        }

        result
    }

    /// Handle `@@store(content)` or `@@store(content, tags)`.
    async fn handle_store(&self, args: &str, output: &mut String) {
        let (content, tags_opt) = Self::parse_store_args(args);

        if content.is_empty() {
            output.push_str("Store error: content cannot be empty.\n");
            return;
        }

        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        let record = MemoryRecord {
            id,
            content: content.to_string(),
            memory_type: "fact".into(),
            scope: "global".into(),
            source: "memory_worker".into(),
            relevance: 1.0,
            tags: tags_opt,
            created_at: now.clone(),
            updated_at: now.clone(),
            last_accessed_at: now,
            access_count: 0,
        };

        match self.db.lock().await.insert_memory(&record) {
            Ok(_) => {
                let preview: String = content.chars().take(100).collect();
                output.push_str(&format!("Stored memory: {}\n", preview));
            }
            Err(e) => output.push_str(&format!("Store error: {}\n", e)),
        }
    }

    /// Parse store arguments: `"content"` or `"content", "tags"`.
    fn parse_store_args(args: &str) -> (&str, Option<String>) {
        let trimmed = args.trim();
        if let Some(comma) = trimmed.find(',') {
            let content = trimmed[..comma].trim().trim_matches('"');
            let tags_raw = trimmed[comma + 1..].trim().trim_matches('"');
            let tags = if tags_raw.is_empty() {
                None
            } else {
                Some(tags_raw.to_string())
            };
            (content, tags)
        } else {
            (trimmed.trim_matches('"'), None)
        }
    }

    /// Handle `@@search(query)` or `@@search(query, limit)`.
    async fn handle_search(&self, args: &str, output: &mut String) {
        let (query, limit) = Self::parse_search_args(args);

        if query.is_empty() {
            output.push_str("Search query is empty.\n");
            return;
        }

        match self.db.lock().await.search_memories(query, None, None) {
            Ok(records) => {
                let shown: Vec<_> = records.iter().take(limit).collect();
                if shown.is_empty() {
                    output.push_str("No memories found matching the query.\n");
                } else {
                    for mem in &shown {
                        let preview: String = mem.content.chars().take(200).collect();
                        let id_preview: &str = if mem.id.len() > 8 {
                            &mem.id[..8]
                        } else {
                            &mem.id
                        };
                        let date_preview: &str = if mem.created_at.len() >= 10 {
                            &mem.created_at[..10]
                        } else {
                            &mem.created_at
                        };
                        output.push_str(&format!(
                            "- [{}] {} (relevance: {:.2}, {})\n",
                            id_preview, preview, mem.relevance, date_preview,
                        ));
                    }
                }
            }
            Err(e) => output.push_str(&format!("Search error: {}\n", e)),
        }
    }

    /// Parse search arguments: `"query"` or `"query", 5`.
    fn parse_search_args(args: &str) -> (&str, usize) {
        let trimmed = args.trim();
        if let Some(comma) = trimmed.find(',') {
            let query = trimmed[..comma].trim().trim_matches('"');
            let limit = trimmed[comma + 1..].trim().parse::<usize>().unwrap_or(10);
            (query, limit)
        } else {
            (trimmed.trim_matches('"'), 10)
        }
    }

    /// Handle `@@list()` or `@@list(limit)`.
    async fn handle_list(&self, args: &str, output: &mut String) {
        let limit_str = args.trim();
        let limit: usize = if limit_str.is_empty() {
            10
        } else {
            limit_str.parse().unwrap_or(10)
        };

        match self.db.lock().await.list_memories() {
            Ok(records) => {
                if records.is_empty() {
                    output.push_str("No memories stored yet.\n");
                    return;
                }
                // Show most recent first (list_memories returns by relevance DESC,
                // so we reverse to get newest first, then take limit, then re-reverse
                // to preserve chronological order within the window).
                let shown: Vec<_> = records.iter().rev().take(limit).collect();
                for mem in shown.iter().rev() {
                    let preview: String = mem.content.chars().take(200).collect();
                    let id_preview: &str = if mem.id.len() > 8 {
                        &mem.id[..8]
                    } else {
                        &mem.id
                    };
                    let date_preview: &str = if mem.created_at.len() >= 10 {
                        &mem.created_at[..10]
                    } else {
                        &mem.created_at
                    };
                    output.push_str(&format!(
                        "- [{}] {} (created: {})\n",
                        id_preview, preview, date_preview,
                    ));
                }
            }
            Err(e) => output.push_str(&format!("List error: {}\n", e)),
        }
    }
}

#[async_trait]
impl WorkerAgent for MemoryWorker {
    fn kind(&self) -> WorkerKind {
        WorkerKind::Memory
    }

    fn description(&self) -> &str {
        "Stores and retrieves memories using @@store, @@search, and @@list commands."
    }

    async fn execute(&self, task: SubTask) -> Result<WorkerResult> {
        let start = std::time::Instant::now();
        let tool_results = self.execute_tools(&task.instruction).await;

        Ok(WorkerResult {
            worker: WorkerKind::Memory,
            task_id: task.id,
            content: if tool_results.is_empty() {
                "No @@store, @@search, or @@list commands found.".to_string()
            } else {
                tool_results
            },
            metadata: None,
            duration_ms: Some(start.elapsed().as_millis() as u64),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_db() -> Arc<Mutex<Database>> {
        let db = Database::new_test().expect("Failed to create test database");
        Arc::new(Mutex::new(db))
    }

    #[tokio::test]
    async fn test_memory_kind() {
        let worker = MemoryWorker::new(create_test_db());
        assert_eq!(worker.kind(), WorkerKind::Memory);
        assert!(!worker.description().is_empty());
    }

    #[tokio::test]
    async fn test_store_empty_content() {
        let worker = MemoryWorker::new(create_test_db());
        let result = worker.execute_tools("@@store(\"\")\n").await;
        assert!(
            result.contains("cannot be empty"),
            "should reject empty content: {}",
            result
        );
    }

    #[tokio::test]
    async fn test_store_and_search() {
        let worker = MemoryWorker::new(create_test_db());
        worker.execute_tools("@@store(\"rust is fast and safe\")\n").await;
        let result = worker.execute_tools("@@search(\"rust\")\n").await;
        assert!(
            result.contains("rust is fast"),
            "should find stored memory: {}",
            result
        );
    }

    #[tokio::test]
    async fn test_store_with_tags() {
        let worker = MemoryWorker::new(create_test_db());
        worker.execute_tools("@@store(\"tagged memory\", \"test,example\")\n").await;
        let result = worker.execute_tools("@@search(\"tagged\")\n").await;
        assert!(
            result.contains("tagged memory"),
            "should find tagged memory: {}",
            result
        );
    }

    #[tokio::test]
    async fn test_list() {
        let worker = MemoryWorker::new(create_test_db());
        worker.execute_tools("@@store(\"memory one\")\n").await;
        worker.execute_tools("@@store(\"memory two\")\n").await;
        let result = worker.execute_tools("@@list(10)\n").await;
        assert!(
            result.contains("memory one"),
            "list should contain memory one: {}",
            result
        );
        assert!(
            result.contains("memory two"),
            "list should contain memory two: {}",
            result
        );
    }

    #[tokio::test]
    async fn test_list_default_limit() {
        let worker = MemoryWorker::new(create_test_db());
        worker.execute_tools("@@store(\"first\")\n").await;
        let result = worker.execute_tools("@@list()\n").await;
        assert!(
            result.contains("first"),
            "list() without args should work: {}",
            result
        );
    }

    #[tokio::test]
    async fn test_no_commands() {
        let worker = MemoryWorker::new(create_test_db());
        let result = worker.execute_tools("Just a plain instruction.\n").await;
        assert!(result.is_empty(), "should return empty when no commands");
    }

    #[tokio::test]
    async fn test_search_no_match() {
        let worker = MemoryWorker::new(create_test_db());
        worker.execute_tools("@@store(\"unique content\")\n").await;
        let result = worker.execute_tools("@@search(\"nonexistent\")\n").await;
        assert!(
            result.contains("No memories found"),
            "should report no matches: {}",
            result
        );
    }

    #[tokio::test]
    async fn test_search_empty_query() {
        let worker = MemoryWorker::new(create_test_db());
        let result = worker.execute_tools("@@search(\"\")\n").await;
        assert!(
            result.contains("empty"),
            "should handle empty query: {}",
            result
        );
    }

    #[tokio::test]
    async fn test_list_empty_db() {
        let worker = MemoryWorker::new(create_test_db());
        let result = worker.execute_tools("@@list(10)\n").await;
        assert!(
            result.contains("No memories"),
            "should report empty: {}",
            result
        );
    }

    #[tokio::test]
    async fn test_execute_task() {
        let worker = MemoryWorker::new(create_test_db());
        let task = SubTask {
            id: "test_1".into(),
            label: "Store something".into(),
            instruction: "@@store(\"task memory\")\n".into(),
            worker_kind: WorkerKind::Memory,
            model_id: None,
            max_tokens: None,
            temperature: None,
            context: None,
        };
        let wr = worker.execute(task).await.expect("execute should succeed");
        assert_eq!(wr.worker, WorkerKind::Memory);
        assert!(wr.content.contains("Stored memory"));
        assert!(wr.duration_ms.is_some());
    }
}
