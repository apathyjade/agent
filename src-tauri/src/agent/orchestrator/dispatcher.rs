//! worker 注册与任务分发。
//!
//! 分发器只按 `WorkerKind` 找到对应 worker 并调用，不关心 worker 内部实现；
//! 未注册的类型会直接返回错误，避免静默失败。

use std::collections::HashMap;

use crate::core::error::{AppError, Result};
use crate::agent::workers::{SubTask, WorkerAgent, WorkerKind, WorkerFeedback, WorkerResult};

/// 管理 worker 注册与任务分发。
pub struct Dispatcher {
    workers: HashMap<WorkerKind, Box<dyn WorkerAgent>>,
}

impl Dispatcher {
    /// 创建一个没有注册任何 worker 的分发器。
    pub fn new() -> Self {
        Self {
            workers: HashMap::new(),
        }
    }

    /// 注册 worker；同类型重复注册会覆盖旧实例。
    pub fn register(&mut self, worker: Box<dyn WorkerAgent>) {
        let kind = worker.kind();
        self.workers.insert(kind, worker);
    }

    /// 判断某个类型是否已注册 worker。
    pub fn has_worker(&self, kind: &WorkerKind) -> bool {
        self.workers.contains_key(kind)
    }

    /// 列出已注册的 worker 类型。
    pub fn list_workers(&self) -> Vec<WorkerKind> {
        self.workers.keys().cloned().collect()
    }

    /// 把子任务分发给对应 worker 执行。
    pub async fn dispatch(&self, task: &SubTask) -> Result<WorkerResult> {
        let worker = self.workers.get(&task.worker_kind).ok_or_else(|| {
            AppError::Orchestrator(format!(
                "No worker registered for kind: {:?}",
                task.worker_kind
            ))
        })?;
        worker.execute(task.clone()).await
    }

    /// 带反馈重试：把评审意见交给 worker 重新执行同一任务。
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
    use crate::agent::workers::WorkerAgent;
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
