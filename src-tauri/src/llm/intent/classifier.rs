//! Data-driven intent scorer and LLM reranker.
//!
//! The rerank prompt is built from [`IntentRecord`] rows, so candidate
//! descriptions and examples live in the database instead of hard-coded
//! tables. The LLM step returns `None` on every failure; the router owns the
//! decision and fallback.

use std::collections::HashSet;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::core::db::models::{IntentRecord, Message};
use crate::llm::provider::ProviderRegistry;

/// Context-aware reranker boundary, injectable for tests.
#[async_trait::async_trait]
pub trait IntentReranker: Send + Sync {
    async fn rerank(
        &self,
        message: &str,
        recent_messages: &[Message],
        candidates: &[IntentRecord],
    ) -> Option<(String, f32)>;
}

/// Context-aware reranker: it receives recent messages and a narrowed
/// candidate set, and it may answer `"none"`.
pub struct LlmReranker {
    providers: Arc<Mutex<ProviderRegistry>>,
    model_id: Option<String>,
}

impl LlmReranker {
    pub fn new(providers: Arc<Mutex<ProviderRegistry>>, model_id: Option<String>) -> Self {
        Self {
            providers,
            model_id,
        }
    }

    /// Ask the LLM to pick one candidate for `message`.
    ///
    /// Returns `None` for every failure, including an explicit `"none"` answer.
    /// Timeouts are owned by the caller.
    pub async fn rerank(
        &self,
        message: &str,
        recent_messages: &[Message],
        candidates: &[IntentRecord],
    ) -> Option<(String, f32)> {
        if candidates.is_empty() {
            return None;
        }

        let provider = {
            let registry = self.providers.lock().await;
            registry.resolve(self.model_id.as_deref()).ok()?
        };

        let preamble = build_rerank_prompt(candidates);
        let prompt = build_rerank_input(message, recent_messages);
        let reply = provider.prompt(&preamble, &prompt).await.ok()?;
        parse_classification_reply(&reply, candidates)
    }
}

#[async_trait::async_trait]
impl IntentReranker for LlmReranker {
    async fn rerank(
        &self,
        message: &str,
        recent_messages: &[Message],
        candidates: &[IntentRecord],
    ) -> Option<(String, f32)> {
        LlmReranker::rerank(self, message, recent_messages, candidates).await
    }
}

/// Render the rerank candidate list. `none` is an explicit valid answer.
pub fn build_rerank_prompt(candidates: &[IntentRecord]) -> String {
    let mut prompt = String::from(
        "You are an intent classifier for an AI coding assistant.\n\
         Pick exactly one candidate intent for the user's current message.\n\
         Use the recent conversation to resolve references such as \"that\" or \
         \"what we just discussed\".\n\
         If none of the candidates fits, answer with \"none\".\n\nCandidates:\n",
    );

    for intent in candidates {
        let examples = intent
            .examples
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join(" | ");
        prompt.push_str(&format!(
            "- id: {}\n  name: {}\n  when: {}\n  examples: {}\n",
            intent.id, intent.name, intent.description, examples
        ));
    }

    prompt.push_str(
        "\nRespond with one JSON object and nothing else:\n\
         { \"intent\": \"<id|none>\", \"confidence\": 0.0-1.0, \"reason\": \"...\" }\n\
         \"intent\" must be exactly one of the ids listed above, or \"none\".",
    );

    prompt
}

/// Render the current message plus a bounded recent-conversation block.
pub fn build_rerank_input(message: &str, recent_messages: &[Message]) -> String {
    const MAX_TOTAL_CHARS: usize = 2000;
    const MAX_MESSAGE_CHARS: usize = 500;

    let mut input = String::new();
    if !recent_messages.is_empty() {
        input.push_str("Recent conversation (oldest first):\n");
        let mut used = 0usize;
        for recent in recent_messages {
            if used >= MAX_TOTAL_CHARS {
                break;
            }
            let role = match recent.role.as_str() {
                "assistant" => "assistant",
                "system" => "system",
                "tool" => "tool",
                _ => "user",
            };
            let content: String = recent.content.chars().take(MAX_MESSAGE_CHARS).collect();
            let line = format!("{role}: {content}\n");
            used += line.chars().count();
            input.push_str(&line);
        }
        input.push('\n');
    }

    input.push_str("Current user message:\n");
    input.push_str(message);
    input
}

