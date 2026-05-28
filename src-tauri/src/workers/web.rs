use async_trait::async_trait;

use crate::error::Result;
use crate::workers::{SubTask, WorkerAgent, WorkerKind, WorkerResult};

pub struct WebWorker;

impl WebWorker {
    /// Execute @@search(query) and @@fetch(url) commands embedded in the instruction.
    async fn execute_tools(instruction: &str) -> String {
        let mut result = String::new();

        for line in instruction.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with("@@search(") && trimmed.ends_with(')') {
                let query = &trimmed[9..trimmed.len() - 1].trim().trim_matches('"');
                if query.is_empty() {
                    result.push_str("Search query is empty.\n");
                    continue;
                }
                let encoded = urlencoding::encode(query);
                let url = format!(
                    "https://api.duckduckgo.com/?q={}&format=json&no_html=1&skip_disambig=1",
                    encoded
                );

                match reqwest::get(&url).await {
                    Ok(resp) => match resp.text().await {
                        Ok(text) => {
                            let preview: String = text.chars().take(3000).collect();
                            result.push_str(&format!(
                                "Search results for '{}':\n{}\n",
                                query, preview
                            ));
                        }
                        Err(e) => {
                            result.push_str(&format!("Search error reading body: {}\n", e))
                        }
                    },
                    Err(e) => {
                        result.push_str(&format!("Search error connecting: {}\n", e))
                    }
                }
            } else if trimmed.starts_with("@@fetch(") && trimmed.ends_with(')') {
                let url = trimmed[8..trimmed.len() - 1].trim().trim_matches('"');

                match reqwest::get(url).await {
                    Ok(resp) => match resp.text().await {
                        Ok(text) => {
                            let preview: String = text.chars().take(2000).collect();
                            result.push_str(&format!("Content from {}:\n{}\n", url, preview));
                        }
                        Err(e) => {
                            result.push_str(&format!("Fetch error reading body: {}\n", e))
                        }
                    },
                    Err(e) => {
                        result.push_str(&format!("Fetch error connecting: {}\n", e))
                    }
                }
            }
        }

        result
    }
}

#[async_trait]
impl WorkerAgent for WebWorker {
    fn kind(&self) -> WorkerKind {
        WorkerKind::Web
    }

    fn description(&self) -> &str {
        "Searches the web and fetches URL content."
    }

    async fn execute(&self, task: SubTask) -> Result<WorkerResult> {
        let start = std::time::Instant::now();
        let tool_results = Self::execute_tools(&task.instruction).await;

        Ok(WorkerResult {
            worker: WorkerKind::Web,
            task_id: task.id,
            content: if tool_results.is_empty() {
                "No @@search or @@fetch commands found in instruction.".to_string()
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

    #[tokio::test]
    async fn test_web_kind() {
        let worker = WebWorker;
        assert_eq!(worker.kind(), WorkerKind::Web);
    }

    #[tokio::test]
    async fn test_execute_search() {
        let instruction = "@@search(\"Rust programming\")\n";
        let result = WebWorker::execute_tools(instruction).await;
        assert!(!result.is_empty(), "search should return something");
        assert!(
            result.contains("error") || result.len() > 10,
            "should have results or at least a clear error"
        );
    }

    #[tokio::test]
    async fn test_execute_fetch() {
        let instruction = "@@fetch(\"https://example.com\")\n";
        let result = WebWorker::execute_tools(instruction).await;
        assert!(!result.is_empty(), "fetch should return something");
    }

    #[tokio::test]
    async fn test_execute_no_commands() {
        let instruction = "Just a plain instruction without tool commands.";
        let result = WebWorker::execute_tools(instruction).await;
        assert!(result.is_empty(), "should return empty when no @@ commands");
    }

    #[tokio::test]
    async fn test_execute_search_empty() {
        let instruction = "@@search(\"\")\n";
        let result = WebWorker::execute_tools(instruction).await;
        assert!(!result.is_empty(), "should handle empty query gracefully");
        assert!(result.contains("empty"), "should mention empty query");
    }

    #[tokio::test]
    async fn test_execute_fetch_error() {
        let instruction = "@@fetch(\"https://nonexistent.example.com\")\n";
        let result = WebWorker::execute_tools(instruction).await;
        assert!(
            result.contains("error") || result.contains("Error"),
            "should report fetch error"
        );
    }
}
