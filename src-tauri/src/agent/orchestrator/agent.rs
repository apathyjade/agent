//! 编排 agent：把目标拆成 DAG 子任务，分发给 worker，经 Critic 评审后汇总输出。
//!
//! 本模块只负责「编排生命周期」：阶段推进、节点调度、事件上报与结果汇总。
//! 具体执行由 `Dispatcher` 交给 worker；质量评审由 `CriticAgent` 完成。

use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

use crate::agent::critic::{CriticAgent, CritiqueDecision};
use crate::core::error::{AppError, Result};
use crate::agent::orchestrator::dispatcher::Dispatcher;
use crate::agent::orchestrator::task_graph::{NodeStatus, TaskGraph};
use crate::agent::workers::WorkerKind;

/// 节点结果摘要的最大字符数。
const NODE_SUMMARY_CHARS: usize = 120;

/// 编排生命周期的阶段。
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

/// 编排过程中发出的事件（用于前端流式展示）。
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

/// 编排 agent，管理完整的编排生命周期。
///
/// 把目标转换成子任务 DAG，通过 `Dispatcher` 分发给 worker，使用
/// `CriticAgent` 校验产出质量，向前端推送进度事件，最后把各节点结果
/// 汇总为最终输出。
pub struct OrchestratorAgent {
    dispatcher: Dispatcher,
    critic: Arc<CriticAgent>,
    event_tx: Arc<Mutex<Option<tokio::sync::mpsc::Sender<OrchestrationEvent>>>>,
    max_critique_rounds: u32,
}

impl OrchestratorAgent {
    /// 用给定的分发器与评审器构造编排 agent，默认最多 3 轮评审。
    pub fn new(dispatcher: Dispatcher, critic: Arc<CriticAgent>) -> Self {
        Self {
            dispatcher,
            critic,
            event_tx: Arc::new(Mutex::new(None)),
            max_critique_rounds: 3,
        }
    }

    /// 绑定事件通道；通道被占用时记录警告并放弃本次绑定。
    pub fn set_event_tx(&self, tx: tokio::sync::mpsc::Sender<OrchestrationEvent>) {
        let mut lock = self.event_tx.try_lock();
        if let Ok(ref mut slot) = lock {
            **slot = Some(tx);
        } else {
            log::warn!("Failed to set event_tx — lock contended");
        }
    }

    /// 设置单节点的最大评审轮数。
    pub fn with_max_critique_rounds(mut self, rounds: u32) -> Self {
        self.max_critique_rounds = rounds;
        self
    }

    /// 只读访问内部的分发器（用于注册 worker 或测试断言）。
    pub fn dispatcher(&self) -> &Dispatcher {
        &self.dispatcher
    }

    /// 发送事件；未绑定通道时静默丢弃。
    async fn emit(&self, event: OrchestrationEvent) {
        if let Some(tx) = &*self.event_tx.lock().await {
            let _ = tx.send(event).await;
        }
    }

    /// 执行完整任务图。
    ///
    /// 1. 发出 `Planning` 阶段；
    /// 2. 循环：取就绪节点并逐个执行；
    ///    - 没有就绪节点但图未完成 → 报「停滞」错误；
    ///    - 出现失败节点 → 带失败详情返回错误；
    /// 3. 发出 `Synthesizing` 阶段，汇总结果并发出 `SynthesizedOutput`；
    /// 4. 返回汇总文本。
    pub async fn execute_graph(&self, graph: &mut TaskGraph) -> Result<String> {
        self.emit(OrchestrationEvent::PhaseChanged(OrchestrationPhase::Planning))
            .await;

        loop {
            // 失败优先：一次性报出所有失败节点，避免逐个重试。
            if graph.has_failures() {
                return Err(AppError::Orchestrator(format!(
                    "Task graph execution failed — nodes: {}",
                    failed_node_descriptions(graph).join("; ")
                )));
            }

            if graph.is_complete() {
                break;
            }

            let ready_ids = ready_node_ids(graph);
            if ready_ids.is_empty() {
                return Err(AppError::Orchestrator(
                    "Task graph stalled — no ready nodes but graph is not complete".into(),
                ));
            }

            for node_id in &ready_ids {
                self.run_node(graph, node_id).await?;
            }
        }

        self.emit(OrchestrationEvent::PhaseChanged(OrchestrationPhase::Synthesizing))
            .await;

        let synthesized = self.synthesize_results(graph).await?;

        self.emit(OrchestrationEvent::SynthesizedOutput(synthesized.clone()))
            .await;
        self.emit(OrchestrationEvent::PhaseChanged(OrchestrationPhase::Done))
            .await;

        Ok(synthesized)
    }

