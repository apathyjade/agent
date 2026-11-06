//! 数据驱动的意图打分器与 LLM reranker。
//!
//! rerank prompt 由 [`IntentRecord`] 行拼装而成，候选的描述与示例都存放在
//! 数据库里，而不是硬编码的表格。LLM 这一步在任何失败下都返回 `None`；决策与
//! fallback 始终由路由器负责。

use std::collections::HashSet;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::core::db::models::{IntentRecord, Message};
use crate::llm::provider::ProviderRegistry;

/// 一次 rerank 的结果。
///
/// 必须把「模型明确说没有候选匹配」与「没拿到可用回答」分开：前者是可信
/// 语义结论，路由器应停止降级；后者才允许继续用 embedding / 规则兜底。
#[derive(Debug, Clone, PartialEq)]
pub enum RerankOutcome {
    /// 模型在候选集内选中了一个意图。
    Picked { intent_id: String, confidence: f32 },
    /// 模型明确回答 `none`：候选都不匹配。
    NoMatch,
    /// 调用失败、超时或回复无法解析；路由器可继续降级。
    Unavailable,
}

/// 上下文感知的 reranker 边界，可在测试中注入替身。
#[async_trait::async_trait]
pub trait IntentReranker: Send + Sync {
    async fn rerank(
        &self,
        message: &str,
        recent_messages: &[Message],
        candidates: &[IntentRecord],
    ) -> RerankOutcome;
}

/// 上下文感知的 reranker：接收最近消息与收窄后的候选集，并允许回答 `"none"`。
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

    /// 让 LLM 为 `message` 从候选中挑一个。
    ///
    /// 调用失败、超时或回复不可解析都返回 [`RerankOutcome::Unavailable`]；
    /// 模型显式回答 `"none"` 返回 [`RerankOutcome::NoMatch`]。超时由调用方负责。
    pub async fn rerank(
        &self,
        message: &str,
        recent_messages: &[Message],
        candidates: &[IntentRecord],
    ) -> RerankOutcome {
        if candidates.is_empty() {
            return RerankOutcome::Unavailable;
        }

        let Ok(provider) = self.providers.lock().await.resolve(self.model_id.as_deref()) else {
            return RerankOutcome::Unavailable;
        };

        let preamble = build_rerank_prompt(candidates);
        let prompt = build_rerank_input(message, recent_messages);
        match provider.prompt(&preamble, &prompt).await {
            Ok(reply) => parse_rerank_reply(&reply, candidates),
            Err(_) => RerankOutcome::Unavailable,
        }
    }
}

#[async_trait::async_trait]
impl IntentReranker for LlmReranker {
    async fn rerank(
        &self,
        message: &str,
        recent_messages: &[Message],
        candidates: &[IntentRecord],
    ) -> RerankOutcome {
        LlmReranker::rerank(self, message, recent_messages, candidates).await
    }
}

/// 渲染 rerank 的候选列表。`none` 是显式合法的回答。
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

/// 渲染当前消息，并附上一段有长度上限的最近对话。
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

