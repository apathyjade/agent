use std::sync::Arc;
use std::time::Instant;

use crate::critic::{CriticAgent, CritiqueDecision};
use crate::error::{AppError, Result};
use crate::orchestrator::dispatcher::Dispatcher;
use crate::orchestrator::task_graph::{NodeStatus, TaskGraph};
use crate::workers::WorkerKind;

/// Phase of the orchestration lifecycle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OrchestrationPhase {
    Idle,
    Analyzing,
    Planning,
    Executing,
    Reflecting,
    Synthesizing,
    Done,
}

/// Event emitted during orchestration (for frontend streaming).
#[derive(Debug, Clone)]
pub enum OrchestrationEvent {
    PhaseChanged(OrchestrationPhase),
    TaskStarted {
        task_id: String,
        label: String,
        worker: WorkerKind,
    },
    TaskCompleted {
        task_id: String,
        label: String,
        result_summary: String,
        duration_ms: u64,
    },
    TaskFailed {
        task_id: String,
        label: String,
        error: String,
    },
    CritiqueReceived {
        task_id: String,
        decision: CritiqueDecision,
        issues: Vec<String>,
    },
    Thinking {
        content: String,
    },
    SynthesizedOutput(String),
}

/// The OrchestratorAgent manages the full orchestration lifecycle:
/// process goals through a DAG of sub-tasks, dispatch to workers via Dispatcher,
/// apply CriticAgent for output quality validation, emit events for frontend progress,
/// and synthesize results into final output.
pub struct OrchestratorAgent {
    dispatcher: Dispatcher,
    critic: Arc<CriticAgent>,
    event_tx: Option<tokio::sync::mpsc::Sender<OrchestrationEvent>>,
    max_critique_rounds: u32,
}

impl OrchestratorAgent {
    pub fn new(dispatcher: Dispatcher, critic: Arc<CriticAgent>) -> Self {
        Self {
            dispatcher,
            critic,
            event_tx: None,
            max_critique_rounds: 3,
        }
    }

    pub fn with_event_channel(
        mut self,
        tx: tokio::sync::mpsc::Sender<OrchestrationEvent>,
    ) -> Self {
        self.event_tx = Some(tx);
        self
    }

    pub fn with_max_critique_rounds(mut self, rounds: u32) -> Self {
        self.max_critique_rounds = rounds;
        self
    }

    pub fn dispatcher(&self) -> &Dispatcher {
        &self.dispatcher
    }

