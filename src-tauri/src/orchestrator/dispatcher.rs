use std::collections::HashMap;

use crate::error::{AppError, Result};
use crate::workers::{SubTask, WorkerAgent, WorkerKind, WorkerFeedback, WorkerResult};

/// Manages worker registration and dispatch.
pub struct Dispatcher {
    workers: HashMap<WorkerKind, Box<dyn WorkerAgent>>,
}

impl Dispatcher {
    pub fn new() -> Self {
        Self {
            workers: HashMap::new(),
        }
    }

    pub fn register(&mut self, worker: Box<dyn WorkerAgent>) {
        let kind = worker.kind();
        self.workers.insert(kind, worker);
    }

    pub fn has_worker(&self, kind: &WorkerKind) -> bool {
        self.workers.contains_key(kind)
    }

    pub fn list_workers(&self) -> Vec<WorkerKind> {
        self.workers.keys().cloned().collect()
    }

    pub async fn dispatch(&self, task: &SubTask) -> Result<WorkerResult> {
        let worker = self.workers.get(&task.worker_kind).ok_or_else(|| {
            AppError::Orchestrator(format!(
                "No worker registered for kind: {:?}",
                task.worker_kind
            ))
        })?;
        worker.execute(task.clone()).await
    }

    pub async fn dispatch_with_feedback(
        &self,
        task: &SubTask,
        feedback: &WorkerFeedback,
    ) -> Result<WorkerResult> {
        let worker = self.workers.get(&task.worker_kind).ok_or_else(|| {
            AppError::Orchestrator(format!(
                "No worker registered for kind: {:?}",
                task.worker_kind
            ))
        })?;
        worker.execute_with_feedback(task.clone(), feedback).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workers::WorkerAgent;
    use async_trait::async_trait;

    struct MockWorker(WorkerKind);

    #[async_trait]
    impl WorkerAgent for MockWorker {
        fn kind(&self) -> WorkerKind {
            self.0.clone()
        }

        fn description(&self) -> &str {
            "mock worker for testing"
        }

        async fn execute(&self, task: SubTask) -> Result<WorkerResult> {
            Ok(WorkerResult {
                worker: self.0.clone(),
                task_id: task.id,
                content: format!("Mock result for: {}", task.label),
                metadata: None,
                duration_ms: Some(10),
            })
        }
    }

    #[test]
    fn test_dispatcher_register_and_list() {
        let mut d = Dispatcher::new();
        assert!(d.list_workers().is_empty());
        d.register(Box::new(MockWorker(WorkerKind::Thinker)));
        assert!(d.has_worker(&WorkerKind::Thinker));
        assert!(!d.has_worker(&WorkerKind::CodeExplorer));
        assert_eq!(d.list_workers().len(), 1);
    }

    #[tokio::test]
    async fn test_dispatcher_dispatch() {
        let mut d = Dispatcher::new();
        d.register(Box::new(MockWorker(WorkerKind::Thinker)));
        let task = SubTask {
            id: "t1".into(),
            label: "Test".into(),
            instruction: "Do it".into(),
            worker_kind: WorkerKind::Thinker,
            model_id: None,
            max_tokens: None,
            temperature: None,
            context: None,
        };
        let result = d.dispatch(&task).await.unwrap();
        assert!(result.content.contains("Mock result for: Test"));
    }

    #[tokio::test]
    async fn test_dispatcher_missing_worker() {
        let d = Dispatcher::new();
        let task = SubTask {
            id: "t1".into(),
            label: "Test".into(),
            instruction: "Do it".into(),
            worker_kind: WorkerKind::Thinker,
            model_id: None,
            max_tokens: None,
            temperature: None,
            context: None,
        };
        let result = d.dispatch(&task).await;
        assert!(result.is_err());
    }
}
