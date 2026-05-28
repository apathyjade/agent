use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::api::provider::{ProviderRegistry, chat_text};
use crate::error::Result;
use crate::workers::{SubTask, WorkerAgent, WorkerKind, WorkerResult};

const COT_SYSTEM_PROMPT: &str = r#"You are a deep thinking module. Structure your reasoning in these phases:

[STEP 1: Problem Analysis]
Analyze what the user truly needs. Identify implicit requirements, constraints, and success criteria.

[STEP 2: Context Gathering]
What information do I need? What do I already know? What assumptions am I making?

[STEP 3: Approach Exploration]
Consider 2-3 approaches. Evaluate trade-offs for each. Identify risks and unknowns.

[STEP 4: Selected Approach]
Choose the best approach. Justify the choice. Define success criteria.

[STEP 5: Execution Plan]
Step-by-step plan with clear checkpoints. Note parallelization opportunities.

Then provide your final, detailed analysis."#;

pub struct ThinkerWorker {
    providers: Arc<Mutex<ProviderRegistry>>,
}

impl ThinkerWorker {
    pub fn new(providers: Arc<Mutex<ProviderRegistry>>) -> Self {
        Self { providers }
    }
}

#[async_trait]
impl WorkerAgent for ThinkerWorker {
    fn kind(&self) -> WorkerKind {
        WorkerKind::Thinker
    }

    fn description(&self) -> &str {
        "Deep reasoning and chain-of-thought analysis. No external tools — pure LLM thinking."
    }

    async fn execute(&self, task: SubTask) -> Result<WorkerResult> {
        let start = std::time::Instant::now();

        let system_prompt = if let Some(ctx) = &task.context {
            format!("{}\n\nAdditional context:\n{}", COT_SYSTEM_PROMPT, serde_json::to_string_pretty(ctx).unwrap_or_default())
        } else {
            COT_SYSTEM_PROMPT.to_string()
        };

        let content = chat_text(
            &self.providers,
            task.model_id.as_deref(),
            &system_prompt,
            &task.instruction,
            task.max_tokens.map(|t| t as usize),
            task.temperature,
        ).await?;

        Ok(WorkerResult {
            worker: WorkerKind::Thinker,
            task_id: task.id,
            content,
            metadata: None,
            duration_ms: Some(start.elapsed().as_millis() as u64),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thinker_kind() {
        let providers = Arc::new(Mutex::new(
            crate::api::provider::ProviderRegistry::new(
                &crate::config::AppConfig::default(),
            ),
        ));
        let worker = ThinkerWorker::new(providers);
        assert_eq!(worker.kind(), WorkerKind::Thinker);
        assert!(!worker.description().is_empty());
    }
}