/// Fallback scoring driven purely by the stored data.
///
/// 1. Any intent whose `examples` appear in `message` (case-insensitive
///    substring) scores `0.9`; among several hits the longest example wins.
/// 2. Otherwise score by Chinese bigram overlap between the message and
///    `description` + `name` (intersection over union).
///
/// Returns the best intent, or `None` when every score is zero.
pub fn score_intents(message: &str, intents: &[IntentRecord]) -> Option<(String, f32)> {
    let lowered_message = message.to_lowercase();

    let mut best_example: Option<(&IntentRecord, usize)> = None;
    for intent in intents {
        for example in &intent.examples {
            let lowered_example = example.trim().to_lowercase();
            // A one-character example ("改") matches almost every sentence and
            // steals the decision from a better signal.
            if lowered_example.chars().count() < 2
                || !lowered_message.contains(lowered_example.as_str())
            {
                continue;
            }
            let matched_len = lowered_example.chars().count();
            let is_longer = match best_example {
                Some((_, best_len)) => matched_len > best_len,
                None => true,
            };
            if is_longer {
                best_example = Some((intent, matched_len));
            }
        }
    }
    if let Some((intent, _)) = best_example {
        // Deliberately below 1.0: an example hit is a strong hint, not proof.
        return Some((intent.id.clone(), 0.9));
    }

    let message_bigrams = bigrams(&lowered_message);
    if message_bigrams.is_empty() {
        return None;
    }

    let mut best: Option<(&IntentRecord, f32)> = None;
    for intent in intents {
        let target = bigrams(&format!("{}{}", intent.description, intent.name).to_lowercase());
        if target.is_empty() {
            continue;
        }
        let intersection = message_bigrams.intersection(&target).count();
        let union = message_bigrams.union(&target).count();
        let score = intersection as f32 / union as f32;
        if score <= 0.0 {
            continue;
        }
        let is_better = match best {
            Some((_, best_score)) => score > best_score,
            None => true,
        };
        if is_better {
            best = Some((intent, score));
        }
    }

    best.map(|(intent, score)| (intent.id.clone(), score))
}

/// Parse an LLM reply into `(intent_id, confidence)`.
///
/// Tolerates markdown fences and surrounding prose: the first JSON object that
/// carries both `intent` and `confidence` with a finite score is used. Objects
/// naming an unknown id, or carrying a non-finite score, are skipped. Returns
/// `None` when no usable object remains.
pub fn parse_classification_reply(reply: &str, intents: &[IntentRecord]) -> Option<(String, f32)> {
    for (start, _) in reply.match_indices('{') {
        let Some(end) = matching_brace_end(reply, start) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&reply[start..=end]) else {
            continue;
        };
        let Some(intent) = value.get("intent").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(confidence) = value.get("confidence").and_then(parse_confidence) else {
            continue;
        };

        // An unknown id only disqualifies this object; a model that restates
        // the candidate list may answer with a valid object later in the reply.
        if !intents.iter().any(|item| item.id == intent) {
            continue;
        }
        return Some((intent.to_string(), confidence));
    }

    None
}

fn parse_confidence(value: &serde_json::Value) -> Option<f32> {
    let raw = value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.parse::<f64>().ok()))?;
    // `clamp` does not sanitise NaN, and NaN would poison every later
    // comparison, so non-finite values make this object unusable.
    if !raw.is_finite() {
        return None;
    }
    Some((raw as f32).clamp(0.0, 1.0))
}

/// Find the closing brace of the object that starts at `start`, skipping
/// braces inside JSON strings.
fn matching_brace_end(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (offset, byte) in bytes[start..].iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                in_string = false;
            }
            continue;
        }

        match byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(start + offset);
                }
            }
            _ => {}
        }
    }

    None
}

