//! 基于 LLM 与规则的质量评审器。
//!
//! 评审结论只有三种：通过（`Go`）、返工（`Revise`）、上报（`Escalate`）。
//! 规则快审不消耗 LLM 调用，LLM 深审用于规则无法覆盖的质量问题。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::llm::provider::ProviderRegistry;
use crate::llm::util::extract_json;
use crate::core::error::Result;
use crate::agent::workers::{SubTask, WorkerResult};

/// 评审结论。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CritiqueDecision {
    Go,
    Revise,
    Escalate,
}

/// 结构化的评审结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Critique {
    pub decision: CritiqueDecision,
    pub issues: Vec<String>,
    pub suggestions: Vec<String>,
    #[serde(default)]
    pub score: Option<f32>,
}

/// LLM 深审使用的系统提示词。
const CRITIC_SYSTEM_PROMPT: &str = r#"You are a critical reviewer evaluating AI agent work output. Your job is to assess correctness, completeness, quality, consistency, and safety.

Evaluate the worker output against these criteria:
1. **Correctness** — Does the output correctly address the task instruction?
2. **Completeness** — Is anything missing or incomplete?
3. **Quality** — Is the output well-structured, clear, and actionable?
4. **Consistency** — Does the output contradict itself or the task?
5. **Safety** — Does the output contain harmful, biased, or unsafe content?

Return your review as a JSON object with these fields:
- "decision": one of "go" (pass), "revise" (needs changes), "escalate" (needs human intervention)
- "issues": array of strings describing specific problems found (empty if none)
- "suggestions": array of strings with concrete improvement suggestions (empty if none)
- "score": optional float from 0.0 to 1.0 indicating overall quality

Only respond with the JSON object, no other text."#;

/// 使用 LLM 评审或规则检查来评审 worker 产出的 agent。
pub struct CriticAgent {
    providers: Arc<Mutex<ProviderRegistry>>,
}

impl CriticAgent {
    /// 用共享的 provider 注册表构造评审器。
    pub fn new(providers: Arc<Mutex<ProviderRegistry>>) -> Self {
        Self { providers }
    }

    /// 基于 LLM 深度评审 worker 产出。
    ///
    /// `model_id` 为 `None` 时回落到注册表的默认模型。没有可用模型或
    /// JSON 解析失败时返回默认的「Go」，避免评审环节阻断主流程。
    pub async fn review(
        &self,
        original_task: &SubTask,
        worker_result: &WorkerResult,
        model_id: Option<&str>,
    ) -> Result<Critique> {
        // 先确定模型；没有可用模型时直接通过。
        let mid = self.resolve_model_id(model_id).await;
        if mid.is_empty() {
            return Ok(self.default_go_critique());
        }

        // 确认 provider 确实已注册。
        {
            let registry = self.providers.lock().await;
            if !registry.is_registered(&mid) {
                return Ok(self.default_go_critique());
            }
        }

        let prompt = format!(
            r#"Task instruction: {instruction}

Worker output:
{content}

Evaluate the worker output against the instruction and provide your critique."#,
            instruction = original_task.instruction,
            content = worker_result.content,
        );
        let res = { let registry = self.providers.lock().await; registry.get(&mid) };
        let provider = match res {
            Ok(p) => p,
            Err(e) => {
                log::warn!("Critic provider not available ({}), returning default Go", e);
                return Ok(self.default_go_critique());
            }
        };

        let raw = match provider.prompt(CRITIC_SYSTEM_PROMPT, &prompt).await {
            Ok(r) => r,
            Err(e) => {
                log::warn!("Critic LLM call failed ({}), returning default Go", e);
                return Ok(self.default_go_critique());
            }
        };

        let extracted = extract_json(&raw);

        // 解析提取出的 JSON。
        match serde_json::from_str::<Critique>(&extracted) {
            Ok(critique) => Ok(critique),
            Err(e) => {
                log::warn!(
                    "Failed to parse critique JSON ({}), returning default Go. Raw: {}",
                    e,
                    raw.chars().take(200).collect::<String>()
                );
                Ok(Critique {
                    decision: CritiqueDecision::Go,
                    issues: vec![],
                    suggestions: vec![],
                    score: None,
                })
            }
        }
    }

    /// 不调用 LLM 的规则快审。
    ///
    /// 没有发现明显问题时返回 `None`。
    pub fn quick_review(_original_task: &SubTask, worker_result: &WorkerResult) -> Option<Critique> {
        let content = worker_result.content.trim();

        if content.is_empty() {
            return Some(Critique {
                decision: CritiqueDecision::Revise,
                issues: vec!["Worker returned empty content".into()],
                suggestions: vec!["Ensure the worker produces meaningful output".into()],
                score: Some(0.0),
            });
        }

        if content.len() < 10 {
            return Some(Critique {
                decision: CritiqueDecision::Revise,
                issues: vec!["Suspiciously short response".into()],
                suggestions: vec![format!(
                    "Worker returned only {} characters — expected a detailed response",
                    content.len()
                )],
                score: Some(0.1),
            });
        }

        let lower = content.to_lowercase();
        if lower.contains("i cannot") || lower.contains("i'm not able") || lower.contains("i am not able") {
            return Some(Critique {
                decision: CritiqueDecision::Revise,
                issues: vec!["Worker refused to complete the task".into()],
                suggestions: vec![
                    "Provide more context or rephrase the instruction".into(),
                    "Ensure the task is within the worker's capabilities".into(),
                ],
                score: Some(0.0),
            });
        }

        // 没有发现明显问题。
        None
    }