/// 纯靠已存数据驱动的兜底打分。
///
/// 1. 任何意图的 `examples` 出现在 `message` 中（大小写不敏感的子串）即得
///    `0.9`；多个命中时长 example 优先。
/// 2. 否则按消息与 `description` + `name` 的中文 bigram 重叠度（交并比）打分。
///
/// 返回最佳意图；所有分数都为 0 时返回 `None`。
pub fn score_intents(message: &str, intents: &[IntentRecord]) -> Option<(String, f32)> {
    let lowered_message = message.to_lowercase();

    let mut best_example: Option<(&IntentRecord, usize)> = None;
    for intent in intents {
        for example in &intent.examples {
            let lowered_example = example.trim().to_lowercase();
            // 单字 example（如「改」）几乎能匹配任何句子，会把决策从更可靠的
            // 信号手里抢走。
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
        // 故意低于 1.0：example 命中只是强提示，不是证明。
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

/// 把 LLM 回复解析成 [`RerankOutcome`]。
///
/// 容忍 markdown 代码围栏与前后文：使用第一个带 `intent` 的 JSON 对象。
/// `intent: "none"` 表示语义上的无匹配（`confidence` 可缺省）；未知 id、非有限
/// 分数或完全没有可用对象都视为 `Unavailable`。
pub fn parse_rerank_reply(reply: &str, intents: &[IntentRecord]) -> RerankOutcome {
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
        if intent == "none" {
            return RerankOutcome::NoMatch;
        }
        let Some(confidence) = value.get("confidence").and_then(parse_confidence) else {
            continue;
        };

        // 未知 id 只否定这一个对象；模型复述候选列表后，可能在回复更靠后的
        // 位置给出合法对象。
        if !intents.iter().any(|item| item.id == intent) {
            continue;
        }
        return RerankOutcome::Picked {
            intent_id: intent.to_string(),
            confidence,
        };
    }

    RerankOutcome::Unavailable
}

fn parse_confidence(value: &serde_json::Value) -> Option<f32> {
    let raw = value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.parse::<f64>().ok()))?;
    // `clamp` 不会清洗 NaN，而 NaN 会污染后续所有比较，因此非有限值让这个
    // 对象直接不可用。
    if !raw.is_finite() {
        return None;
    }
    Some((raw as f32).clamp(0.0, 1.0))
}

/// 找到从 `start` 开始的那个对象的右花括号，跳过 JSON 字符串内部的括号。
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

/// `text` 的相邻字符二元组。
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
        assert_eq!(
            parse_rerank_reply(reply, &fixtures()),
            RerankOutcome::Picked {
                intent_id: "builtin.software_dev.implement".to_string(),
                confidence: 0.93,
            }
        );
    }

    #[test]
    fn parse_reply_accepts_surrounding_prose() {
        let reply = "Sure, here you go: {\"intent\": \"builtin.software_dev.explain\", \"confidence\": 0.6} Hope that helps.";
        assert_eq!(
            parse_rerank_reply(reply, &fixtures()),
            RerankOutcome::Picked {
                intent_id: "builtin.software_dev.explain".to_string(),
                confidence: 0.6,
            }
        );
    }

    #[test]
    fn parse_reply_rejects_unknown_intent() {
        let reply = "{\"intent\": \"builtin.software_dev.nope\", \"confidence\": 0.99}";
        assert_eq!(
            parse_rerank_reply(reply, &fixtures()),
            RerankOutcome::Unavailable
        );
    }

    #[test]
    fn explicit_none_is_a_match_failure_not_an_error() {
        let reply = "{\"intent\": \"none\", \"confidence\": 0.0}";
        assert_eq!(parse_rerank_reply(reply, &fixtures()), RerankOutcome::NoMatch);
    }

    #[test]
    fn none_without_confidence_is_still_a_match_failure() {
        let reply = "{\"intent\": \"none\"}";
        assert_eq!(parse_rerank_reply(reply, &fixtures()), RerankOutcome::NoMatch);
    }

    #[test]
    fn garbage_reply_is_unavailable() {
        assert_eq!(
            parse_rerank_reply("I cannot help with that.", &fixtures()),
            RerankOutcome::Unavailable
        );
    }

    #[test]
    fn bigram_overlap_scores_positive_without_example_match() {
        let intents = fixtures();
        // 没有 example 命中；重叠来自 refactor 意图描述里的「重构 / 结构」。
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
                parse_rerank_reply(&reply, &fixtures()),
                RerankOutcome::Unavailable,
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
            tool_name: None,
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
    fn rerank_reply_rejects_unknown_candidate() {
        let reply = "{\"intent\": \"builtin.software_dev.does_not_exist\", \"confidence\": 0.9}";
        assert_eq!(
            parse_rerank_reply(reply, &fixtures()),
            RerankOutcome::Unavailable
        );
    }
}
