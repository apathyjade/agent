use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::api::provider::ProviderRegistry;
use crate::api::util::extract_json;
use crate::error::Result;
use crate::workers::{SubTask, WorkerResult};

/// Critique decision from the reviewer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CritiqueDecision {
    Go,
    Revise,
    Escalate,
}

/// Structured critique result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Critique {
    pub decision: CritiqueDecision,
    pub issues: Vec<String>,
    pub suggestions: Vec<String>,
    #[serde(default)]
    pub score: Option<f32>,
}

/// System prompt for the LLM-based review.
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

/// Agent that critiques worker outputs using LLM review or rule-based checks.
pub struct CriticAgent {
    providers: Arc<Mutex<ProviderRegistry>>,
}

impl CriticAgent {
    pub fn new(providers: Arc<Mutex<ProviderRegistry>>) -> Self {
        Self { providers }
    }

    /// LLM-based deep review of worker output.
    ///
    /// If `model_id` is `None`, falls back to the registry's default model.
    /// Returns a default "Go" critique when no model is available or JSON parsing fails.
    pub async fn review(
        &self,
        original_task: &SubTask,
        worker_result: &WorkerResult,
        model_id: Option<&str>,
    ) -> Result<Critique> {
        // Determine which model to use — return Go if none available
        let mid = self.resolve_model_id(model_id).await;
        if mid.is_empty() {
            return Ok(self.default_go_critique());
        }

        // Check that the provider actually exists in the registry
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

        let provider = match { let registry = self.providers.lock().await; registry.get(&mid) } {
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

        // Try to parse the extracted JSON
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

    /// Rule-based quick review without calling an LLM.
    ///
    /// Returns `None` when no obvious issues are found.
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

        // No obvious issues
        None
    }

    /// Resolve the model ID to use for review, returning empty string if unavailable.
    async fn resolve_model_id(&self, model_id: Option<&str>) -> String {
        match model_id {
            Some(id) if !id.is_empty() => id.to_string(),
            _ => {
                let registry = self.providers.lock().await;
                registry.default_model_id().to_string()
            }
        }
    }

    /// Return a default "Go" critique (pass).
    fn default_go_critique(&self) -> Critique {
        Critique {
            decision: CritiqueDecision::Go,
            issues: vec![],
            suggestions: vec![],
            score: None,
        }
    }
}

/// Trait for components that can review worker outputs.
#[async_trait]
pub trait Reviewable: Send + Sync {
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
    use crate::workers::WorkerKind;

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
        let providers = Arc::new(Mutex::new(ProviderRegistry::new(
            &crate::config::AppConfig::default(),
        )));
        let agent = CriticAgent::new(providers);
        let task = make_task("t1", "Test", "Do something");
        let result = make_result(WorkerKind::Thinker, "t1", "Some output");

        // With empty model_id, review should return default Go
        let rt = tokio::runtime::Runtime::new().unwrap();
        let critique = rt.block_on(agent.review(&task, &result, Some(""))).unwrap();
        assert_eq!(critique.decision, CritiqueDecision::Go);
    }
}