    /// 执行一个已就绪节点：标记状态、分发、回写结果并上报事件。
    ///
    /// 节点失败时先把失败状态写回图，再向上返回错误，保证调用方能看到
    /// 失败详情。
    async fn run_node(&self, graph: &mut TaskGraph, node_id: &str) -> Result<()> {
        let (node_label, node_worker) = {
            let node = graph
                .get_node(node_id)
                .ok_or_else(|| AppError::Orchestrator(format!("Node '{}' not found", node_id)))?;
            (node.label.clone(), node.worker_kind.clone())
        };

        self.emit(OrchestrationEvent::TaskStarted {
            task_id: node_id.to_string(),
            label: node_label.clone(),
            worker: node_worker,
        })
        .await;

        if let Some(mut_node) = graph.get_node_mut(node_id) {
            mut_node.status = NodeStatus::Running;
        }

        let start = Instant::now();
        match self.execute_node(graph, node_id).await {
            Ok(content) => {
                let duration = start.elapsed().as_millis() as u64;
                let summary = summarize(&content);

                if let Some(mut_node) = graph.get_node_mut(node_id) {
                    mut_node.status = NodeStatus::Completed;
                    mut_node.result_summary = Some(summary.clone());
                    mut_node.duration_ms = Some(duration);
                }

                self.emit(OrchestrationEvent::TaskCompleted {
                    task_id: node_id.to_string(),
                    label: node_label,
                    result_summary: summary,
                    duration_ms: duration,
                })
                .await;
                Ok(())
            }
            Err(e) => {
                let duration = start.elapsed().as_millis() as u64;
                let err_str = e.to_string();

                if let Some(mut_node) = graph.get_node_mut(node_id) {
                    mut_node.status = NodeStatus::Failed(err_str.clone());
                    mut_node.duration_ms = Some(duration);
                }

                self.emit(OrchestrationEvent::TaskFailed {
                    task_id: node_id.to_string(),
                    label: node_label,
                    error: err_str.clone(),
                })
                .await;

                Err(AppError::Orchestrator(format!(
                    "Node '{}' failed: {}",
                    node_id, err_str
                )))
            }
        }
    }

