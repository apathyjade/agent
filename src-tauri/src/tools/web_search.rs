use async_trait::async_trait;
use serde_json::{json, Value};

use crate::error::{AppError, Result};

use super::r#trait::Tool;

pub struct WebSearchTool;

impl WebSearchTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the web for information. Returns relevant results based on the query."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query"
                },
                "num_results": {
                    "type": "integer",
                    "description": "Number of results to return (default: 5)"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let query = input["query"]
            .as_str()
            .ok_or_else(|| AppError::InvalidInput("Missing 'query' parameter".to_string()))?;

        let num_results = input["num_results"].as_u64().unwrap_or(5) as usize;

        let client = reqwest::Client::new();
        let url = format!(
            "https://html.duckduckgo.com/html/?q={}",
            urlencoding::encode(query)
        );

        let response = client
            .get(&url)
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .send()
            .await
            .map_err(|e| AppError::Tool(format!("Search request failed: {}", e)))?;

        let html = response
            .text()
            .await
            .map_err(|e| AppError::Tool(format!("Failed to read response: {}", e)))?;

        let results = Self::parse_results(&html, num_results);

        Ok(json!({
            "query": query,
            "results": results,
            "count": results.len()
        }))
    }
}

impl WebSearchTool {
    fn parse_results(html: &str, max_results: usize) -> Vec<Value> {
        let mut results = Vec::new();
        let mut lines = html.lines();

        while let Some(line) = lines.next() {
            if line.contains("result__a") {
                if let Some(url) = Self::extract_href(line) {
                    if let Some(title) = Self::extract_text_between(line, ">", "</a>") {
                        let mut snippet = String::new();
                        for next_line in lines.by_ref() {
                            if next_line.contains("result__snippet") {
                                snippet = Self::extract_text_between(next_line, ">", "</span>")
                                    .unwrap_or_default();
                                break;
                            }
                        }

                        results.push(json!({
                            "title": title,
                            "url": url,
                            "snippet": snippet
                        }));

                        if results.len() >= max_results {
                            break;
                        }
                    }
                }
            }
        }

        results
    }

    fn extract_href(line: &str) -> Option<String> {
        let start = line.find("href=\"")?;
        let rest = &line[start + 6..];
        let end = rest.find('"')?;
        let url = &rest[..end];

        if url.starts_with("//duckduckgo.com") || url.starts_with("/l/") {
            if let Some(uddg) = url.find("uddg=") {
                let encoded = &url[uddg + 5..];
                return Some(urlencoding::decode(encoded).ok()?.to_string());
            }
        }

        Some(url.to_string())
    }

    fn extract_text_between(line: &str, start_marker: &str, end_marker: &str) -> Option<String> {
        let start = line.find(start_marker)? + start_marker.len();
        let rest = &line[start..];
        let end = rest.find(end_marker)?;
        let text = &rest[..end];
        Some(Self::clean_html(text))
    }

    fn clean_html(text: &str) -> String {
        text.replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#x27;", "'")
            .replace("&nbsp;", " ")
            .replace("<[^>]*>", "")
    }
}
