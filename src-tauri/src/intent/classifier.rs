use std::sync::Arc;

use tokio::sync::Mutex;

use crate::api::provider::ProviderRegistry;
use crate::api::rig::extract_structured;
use crate::api::types::{ChatRequest, Message as ApiMessage, MessageRole};
use crate::intent::ClassificationResult;

/// System prompt for the intent classifier.
const CLASSIFIER_PROMPT: &str = r#"You are an intent classifier for an AI coding assistant.
Classify the user's message into exactly one category:

- "chat": Simple conversation, greetings, questions needing only a direct answer.
- "code": Coding tasks: implement, refactor, fix, debug, analyze, test, build.
- "research": Information seeking: search docs, explain concepts, compare approaches.
- "auto": Complex multi-step tasks requiring planning and autonomous execution.

Rules:
- Simple message → chat, needs-code-change → code, needs-info → research, complex-task → auto
- auto_escalate: false for chat, true for everything else
- max_iterations: simple=5, moderate=10, complex=15
- If unsure between chat and work, prefer work (code/research/auto)"#;

pub struct LlmClassifier {
    providers: Arc<Mutex<ProviderRegistry>>,
    model_id: String,
    openai_api_key: String,
}

impl LlmClassifier {
    pub fn new(providers: Arc<Mutex<ProviderRegistry>>, model_id: String) -> Self {
        let openai_api_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
        Self { providers, model_id, openai_api_key }
    }

    /// Classify a user message into an intent.
    /// Tries Rig structured extraction first, then LLM chat, then keyword fallback.
    pub async fn classify(&self, message: &str) -> ClassificationResult {
        // 1. Try Rig extractor (type-safe, no JSON parsing needed)
        if !self.openai_api_key.is_empty() {
            match self.rig_classify(message).await {
                Ok(result) => {
                    log::info!("Rig classify: intent={}, reason={}", result.intent, result.reason);
                    return result;
                }
                Err(e) => {
                    log::warn!("Rig classify failed, trying LLM chat: {}", e);
                }
            }
        }

        // 2. Try LLM chat-based classification (fallback)
        match self.llm_classify(message).await {
            Ok(result) => {
                log::info!("LLM classify: intent={}, reason={}", result.intent, result.reason);
                return result;
            }
            Err(e) => {
                log::warn!("LLM classify failed, using keyword fallback: {}", e);
            }
        }

        // 3. Fallback keyword heuristic
        fallback_classify(message)
    }

    /// Use Rig's structured extractor for type-safe classification.
    async fn rig_classify(&self, message: &str) -> Result<ClassificationResult, String> {
        let result: ClassificationResult = extract_structured(
            &self.openai_api_key,
            "gpt-4o",
            CLASSIFIER_PROMPT,
            message,
        )
        .await
        .map_err(|e| e.to_string())?;

        Ok(result)
    }

    /// Fallback: use LLM chat with manual JSON parsing.
    async fn llm_classify(&self, message: &str) -> Result<ClassificationResult, String> {
        let providers = self.providers.lock().await;
        let mid = if self.model_id.is_empty() {
            providers.default_model_id().to_string()
        } else {
            self.model_id.clone()
        };
        if mid.is_empty() {
            return Err("No model configured for classification".to_string());
        }
        let provider = providers.get(&mid).map_err(|e| e.to_string())?;
        drop(providers);

        let messages = vec![
            ApiMessage {
                id: None,
                role: MessageRole::System,
                content: CLASSIFIER_PROMPT.to_string(),
                tool_calls: None,
                tool_call_id: None,
            },
            ApiMessage {
                id: None,
                role: MessageRole::User,
                content: message.to_string(),
                tool_calls: None,
                tool_call_id: None,
            },
        ];

        let request = ChatRequest {
            messages,
            model: mid.to_string(),
            tools: None,
            stream: Some(false),
            max_tokens: Some(100),
            temperature: Some(0.0),
        };

        let response = provider.chat(request).await.map_err(|e| e.to_string())?;
        let content = response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        let trimmed = content.trim();
        let json_str = if let Some(start) = trimmed.find('{') {
            let end = trimmed.rfind('}').unwrap_or(trimmed.len());
            &trimmed[start..=end]
        } else {
            trimmed
        };

        serde_json::from_str::<ClassificationResult>(json_str)
            .map_err(|e| format!("Failed to parse classifier response: {}, raw: {}", e, &json_str.chars().take(100).collect::<String>()))
    }
}

/// Fallback keyword heuristic when LLM is unavailable.
fn fallback_classify(message: &str) -> ClassificationResult {
    let msg = message.to_lowercase();
    let chinese_task = ["重构", "实现", "编写", "修复", "调试", "编译", "测试",
        "分析", "创建", "开发", "构建", "修改", "添加", "删除", "迁移", "生成",
        "解析", "转换", "优化", "拆分", "合并", "提取", "注入", "检查", "查看",
        "告诉我", "列出", "显示", "搜索", "查询"];
    let chinese_research = ["为什么", "如何", "什么", "怎么", "区别", "比较", "解释", "了解"];

    let has_code = chinese_task.iter().any(|kw| message.contains(kw));
    let has_research = chinese_research.iter().any(|kw| message.contains(kw));
    let has_english_code = ["implement", "refactor", "fix", "debug", "analyze",
        "build", "create", "write", "test", "function", "class", "api"]
        .iter().any(|kw| msg.contains(kw));
    let has_english_research = ["search", "find", "explain", "compare", "what is",
        "how does", "why does", "research", "investigate"]
        .iter().any(|kw| msg.contains(kw));

    if has_code || has_english_code {
        ClassificationResult {
            intent: "code".to_string(),
            reason: "fallback: task keyword matched".to_string(),
            auto_escalate: true,
            max_iterations: 10,
        }
    } else if has_research || has_english_research {
        ClassificationResult {
            intent: "research".to_string(),
            reason: "fallback: research keyword matched".to_string(),
            auto_escalate: true,
            max_iterations: 10,
        }
    } else if message.chars().count() > 20 {
        ClassificationResult {
            intent: "auto".to_string(),
            reason: "fallback: long message".to_string(),
            auto_escalate: true,
            max_iterations: 15,
        }
    } else {
        ClassificationResult {
            intent: "chat".to_string(),
            reason: "fallback: no task keywords".to_string(),
            auto_escalate: false,
            max_iterations: 1,
        }
    }
}