    async fn emit(&self, event: OrchestrationEvent) {
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(event).await;
        }
    }

    /// Execute a full task graph through the orchestration pipeline.
    ///
    /// 1. Emit PhaseChanged::Planning
    /// 2. Loop: get ready_nodes() → for each, call execute_node()
    ///    - If no ready nodes and not complete → Err(AppError::Orchestrator("stalled"))
    ///    - If any node Failed → Err with failed node details
    /// 3. Emit synthesizing → call synthesize_results → emit SynthesizedOutput
    /// 4. Return synthesized string
    pub async fn execute_graph(&self, graph: &mut TaskGraph) -> Result<String> {
        self.emit(OrchestrationEvent::PhaseChanged(OrchestrationPhase::Planning))
            .await;

        // Execute loop: repeatedly process ready nodes
        loop {
            // Check for failures first
            if graph.has_failures() {
                let failed_nodes: Vec<String> = graph
                    .nodes
                    .iter()
                    .filter_map(|n| {
                        if let NodeStatus::Failed(ref err) = n.status {
                            Some(format!("{}: {}", n.id, err))
                        } else {
                            None
                        }
                    })
                    .collect();
                return Err(AppError::Orchestrator(format!(
                    "Task graph execution failed — nodes: {}",
                    failed_nodes.join("; ")
                )));
            }

            // Check if graph is complete
            if graph.is_complete() {
                break;
            }

            let ready_ids: Vec<String> = graph
                .ready_nodes()
                .iter()
                .map(|n| n.id.clone())
                .collect();

            if ready_ids.is_empty() {
                return Err(AppError::Orchestrator(
                    "Task graph stalled — no ready nodes but graph is not complete".into(),
                ));
            }

            // Process each ready node
            for node_id in &ready_ids {
                // Borrow graph immutably to read node data
                let (node_label, node_worker) = {
                    let node = graph.get_node(node_id).ok_or_else(|| {
                        AppError::Orchestrator(format!("Node '{}' not found", node_id))
                    })?;
                    (node.label.clone(), node.worker_kind.clone())
                };

                self.emit(OrchestrationEvent::TaskStarted {
                    task_id: node_id.clone(),
                    label: node_label.clone(),
                    worker: node_worker,
                })
                .await;

                // Mark node as Running
                if let Some(mut_node) = graph.get_node_mut(node_id) {
                    mut_node.status = NodeStatus::Running;
                }

                // Execute the node (immutable read of graph)
                let start = Instant::now();
                match self.execute_node(graph, node_id).await {
                    Ok(content) => {
                        let duration = start.elapsed().as_millis() as u64;
                        // Safe UTF-8 char boundary slicing: take first 120 chars, not bytes
                        let summary = if content.chars().count() > 120 {
                            format!("{}…", content.chars().take(120).collect::<String>())
                        } else {
                            content.clone()
                        };

                        if let Some(mut_node) = graph.get_node_mut(node_id) {
                            mut_node.status = NodeStatus::Completed;
                            mut_node.result_summary = Some(summary.clone());
                            mut_node.duration_ms = Some(duration);
                        }

                        self.emit(OrchestrationEvent::TaskCompleted {
                            task_id: node_id.clone(),
                            label: node_label,
                            result_summary: summary,
                            duration_ms: duration,
                        })
                        .await;
                    }
                    Err(e) => {
                        let duration = start.elapsed().as_millis() as u64;
                        let err_str = e.to_string();

                        if let Some(mut_node) = graph.get_node_mut(node_id) {
                            mut_node.status = NodeStatus::Failed(err_str.clone());
                            mut_node.duration_ms = Some(duration);
                        }

                        self.emit(OrchestrationEvent::TaskFailed {
                            task_id: node_id.clone(),
                            label: node_label,
                            error: err_str.clone(),
                        })
                        .await;

                        return Err(AppError::Orchestrator(format!(
                            "Node '{}' failed: {}",
                            node_id, err_str
                        )));
                    }
                }
            }
        }

        // Synthesize results
        self.emit(OrchestrationEvent::PhaseChanged(OrchestrationPhase::Synthesizing))
            .await;

        let synthesized = self.synthesize_results(graph).await?;

        self.emit(OrchestrationEvent::SynthesizedOutput(synthesized.clone()))
            .await;
        self.emit(OrchestrationEvent::PhaseChanged(OrchestrationPhase::Done))
            .await;

        Ok(synthesized)
    }

    /// Execute a single node with Critic reflection loop.
    ///
    /// 1. Get node from graph, convert to SubTask
    /// 2. Try up to max_critique_rounds times:
    ///    - Dispatch to worker
    ///    - quick_review from CriticAgent
    ///    - If Revise and rounds remain: WorkerFeedback → retry
    ///    - LLM-based review from CriticAgent
    ///    - If Go → return content
    ///    - If Revise and rounds remain → continue
    ///    - If Escalate → error
    /// 3. After max rounds → accept output (log warning)
    async fn execute_node(&self, graph: &TaskGraph, node_id: &str) -> Result<String> {
        let node = graph
            .get_node(node_id)
            .ok_or_else(|| AppError::Orchestrator(format!("Node '{}' not found in graph", node_id)))?;

        let mut current_task = node.to_sub_task();

        for round in 0..self.max_critique_rounds {
            if round > 0 {
                self.emit(OrchestrationEvent::PhaseChanged(OrchestrationPhase::Reflecting))
                    .await;
            }

            // Dispatch to worker (instruction already incorporates any prior feedback)
            let worker_result = self.dispatcher.dispatch(&current_task).await?;

            // Rule-based quick review
            if let Some(quick) = CriticAgent::quick_review(&current_task, &worker_result) {
                if quick.decision == CritiqueDecision::Revise && round + 1 < self.max_critique_rounds
                {
                    current_task.instruction = format!(
                        "Previous attempt had these issues:\n{}\n\nFeedback:\n{}\n\nPlease fix and retry.\n\n{}",
                        quick.issues.join("\n"),
                        quick.suggestions.join("\n"),
                        node.instruction
                    );
                    self.emit(OrchestrationEvent::CritiqueReceived {
                        task_id: node_id.into(),
                        decision: CritiqueDecision::Revise,
                        issues: quick.issues.clone(),
                    })
                    .await;
                    continue;
                }
            }

            // LLM-based deep review
            let critique = self
                .critic
                .review(&current_task, &worker_result, None)
                .await?;

            self.emit(OrchestrationEvent::CritiqueReceived {
                task_id: node_id.into(),
                decision: critique.decision.clone(),
                issues: critique.issues.clone(),
            })
            .await;

            match critique.decision {
                CritiqueDecision::Go => {
                    self.emit(OrchestrationEvent::PhaseChanged(OrchestrationPhase::Executing))
                        .await;
                    return Ok(worker_result.content);
                }
                CritiqueDecision::Revise if round + 1 < self.max_critique_rounds => {
                    current_task.instruction = format!(
                        "Previous attempt had these issues:\n{}\n\nFeedback:\n{}\n\nPlease fix and retry.\n\n{}",
                        critique.issues.join("\n"),
                        critique.suggestions.join("\n"),
                        node.instruction
                    );
                    continue;
                }
                CritiqueDecision::Escalate => {
                    return Err(AppError::Orchestrator(format!(
                        "Critic escalated node '{}': {}",
                        node_id,
                        critique.issues.join("; ")
                    )));
                }
                _ => {
                    // Last round or no rounds left — accept output with warning
                    log::warn!(
                        "Node '{}' exceeded max critique rounds ({}), accepting output",
                        node_id,
                        self.max_critique_rounds
                    );
                    return Ok(worker_result.content);
                }
            }
        }

        // Should not reach here — if we exhausted rounds without returning,
        // accept whatever we have as a fallback.
        Err(AppError::Orchestrator(format!(
            "Node '{}' produced no acceptable output after {} critique rounds",
            node_id, self.max_critique_rounds
        )))
    }

    /// Synthesize all completed node results into a coherent final output.
    async fn synthesize_results(&self, graph: &TaskGraph) -> Result<String> {
        let mut output = String::new();
        output.push_str(&format!("# Execution Results: {}\n\n", graph.goal));

        for node in &graph.nodes {
            match &node.status {
                NodeStatus::Completed => {
                    let summary = node
                        .result_summary
                        .as_deref()
                        .unwrap_or("(no summary)");
                    let duration = node
                        .duration_ms
                        .map(|ms| format!("{}ms", ms))
                        .unwrap_or_else(|| "unknown".into());
                    output.push_str(&format!(
                        "## {} — {}\n- Worker: {:?}\n- Duration: {}\n- Result: {}\n\n",
                        node.id, node.label, node.worker_kind, duration, summary
                    ));
                }
                NodeStatus::Failed(err) => {
                    output.push_str(&format!(
                        "## {} — {}\n- **FAILED**: {}\n\n",
                        node.id, node.label, err
                    ));
                }
                NodeStatus::Skipped => {
                    output.push_str(&format!(
                        "## {} — {}\n- _Skipped_\n\n",
                        node.id, node.label
                    ));
                }
                NodeStatus::Pending | NodeStatus::Running => {
                    output.push_str(&format!(
                        "## {} — {}\n- _Not executed (status: {:?})_\n\n",
                        node.id, node.label, node.status
                    ));
                }
            }
        }

        Ok(output)
    }

    /// Convenience: single goal → full pipeline.
    ///
    /// Takes a natural language goal, auto-decomposes, executes, and returns the result.
    /// Currently creates a TaskGraph with one Thinker task.
    /// In future phases this will use LLM-based decomposition.
    pub async fn process_goal(&self, goal: &str, _model_id: Option<&str>) -> Result<String> {
        let mut graph = TaskGraph::new("goal_1".into(), goal.into());

        let node = crate::orchestrator::task_graph::TaskNode::new(
            "task_1".into(),
            format!("Process: {}", goal),
            WorkerKind::Thinker,
            goal.into(),
        );
        graph.add_node(node);

        self.execute_graph(&mut graph).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::critic::CriticAgent;
    use crate::orchestrator::dispatcher::Dispatcher;
    use crate::orchestrator::task_graph::TaskNode;
    use crate::workers::{SubTask, WorkerAgent, WorkerResult};
    use async_trait::async_trait;
    use std::sync::Arc;
    use tokio::sync::Mutex;

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
    fn test_orchestrator_dispatcher_register_and_list() {
        let mut d = Dispatcher::new();
        assert!(d.list_workers().is_empty());
        d.register(Box::new(MockWorker(WorkerKind::Thinker)));
        assert!(d.has_worker(&WorkerKind::Thinker));
        assert!(!d.has_worker(&WorkerKind::CodeExplorer));
        assert_eq!(d.list_workers().len(), 1);
    }

    #[tokio::test]
    async fn test_orchestrator_dispatcher_dispatch() {
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
    async fn test_orchestrator_dispatcher_missing_worker() {
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

    #[tokio::test]
    async fn test_orchestrator_empty_graph() {
        let dispatcher = Dispatcher::new();
        let providers = Arc::new(Mutex::new(
            crate::api::provider::ProviderRegistry::new(&crate::config::AppConfig::default()),
        ));
        let critic = Arc::new(CriticAgent::new(providers));
        let orchestrator = OrchestratorAgent::new(dispatcher, critic);
        let mut graph = TaskGraph::new("g1".into(), "empty test".into());
        let result = orchestrator.execute_graph(&mut graph).await;
        assert!(
            result.is_ok(),
            "empty graph should complete: {:?}",
            result.err()
        );
        assert!(graph.is_complete());
    }

    #[tokio::test]
    async fn test_orchestrator_single_task() {
        let mut dispatcher = Dispatcher::new();
        dispatcher.register(Box::new(MockWorker(WorkerKind::Thinker)));
        let providers = Arc::new(Mutex::new(
            crate::api::provider::ProviderRegistry::new(&crate::config::AppConfig::default()),
        ));
        let critic = Arc::new(CriticAgent::new(providers));
        let orchestrator = OrchestratorAgent::new(dispatcher, critic);
        let mut graph = TaskGraph::new("g2".into(), "single task test".into());
        graph.add_node(TaskNode::new(
            "s1".into(),
            "Test step".into(),
            WorkerKind::Thinker,
            "Say hello".into(),
        ));
        let result = orchestrator.execute_graph(&mut graph).await.unwrap();
        assert!(result.contains("Test step"));
        assert_eq!(
            graph.get_node("s1").unwrap().status,
            NodeStatus::Completed
        );
    }

    #[test]
    fn test_orchestration_phase_ordering() {
        assert_ne!(OrchestrationPhase::Idle as u8, OrchestrationPhase::Done as u8);
    }
}
