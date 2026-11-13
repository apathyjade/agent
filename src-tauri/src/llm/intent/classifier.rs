//! 数据驱动的意图打分器与 LLM reranker。
//!
//! rerank prompt 由 [`IntentRecord`] 行拼装而成，候选的描述与示例都存放在
//! 数据库里，而不是硬编码的表格。LLM 这一步在任何失败下都返回 `None`；决策与
//! fallback 始终由路由器负责。

use std::collections::HashSet;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::core::db::models::{IntentRecord, Message};
use crate::llm::intent::scope::{build_scope_prompt, parse_scope_reply, CatalogTool, ScopeDraft};
use crate::llm::provider::ProviderRegistry;

/// 拆解输入携带的最近消息条数（与 `ROUTE_HISTORY_LIMIT` 的 6 条保持一致）。
const DECOMPOSE_HISTORY_LIMIT: usize = 6;

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

/// 一次拆解里的单个意图项（已通过网络边界与候选校验）。
#[derive(Debug, Clone, PartialEq)]
pub struct DecomposedItem {
    pub intent_id: String,
    pub instruction: String,
    pub confidence: f32,
}

/// 拆解调用结果。
#[derive(Debug, Clone, PartialEq)]
pub enum DecomposeOutcome {
    /// 模型返回了可解析的意图列表（可能为空 = 零意图）。
    Parsed(Vec<DecomposedItem>),
    /// 调用失败、超时或无可用候选；上层应走普通聊天兜底。
    Unavailable,
}

/// 上下文感知的拆解器边界，可在测试中注入替身。
#[async_trait::async_trait]
pub trait IntentDecomposer: Send + Sync {
    async fn decompose(
        &self,
        message: &str,
        recent_messages: &[Message],
        candidates: &[IntentRecord],
    ) -> DecomposeOutcome;

    /// 第二级路由：在候选资源里挑出本回合要用的子集。
    ///
    /// 默认实现不参与收窄，因此既有测试替身无需改动；只有真实 LLM 实现覆盖它。
    async fn narrow_scope(&self, _message: &str, _resources: &[CatalogTool]) -> Option<ScopeDraft> {
        None
    }
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

impl LlmReranker {
    /// 让 LLM 把用户输入拆解为**零个或多个**候选意图。
    ///
    /// 调用失败、超时或没有候选时返回 [`DecomposeOutcome::Unavailable`]；
    /// 解析失败视为零意图。超时由调用方负责。
    pub async fn decompose(
        &self,
        message: &str,
        recent_messages: &[Message],
        candidates: &[IntentRecord],
    ) -> DecomposeOutcome {
        if candidates.is_empty() {
            return DecomposeOutcome::Unavailable;
        }

        let Ok(provider) = self.providers.lock().await.resolve(self.model_id.as_deref()) else {
            return DecomposeOutcome::Unavailable;
        };

        let preamble = build_decompose_prompt(candidates);
        let prompt = build_decompose_input(message, recent_messages);
        match provider.prompt(&preamble, &prompt).await {
            Ok(reply) => DecomposeOutcome::Parsed(parse_decompose_reply(&reply, candidates)),
            Err(_) => DecomposeOutcome::Unavailable,
        }
    }

    /// 第二级路由：让模型从资源摘要里挑出本回合要用的工具。
    ///
    /// 失败、超时（由调用方控制）或回复不可解析都返回 `None`——收窄失败必须
    /// 退回「不收窄」，而不是把工具全部裁掉。
    pub async fn narrow_scope(
        &self,
        message: &str,
        resources: &[CatalogTool],
    ) -> Option<ScopeDraft> {
        if resources.is_empty() {
            return None;
        }
        let Ok(provider) = self.providers.lock().await.resolve(self.model_id.as_deref()) else {
            return None;
        };
        let preamble = build_scope_prompt(resources);
        match provider.prompt(&preamble, message).await {
            Ok(reply) => parse_scope_reply(&reply, resources),
            Err(_) => None,
        }
    }
}

#[async_trait::async_trait]
impl IntentDecomposer for LlmReranker {
    async fn decompose(
        &self,
        message: &str,
        recent_messages: &[Message],
        candidates: &[IntentRecord],
    ) -> DecomposeOutcome {
        LlmReranker::decompose(self, message, recent_messages, candidates).await
    }