    /// 执行单个节点，并应用 Critic 反思循环。
    ///
    /// 流程：
    /// 1. 从图中取节点并转换为 `SubTask`；
    /// 2. 最多尝试 `max_critique_rounds` 轮：
    ///    - 分发给 worker；
    ///    - 规则快审判定 Revise 且仍有轮次时，带反馈重试；
    ///    - LLM 深审：Go 直接返回，Revise 且仍有轮次则继续，Escalate 报错；
    /// 3. 轮次用尽后接受当前输出并记录警告。
    async fn execute_node(&self, graph: &TaskGraph, node_id: &str) -> Result<String> {
        let node = graph.get_node(node_id).ok_or_else(|| {
            AppError::Orchestrator(format!("Node '{}' not found in graph", node_id))
        })?;

        let mut current_task = node.to_sub_task();

        for round in 0..self.max_critique_rounds {
            if round > 0 {
                self.emit(OrchestrationEvent::PhaseChanged(OrchestrationPhase::Reflecting))
                    .await;
            }

            // 指令中已经包含上一轮追加的反馈。
            let worker_result = self.dispatcher.dispatch(&current_task).await?;

            // 规则快审：成本低，能在明显问题时省掉一次 LLM 调用。
            if let Some(quick) = CriticAgent::quick_review(&current_task, &worker_result) {
                if quick.decision == CritiqueDecision::Revise && round + 1 < self.max_critique_rounds
                {
                    current_task.instruction =
                        build_retry_instruction(&node.instruction, &quick.issues, &quick.suggestions);
                    self.emit_critique(node_id, CritiqueDecision::Revise, quick.issues.clone())
                        .await;
                    continue;
                }
            }

            // LLM 深审。
            let critique = self
                .critic
                .review(&current_task, &worker_result, None)
                .await?;

            self.emit_critique(node_id, critique.decision.clone(), critique.issues.clone())
                .await;

            match critique.decision {
                CritiqueDecision::Go => {
                    self.emit(OrchestrationEvent::PhaseChanged(OrchestrationPhase::Executing))
                        .await;
                    return Ok(worker_result.content);
                }
                CritiqueDecision::Revise if round + 1 < self.max_critique_rounds => {
                    current_task.instruction = build_retry_instruction(
                        &node.instruction,
                        &critique.issues,
                        &critique.suggestions,
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
                    // 轮次用尽：接受当前输出，避免整条链路失败。
                    log::warn!(
                        "Node '{}' exceeded max critique rounds ({}), accepting output",
                        node_id,
                        self.max_critique_rounds
                    );
                    return Ok(worker_result.content);
                }
            }
        }

        // 理论上不可达：轮次耗尽仍未返回，说明没有产出可接受的输出。
        Err(AppError::Orchestrator(format!(
            "Node '{}' produced no acceptable output after {} critique rounds",
            node_id, self.max_critique_rounds
        )))
    }

    /// 上报一条 critic 评审结果。
    async fn emit_critique(
        &self,
        node_id: &str,
        decision: CritiqueDecision,
        issues: Vec<String>,
    ) {
        self.emit(OrchestrationEvent::CritiqueReceived {
            task_id: node_id.to_string(),
            decision,
            issues,
        })
        .await;
    }

    /// 把已完成节点的结果汇总成一段连贯的最终输出。
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

    /// 便捷入口：单个目标 → 完整编排流程。
    ///
    /// 接收自然语言目标，自动拆解、执行并返回结果。当前只创建一个 Thinker
    /// 任务；后续阶段会改为基于 LLM 的拆解。
    pub async fn process_goal(&self, goal: &str, model_id: Option<&str>) -> Result<String> {
        let mut graph = TaskGraph::new("goal_1".into(), goal.into());

        let mut node = crate::agent::orchestrator::task_graph::TaskNode::new(
            "task_1".into(),
            format!("Process: {}", goal),
            WorkerKind::Thinker,
            goal.into(),
        );
        if let Some(mid) = model_id {
            node = node.with_model_id(mid.to_string());
        }
        graph.add_node(node);

        self.execute_graph(&mut graph).await
    }
}

/// 汇总失败节点的 `id: error` 描述，用于一次性报出所有失败原因。
fn failed_node_descriptions(graph: &TaskGraph) -> Vec<String> {
    graph
        .nodes
        .iter()
        .filter_map(|n| {
            if let NodeStatus::Failed(ref err) = n.status {
                Some(format!("{}: {}", n.id, err))
            } else {
                None
            }
        })
        .collect()
}

/// 返回当前所有可执行节点的 id。
fn ready_node_ids(graph: &TaskGraph) -> Vec<String> {
    graph.ready_nodes().iter().map(|n| n.id.clone()).collect()
}

/// 把节点输出截成摘要，超长时追加省略号（按字符截断，不会切断中文）。
fn summarize(content: &str) -> String {
    if content.chars().count() > NODE_SUMMARY_CHARS {
        format!(
            "{}…",
            content.chars().take(NODE_SUMMARY_CHARS).collect::<String>()
        )
    } else {
        content.to_string()
    }
}

/// 把上一轮的 issues 与 suggestions 拼成重试指令，并保留原始任务描述。
fn build_retry_instruction(original: &str, issues: &[String], suggestions: &[String]) -> String {
    format!(
        "Previous attempt had these issues:\n{}\n\nFeedback:\n{}\n\nPlease fix and retry.\n\n{}",
        issues.join("\n"),
        suggestions.join("\n"),
        original
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::critic::CriticAgent;
    use crate::agent::orchestrator::dispatcher::Dispatcher;
    use crate::agent::orchestrator::task_graph::TaskNode;
    use crate::agent::workers::{SubTask, WorkerAgent, WorkerResult};
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
            crate::llm::provider::ProviderRegistry::new(&[]),
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
            crate::llm::provider::ProviderRegistry::new(&[]),
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