    /// 解析本次评审使用的模型 ID；不可用时返回空串。
    async fn resolve_model_id(&self, model_id: Option<&str>) -> String {
        match model_id {
            Some(id) if !id.is_empty() => id.to_string(),
            _ => {
                let registry = self.providers.lock().await;
                registry.default_model_id().to_string()
            }
        }
    }

    /// 返回默认的「Go」评审结果（直接通过）。
    fn default_go_critique(&self) -> Critique {
        Critique {
            decision: CritiqueDecision::Go,
            issues: vec![],
            suggestions: vec![],
            score: None,
        }
    }
}

/// 可评审 worker 产出的组件统一契约。
#[async_trait]
pub trait Reviewable: Send + Sync {
    /// 对一次 worker 产出给出评审结论。
    async fn review(&self, task: &SubTask, result: &WorkerResult) -> Result<Critique>;
}

#[async_trait]
impl Reviewable for CriticAgent {
    async fn review(&self, task: &SubTask, result: &WorkerResult) -> Result<Critique> {
        self.review(task, result, None).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::workers::WorkerKind;

    fn make_task(id: &str, label: &str, instruction: &str) -> SubTask {
        SubTask {
            id: id.into(),
            label: label.into(),
            instruction: instruction.into(),
            worker_kind: WorkerKind::Thinker,
            model_id: None,
            max_tokens: None,
            temperature: None,
            context: None,
        }
    }

    fn make_result(worker: WorkerKind, task_id: &str, content: &str) -> WorkerResult {
        WorkerResult {
            worker,
            task_id: task_id.into(),
            content: content.into(),
            metadata: None,
            duration_ms: Some(100),
        }
    }

    #[test]
    fn test_quick_review_empty_content() {
        let task = make_task("t1", "Test", "Do something");
        let result = make_result(WorkerKind::Thinker, "t1", "");
        let critique = CriticAgent::quick_review(&task, &result);
        assert!(critique.is_some());
        assert_eq!(critique.unwrap().decision, CritiqueDecision::Revise);
    }

    #[test]
    fn test_quick_review_clean() {
        let task = make_task("t2", "Test", "Do something");
        let result = make_result(
            WorkerKind::Thinker,
            "t2",
            "Here is a detailed analysis of the problem with multiple paragraphs of useful information.",
        );
        let critique = CriticAgent::quick_review(&task, &result);
        assert!(critique.is_none());
    }

    #[test]
    fn test_quick_review_short_content() {
        let task = make_task("t3", "Test", "Do something");
        let result = make_result(WorkerKind::Thinker, "t3", "Hi");
        let critique = CriticAgent::quick_review(&task, &result);
        assert!(critique.is_some());
        assert_eq!(critique.unwrap().decision, CritiqueDecision::Revise);
    }

    #[test]
    fn test_quick_review_refused() {
        let task = make_task("t4", "Test", "Do something");
        let result = make_result(WorkerKind::Thinker, "t4", "I cannot do that task for you.");
        let critique = CriticAgent::quick_review(&task, &result);
        assert!(critique.is_some());
        assert_eq!(critique.unwrap().decision, CritiqueDecision::Revise);
    }

    #[test]
    fn test_extract_json() {
        let input = "Here is the review:\n```json\n{\"decision\": \"go\", \"issues\": [], \"suggestions\": []}\n```\nEnd.";
        let extracted = extract_json(input);
        assert!(extracted.contains("\"decision\": \"go\""));
    }

    #[test]
    fn test_extract_json_bare() {
        let input =
            "{\"decision\": \"revise\", \"issues\": [\"bug\"], \"suggestions\": [\"fix it\"]}";
        let extracted = extract_json(input);
        assert!(extracted.contains("\"revise\""));
    }

    #[test]
    fn test_extract_json_generic_fence() {
        let input = "Some text\n```\n{\"decision\": \"go\"}\n```\nmore text";
        let extracted = extract_json(input);
        assert!(extracted.contains("\"decision\": \"go\""));
    }

    #[test]
    fn test_critique_serialization() {
        let critique = Critique {
            decision: CritiqueDecision::Revise,
            issues: vec!["Missing error handling".into()],
            suggestions: vec!["Add try-catch block".into()],
            score: Some(0.4),
        };
        let json = serde_json::to_string(&critique).unwrap();
        assert!(json.contains("\"revise\""));
        assert!(json.contains("\"Missing error handling\""));

        let deserialized: Critique = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.decision, CritiqueDecision::Revise);
        assert_eq!(deserialized.score, Some(0.4));
    }

    #[test]
    fn test_review_no_model_returns_go() {
        let providers = Arc::new(Mutex::new(ProviderRegistry::new(&[])));
        let agent = CriticAgent::new(providers);
        let task = make_task("t1", "Test", "Do something");
        let result = make_result(WorkerKind::Thinker, "t1", "Some output");

        // model_id 为空时，review 应返回默认的 Go。
        let rt = tokio::runtime::Runtime::new().unwrap();
        let critique = rt.block_on(agent.review(&task, &result, Some(""))).unwrap();
        assert_eq!(critique.decision, CritiqueDecision::Go);
    }
}