    async fn narrow_scope(&self, message: &str, resources: &[CatalogTool]) -> Option<ScopeDraft> {
        LlmReranker::narrow_scope(self, message, resources).await
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

/// 渲染拆解的候选清单。零意图是显式合法的回答（空数组）。
pub fn build_decompose_prompt(candidates: &[IntentRecord]) -> String {
    let mut prompt = String::from(
        "You are an intent decomposer for an AI coding assistant.\n\
         Read the user's current message and decompose it into ZERO OR MORE candidate intents.\n\
         Use the recent conversation to resolve references such as \"that\" or \n\
         \"what we just discussed\".\n\
         A single message often needs MORE THAN ONE intent: when it contains several\n\
         independent things (for example \"plan first, then implement\" or \"fix the bug\n\
         and add tests\"), list EVERY matching intent, each with its own concrete\n\
         instruction. Do not just pick the single best match.\n\
         Only use ids from the candidate list below; never invent an id, and never\n\
         list the same id more than once.\n\
         For each intent, write a concrete instruction that tells the agent what to do.\n\
         Matching only some of the candidates still counts as a match.\n\
         Return an empty intents array only when the message is unrelated to ALL\n\
         candidate intents (greetings, chit-chat, weather, translation, emotional\n\
         expression).\n\nCandidates:\n",
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
         { \"intents\": [ { \"intent_id\": \"<id>\", \"instruction\": \"...\", \"confidence\": 0.0-1.0 } ] }\n\
         Return {\"intents\": []} only when the message is unrelated to all candidates.",
    );

    prompt
}

/// 渲染拆解的输入：最近对话（最多 [`DECOMPOSE_HISTORY_LIMIT`] 条）与当前消息。
pub fn build_decompose_input(message: &str, recent_messages: &[Message]) -> String {
    let start = recent_messages.len().saturating_sub(DECOMPOSE_HISTORY_LIMIT);
    build_rerank_input(message, &recent_messages[start..])
}

/// 把 LLM 回复解析成已校验的意图项列表。
///
/// 校验规则（防幻觉）：
/// - `intent_id` 不在候选集合内 → 丢弃该项；
/// - `instruction` 为空 → 丢弃该项；
/// - `confidence` 缺失或非数值 → 记 `0.0`；
/// - 回复里没有可解析的 `intents` 数组 → 空列表（零意图）。
pub fn parse_decompose_reply(reply: &str, candidates: &[IntentRecord]) -> Vec<DecomposedItem> {
    let mut items: Vec<DecomposedItem> = Vec::new();

    for (start, _) in reply.match_indices('{') {
        let Some(end) = matching_brace_end(reply, start) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&reply[start..=end]) else {
            continue;
        };
        let Some(entries) = value.get("intents").and_then(|v| v.as_array()) else {
            continue;
        };

        for entry in entries {
            let Some(intent_id) = entry.get("intent_id").and_then(|v| v.as_str()) else {
                continue;
            };
            if !candidates.iter().any(|intent| intent.id == intent_id) {
                continue;
            }
            let instruction = entry
                .get("instruction")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .trim()
                .to_string();
            if instruction.is_empty() {
                continue;
            }
            let confidence = entry
                .get("confidence")
                .and_then(parse_confidence)
                .unwrap_or(0.0);
            if items.iter().any(|item| item.intent_id == intent_id) {
                continue;
            }
            items.push(DecomposedItem {
                intent_id: intent_id.to_string(),
                instruction,
                confidence,
            });
        }

        return items;
    }

    items
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

    #[test]
    fn decompose_prompt_lists_candidates_and_zero_intent_contract() {
        let prompt = build_decompose_prompt(&fixtures());

        assert!(prompt.contains("builtin.software_dev.implement"));
        assert!(prompt.contains("builtin.software_dev.explain"));
        assert!(prompt.contains("name: 实现功能"));
        assert!(prompt.contains("when: 把需求实现为代码"));
        assert!(prompt.contains("examples: 把这段改成异步"));
        assert!(
            prompt.contains("{\"intents\": []}"),
            "必须给出零意图的确定表达路径：{prompt}"
        );
    }

    #[test]
    fn decompose_prompt_encourages_listing_every_matching_intent() {
        let prompt = build_decompose_prompt(&fixtures());

        assert!(prompt.contains("MORE THAN ONE"), "{prompt}");
        assert!(prompt.contains("EVERY matching intent"), "{prompt}");
        assert!(
            prompt.contains("list the same id more than once"),
            "必须明确每个 id 至多出现一次：{prompt}"
        );
        assert!(
            prompt.contains("Matching only some of the candidates still counts as a match"),
            "部分匹配也算匹配：{prompt}"
        );
        assert!(
            prompt.contains("unrelated to ALL"),
            "零意图条件必须收窄成「与所有候选都无关」：{prompt}"
        );
    }

    #[test]
    fn decompose_reply_parses_ordered_items() {
        let reply = r#"{"intents": [
            {"intent_id": "builtin.software_dev.implement", "instruction": "实现导出", "confidence": 0.91},
            {"intent_id": "builtin.software_dev.explain", "instruction": "解释现有实现", "confidence": 0.62}
        ]}"#;

        let items = parse_decompose_reply(reply, &fixtures());

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].intent_id, "builtin.software_dev.implement");
        assert_eq!(items[0].instruction, "实现导出");
        assert_eq!(items[0].confidence, 0.91);
        assert_eq!(items[1].intent_id, "builtin.software_dev.explain");
    }

    #[test]
    fn decompose_reply_empty_array_is_zero_intent() {
        assert!(parse_decompose_reply(r#"{"intents": []}"#, &fixtures()).is_empty());
    }

    #[test]
    fn decompose_reply_missing_confidence_becomes_zero() {
        let reply = r#"{"intents": [{"intent_id": "builtin.software_dev.implement", "instruction": "实现"}]}"#;

        let items = parse_decompose_reply(reply, &fixtures());

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].confidence, 0.0);
    }

    #[test]
    fn decompose_reply_drops_empty_instruction() {
        let reply = r#"{"intents": [
            {"intent_id": "builtin.software_dev.implement", "instruction": "  ", "confidence": 0.8},
            {"intent_id": "builtin.software_dev.explain", "instruction": "解释", "confidence": 0.8}
        ]}"#;

        let items = parse_decompose_reply(reply, &fixtures());

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].intent_id, "builtin.software_dev.explain");
    }

    #[test]
    fn decompose_reply_drops_hallucinated_intent() {
        let reply = r#"{"intents": [
            {"intent_id": "builtin.software_dev.does_not_exist", "instruction": "幻觉", "confidence": 0.99},
            {"intent_id": "builtin.software_dev.implement", "instruction": "实现", "confidence": 0.9}
        ]}"#;

        let items = parse_decompose_reply(reply, &fixtures());

        assert_eq!(items.len(), 1, "候选外的 id 必须被丢弃");
        assert_eq!(items[0].intent_id, "builtin.software_dev.implement");
    }

    #[test]
    fn decompose_input_keeps_only_the_last_six_messages() {
        let recent: Vec<Message> = (1..=8)
            .map(|index| message("user", &format!("MESSAGE_{index}")))
            .collect();

        let input = build_decompose_input("now", &recent);

        assert!(!input.contains("MESSAGE_1"));
        assert!(!input.contains("MESSAGE_2"));
        assert!(input.contains("MESSAGE_3"));
        assert!(input.contains("MESSAGE_8"));
        assert!(input.contains("Current user message:\nnow"));
    }
}
