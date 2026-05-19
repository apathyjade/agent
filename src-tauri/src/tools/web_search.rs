use async_trait::async_trait;
use serde_json::{json, Value};
use std::time::Duration;

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
                    "description": "Number of results to return (default: 5, max: 10)"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let query = input["query"]
            .as_str()
            .ok_or_else(|| AppError::InvalidInput("Missing 'query' parameter".to_string()))?;

        let num_results = input["num_results"]
            .as_u64()
            .unwrap_or(5)
            .min(10) as usize;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| AppError::Tool(format!("Failed to create HTTP client: {}", e)))?;

        let url = format!(
            "https://html.duckduckgo.com/html/?q={}",
            urlencoding::encode(query)
        );

        let response = client
            .get(&url)
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .header("Accept", "text/html,application/xhtml+xml")
            .send()
            .await
            .map_err(|e| AppError::Tool(format!("Search request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(AppError::Tool(format!(
                "Search engine returned status {}",
                response.status()
            )));
        }

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

        // Split by result containers — DuckDuckGo uses <div class="result">
        for block in html.split(r#"class="result"#).skip(1) {
            if results.len() >= max_results {
                break;
            }

            let title = Self::extract_between(block, r#"result__a"#, "</a>")
                .and_then(|s| Self::strip_html_tags(&s));
            let url = Self::extract_between(block, r#"href=""#, r#""#)
                .map(|s| Self::resolve_url(&s));
            let snippet = Self::extract_between(block, r#"result__snippet"#, "</span>")
                .and_then(|s| Self::strip_html_tags(&s));

            if let (Some(title), Some(url)) = (title, url) {
                results.push(json!({
                    "title": title.trim(),
                    "url": url,
                    "snippet": snippet.unwrap_or_default().trim()
                }));
            }
        }

        results
    }

    /// Extract text between two markers after a known anchor.
    fn extract_between<'a>(text: &'a str, anchor: &str, end_marker: &str) -> Option<String> {
        let start = text.find(anchor)?;
        let after_anchor = &text[start + anchor.len()..];
        // Skip past closing `>` of the anchor tag
        let tag_end = after_anchor.find('>')? + 1;
        let content = &after_anchor[tag_end..];
        let end = content.find(end_marker)?;
        Some(content[..end].to_string())
    }

    /// Strip HTML tags from a string.
    fn strip_html_tags(text: &str) -> Option<String> {
        let mut result = String::with_capacity(text.len());
        let mut in_tag = false;
        for c in text.chars() {
            match c {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => result.push(c),
                _ => {}
            }
        }
        if result.is_empty() { None } else { Some(result) }
    }

    /// Resolve redirect URLs from DuckDuckGo.
    fn resolve_url(url: &str) -> String {
        if let Some(uddg_pos) = url.find("uddg=") {
            let encoded = &url[uddg_pos + 5..];
            let end = encoded.find('&').unwrap_or(encoded.len());
            urlencoding::decode(&encoded[..end])
                .ok()
                .map(|s| s.to_string())
                .unwrap_or_else(|| url.to_string())
        } else if url.starts_with("//") {
            format!("https:{}", url)
        } else {
            url.to_string()
        }
    }
}
