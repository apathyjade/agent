//! Data-driven intent classifier.
//!
//! The classification prompt is built from [`IntentRecord`] rows, so keyword
//! capability comes from the stored `examples` instead of hard-coded tables.
//! The LLM step returns `None` on every failure; the caller owns the fallback
//! chain.

use std::collections::HashSet;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::core::db::models::IntentRecord;
use crate::llm::provider::ProviderRegistry;

/// Classifies user messages with the configured LLM provider.
pub struct LlmClassifier {
    providers: Arc<Mutex<ProviderRegistry>>,
    model_id: Option<String>,
}

impl LlmClassifier {
    pub fn new(providers: Arc<Mutex<ProviderRegistry>>, model_id: Option<String>) -> Self {
        Self {
            providers,
            model_id,
        }
    }

    /// Ask the LLM to pick one of `intents` for `message`.
    ///
    /// Returns `None` for every failure: no provider, request error,
    /// unparsable reply, or an intent id that is not in `intents`. This method
    /// never falls back internally — the caller owns fallback.
    pub async fn classify(
        &self,
        message: &str,
        intents: &[IntentRecord],
    ) -> Option<(String, f32)> {
        if intents.is_empty() {
            return None;
        }

        let provider = {
            let registry = self.providers.lock().await;
            registry.resolve(self.model_id.as_deref()).ok()?
        };

        let preamble = build_classifier_prompt(intents);
        let reply = provider.prompt(&preamble, message).await.ok()?;
        parse_classification_reply(&reply, intents)
    }
}

/// Render the classifier prompt from the stored intents.
pub fn build_classifier_prompt(intents: &[IntentRecord]) -> String {
    let mut prompt = String::from(
        "You are an intent classifier for an AI coding assistant.\n\
         Pick exactly one intent for the user's message.\n\nIntents:\n",
    );

    for intent in intents {
        prompt.push_str(&format!(
            "- id: {}\n  name: {}\n  when: {}\n  examples: {}\n",
            intent.id,
            intent.name,
            intent.description,
            intent.examples.join(" | ")
        ));
    }

    prompt.push_str(
        "\nRespond with one JSON object and nothing else:\n\
         { \"intent\": \"<id>\", \"confidence\": 0.0-1.0, \"reason\": \"...\" }\n\
         \"intent\" must be exactly one of the ids listed above.",
    );

    prompt
}

/// Fallback scoring driven purely by the stored data.
///
/// 1. Any intent whose `examples` appear in `message` (case-insensitive
///    substring) scores `1.0`; among several hits the longest example wins.
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
            if lowered_example.is_empty() || !lowered_message.contains(lowered_example.as_str()) {
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
        return Some((intent.id.clone(), 1.0));
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

/// Apply the confidence threshold of the chosen intent.
///
/// Returns `chosen` unchanged when `score` clears its threshold. Otherwise it
/// falls back to `fallback_intent` (when non-empty and present), then to the
/// `.clarify` intent, then to `chosen`.
pub fn apply_confidence(intents: &[IntentRecord], chosen: &str, score: f32) -> String {
    let Some(chosen_intent) = intents.iter().find(|intent| intent.id == chosen) else {
        return chosen.to_string();
    };

    if f64::from(score) >= chosen_intent.confidence_threshold {
        return chosen.to_string();
    }

    if !chosen_intent.fallback_intent.is_empty()
        && intents
            .iter()
            .any(|intent| intent.id == chosen_intent.fallback_intent)
    {
        return chosen_intent.fallback_intent.clone();
    }

    if let Some(clarify) = intents.iter().find(|intent| intent.id.ends_with(".clarify")) {
        return clarify.id.clone();
    }

    chosen.to_string()
}

/// Parse an LLM reply into `(intent_id, confidence)`.
///
/// Tolerates markdown fences and surrounding prose: the first JSON object that
/// carries both `intent` and `confidence` is used. Returns `None` when no such
/// object exists or when its `intent` is not one of `intents`.
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

        if !intents.iter().any(|item| item.id == intent) {
            return None;
        }
        return Some((intent.to_string(), confidence));
    }

    None
}

fn parse_confidence(value: &serde_json::Value) -> Option<f32> {
    let raw = value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.parse::<f64>().ok()))?;
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
    fn prompt_lists_every_intent_id() {
        let intents = fixtures();
        let prompt = build_classifier_prompt(&intents);
        for item in &intents {
            assert!(
                prompt.contains(&item.id),
                "prompt should mention id {}",
                item.id
            );
        }
    }

    #[test]
    fn prompt_mentions_confidence_field() {
        let prompt = build_classifier_prompt(&fixtures());
        assert!(prompt.contains("confidence"));
        assert!(prompt.contains("\"intent\""));
    }

    #[test]
    fn score_matches_example_substring() {
        let intents = fixtures();
        let scored = score_intents("帮我解释这段代码", &intents);
        assert_eq!(
            scored,
            Some(("builtin.software_dev.explain".to_string(), 1.0))
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
    fn low_confidence_falls_back_to_neighbour() {
        let intents = fixtures();
        assert_eq!(
            apply_confidence(&intents, "builtin.software_dev.implement", 0.2),
            "builtin.software_dev.clarify"
        );
        assert_eq!(
            apply_confidence(&intents, "builtin.software_dev.implement", 0.9),
            "builtin.software_dev.implement"
        );
    }

    #[test]
    fn empty_fallback_is_terminal() {
        let intents = vec![
            intent(
                "builtin.software_dev.implement",
                "实现功能",
                "把需求实现为代码",
                &[],
                0.9,
                "",
            ),
            intent(
                "builtin.software_dev.explain",
                "解释代码",
                "解释代码逻辑",
                &[],
                0.7,
                "",
            ),
        ];
        assert_eq!(
            apply_confidence(&intents, "builtin.software_dev.implement", 0.1),
            "builtin.software_dev.implement"
        );
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
}