/// Adjacent character pairs of `text`.
fn bigrams(text: &str) -> HashSet<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() < 2 {
        return HashSet::new();
    }
    chars.windows(2).map(|pair| pair.iter().collect()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::db::models::IntentRecord;

    fn intent(
        id: &str,
        name: &str,
        description: &str,
        examples: &[&str],
        confidence_threshold: f64,
        fallback_intent: &str,
    ) -> IntentRecord {
        IntentRecord {
            id: id.to_string(),
            domain_id: "builtin.software_dev".to_string(),
            name: name.to_string(),
            description: description.to_string(),
            examples: examples.iter().map(|e| e.to_string()).collect(),
            workflow: String::new(),
            allowed_tools: Vec::new(),
            default_agent: String::new(),
            scope: "global".to_string(),
            enabled: true,
            version: 1,
            confidence_threshold,
            fallback_intent: fallback_intent.to_string(),
            path: "fast".to_string(),
            auto_escalate: false,
            system_prompt_appendix: String::new(),
            model_id: String::new(),
            max_iterations: None,
            sort_order: 0,
            created_at: "2026-11-04T00:00:00Z".to_string(),
            updated_at: "2026-11-04T00:00:00Z".to_string(),
        }
    }

    fn fixtures() -> Vec<IntentRecord> {
        vec![
            intent(
                "builtin.software_dev.implement",
                "实现功能",
                "把需求实现为代码，编写或修改源文件",
                &["把这段改成异步", "实现一个新函数"],
                0.7,
                "builtin.software_dev.clarify",
            ),
            intent(
                "builtin.software_dev.explain",
                "解释代码",
                "解释代码逻辑与设计原理",
                &["解释这段代码", "改成"],
                0.7,
                "",
            ),
            intent(
                "builtin.software_dev.refactor",
                "重构代码",
                "在不改变行为的前提下重构结构",
                &[],
                0.7,
                "",
            ),
            intent(
                "builtin.software_dev.clarify",
                "澄清需求",
                "当意图不明确时向用户提问",
                &[],
                0.7,
                "",
            ),
        ]
    }

    #[test]
    fn score_matches_example_substring() {
        let intents = fixtures();
        let scored = score_intents("帮我解释这段代码", &intents);
        assert_eq!(
            scored,
            Some(("builtin.software_dev.explain".to_string(), 0.9))
        );
    }

    #[test]
    fn score_prefers_longer_example_match() {
        let intents = fixtures();
        let scored = score_intents("把这段改成异步", &intents);
        assert_eq!(
            scored.map(|(id, _)| id),
            Some("builtin.software_dev.implement".to_string())
        );
    }

    #[test]
    fn score_returns_none_when_nothing_matches() {
        assert_eq!(score_intents("hello there", &fixtures()), None);
    }

    #[test]
    fn one_character_example_is_ignored() {
        let intents = vec![intent(
            "builtin.software_dev.implement",
            "实现功能",
            "写代码",
            &["改"],
            0.7,
            "",
        )];

        assert_eq!(score_intents("改一下这个", &intents), None);
    }

    #[test]
    fn parse_reply_accepts_json_fence() {
        let reply = "```json\n{\"intent\": \"builtin.software_dev.implement\", \"confidence\": 0.93, \"reason\": \"asks for code\"}\n```";
        let parsed = parse_classification_reply(reply, &fixtures());
        let (id, score) = parsed.expect("fenced reply should parse");
        assert_eq!(id, "builtin.software_dev.implement");
        assert!((score - 0.93).abs() < 1e-6);
    }

    #[test]
    fn parse_reply_accepts_surrounding_prose() {
        let reply = "Sure, here you go: {\"intent\": \"builtin.software_dev.explain\", \"confidence\": 0.6} Hope that helps.";
        let parsed = parse_classification_reply(reply, &fixtures());
        let (id, score) = parsed.expect("prose-wrapped reply should parse");
        assert_eq!(id, "builtin.software_dev.explain");
        assert!((score - 0.6).abs() < 1e-6);
    }

    #[test]
    fn parse_reply_rejects_unknown_intent() {
        let reply = "{\"intent\": \"builtin.software_dev.nope\", \"confidence\": 0.99}";
        assert_eq!(parse_classification_reply(reply, &fixtures()), None);
    }

    #[test]
    fn bigram_overlap_scores_positive_without_example_match() {
        let intents = fixtures();
        // No example matches; the overlap comes from 重构 / 结构 in the
        // refactor intent's description.
        let scored = score_intents("重构这个结构", &intents);
        let (id, score) = scored.expect("bigram overlap should score");
        assert_eq!(id, "builtin.software_dev.refactor");
        assert!(score > 0.0, "expected a positive score, got {score}");
    }

    #[test]
    fn non_finite_confidence_is_rejected() {
        for raw in ["NaN", "inf", "-inf"] {
            let reply = format!(
                "{{\"intent\": \"builtin.software_dev.implement\", \"confidence\": \"{raw}\"}}"
            );
            assert_eq!(
                parse_classification_reply(&reply, &fixtures()),
                None,
                "confidence {raw} must be rejected"
            );
        }
    }

    fn message(role: &str, content: &str) -> Message {
        Message {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: "session".to_string(),
            role: role.to_string(),
            content: content.to_string(),
            tool_calls: None,
            tool_call_id: None,
            tokens: None,
            created_at: "2026-11-05T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn rerank_prompt_lists_candidates_and_none_option() {
        let prompt = build_rerank_prompt(&fixtures());
        assert!(prompt.contains("builtin.software_dev.implement"));
        assert!(prompt.contains("builtin.software_dev.explain"));
        assert!(prompt.contains("none"));
        assert!(prompt.contains("confidence"));
    }

    #[test]
    fn rerank_input_includes_recent_messages_and_current_message() {
        let recent = vec![
            message("user", "登录接口报 500"),
            message("assistant", "我先看日志"),
        ];

        let input = build_rerank_input("先看看日志", &recent);

        assert!(input.contains("user: 登录接口报 500"));
        assert!(input.contains("assistant: 我先看日志"));
        assert!(input.contains("Current user message:\n先看看日志"));
    }

    #[test]
    fn rerank_input_truncates_long_messages() {
        let long = "x".repeat(2000);
        let recent = vec![message("user", &long), message("assistant", &long)];

        let input = build_rerank_input("now", &recent);

        assert!(input.chars().count() < 2600);
        assert!(input.contains("Current user message:\nnow"));
    }

    #[test]
    fn rerank_reply_accepts_none_as_no_match() {
        let reply = "{\"intent\": \"none\", \"confidence\": 0.0}";
        assert_eq!(parse_classification_reply(reply, &fixtures()), None);
    }

    #[test]
    fn rerank_reply_rejects_unknown_candidate() {
        let reply = "{\"intent\": \"builtin.software_dev.does_not_exist\", \"confidence\": 0.9}";
        assert_eq!(parse_classification_reply(reply, &fixtures()), None);
    }
}
