//! 任务图（DAG）：描述编排阶段要执行的子任务及其依赖关系。
//!
//! 图只描述结构与状态，不包含执行逻辑；`ready_nodes` 是调度器的唯一依据。

use serde::{Deserialize, Serialize};

use crate::agent::workers::{SubTask, WorkerKind};

/// 单个任务节点在 DAG 中的状态。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum NodeStatus {
    #[default]
    Pending,
    Running,
    Completed,
    Failed(String),
    Skipped,
}


/// 任务图中的一个节点 —— 由某个 worker 执行的一个原子子任务。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNode {
    pub id: String,
    pub label: String,
    pub worker_kind: WorkerKind,
    pub instruction: String,
    #[serde(default)]
    pub status: NodeStatus,
    #[serde(default)]
    pub result_summary: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    /// 本任务使用的模型 ID（`None` 表示使用默认模型）。
    #[serde(default)]
    pub model_id: Option<String>,
}

impl TaskNode {
    /// 创建一个状态为 `Pending` 的任务节点。
    pub fn new(
        id: String,
        label: String,
        worker_kind: WorkerKind,
        instruction: String,
    ) -> Self {
        Self {
            id,
            label,
            worker_kind,
            instruction,
            status: NodeStatus::Pending,
            result_summary: None,
            error: None,
            duration_ms: None,
            model_id: None,
        }
    }

    /// 设置本任务使用的模型 ID，返回自身以便链式调用。
    pub fn with_model_id(mut self, model_id: String) -> Self {
        self.model_id = Some(model_id);
        self
    }

    /// 把节点转换成可交给 worker 执行的子任务。
    pub fn to_sub_task(&self) -> SubTask {
        SubTask {
            id: self.id.clone(),
            label: self.label.clone(),
            instruction: self.instruction.clone(),
            worker_kind: self.worker_kind.clone(),
            model_id: self.model_id.clone(),
            max_tokens: None,
            temperature: None,
            context: None,
        }
    }
}

/// 两个任务节点之间的有向边。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEdge {
    pub from: String,
    pub to: String,
}

impl TaskEdge {
    /// 创建一条从 `from` 指向 `to` 的依赖边。
    pub fn new(from: String, to: String) -> Self {
        Self { from, to }
    }
}

/// 待编排子任务构成的有向无环图（DAG）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskGraph {
    pub id: String,
    pub goal: String,
    pub nodes: Vec<TaskNode>,
    pub edges: Vec<TaskEdge>,
}

impl TaskGraph {
    /// 创建一个空图（没有节点，因此视为已完成）。
    pub fn new(id: String, goal: String) -> Self {
        Self {
            id,
            goal,
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }

    /// 添加一个任务节点。
    pub fn add_node(&mut self, node: TaskNode) {
        self.nodes.push(node);
    }

    /// 添加一条依赖边：`to` 必须在 `from` 完成之后才能执行。
    pub fn add_edge(&mut self, from: &str, to: &str) {
        self.edges.push(TaskEdge::new(from.into(), to.into()));
    }

    /// 返回状态为 `Pending` 且所有依赖都已完成的节点。
    pub fn ready_nodes(&self) -> Vec<&TaskNode> {
        self.nodes
            .iter()
            .filter(|node| {
                if node.status != NodeStatus::Pending {
                    return false;
                }
                // 所有入边的起点都必须已完成
                let deps: Vec<&str> = self
                    .edges
                    .iter()
                    .filter(|e| e.to == node.id)
                    .map(|e| e.from.as_str())
                    .collect();
                if deps.is_empty() {
                    return true;
                }
                deps.iter().all(|dep_id| {
                    self.nodes
                        .iter()
                        .any(|n| n.id == **dep_id && n.status == NodeStatus::Completed)
                })
            })
            .collect()
    }

    /// 所有节点都完成时返回 `true`。
    pub fn is_complete(&self) -> bool {
        self.nodes.iter().all(|n| n.status == NodeStatus::Completed)
    }

    /// 存在失败节点时返回 `true`。
    pub fn has_failures(&self) -> bool {
        self.nodes
            .iter()
            .any(|n| matches!(n.status, NodeStatus::Failed(_)))
    }

    /// 按 id 查找节点。
    pub fn get_node(&self, id: &str) -> Option<&TaskNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// 按 id 查找节点并允许修改其状态。
    pub fn get_node_mut(&mut self, id: &str) -> Option<&mut TaskNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    /// 简单的拓扑排序：反复挑选依赖都已排入结果的节点。
    ///
    /// 存在环时无法排完，返回的列表会短于节点总数。
    pub fn topological_order(&self) -> Vec<&TaskNode> {
        let mut ordered: Vec<&TaskNode> = Vec::new();
        let mut visited: std::collections::HashSet<&str> = std::collections::HashSet::new();

        while ordered.len() < self.nodes.len() {
            for node in &self.nodes {
                if visited.contains(node.id.as_str()) {
                    continue;
                }
                let deps: Vec<&str> = self
                    .edges
                    .iter()
                    .filter(|e| e.to == node.id)
                    .map(|e| e.from.as_str())
                    .collect();
                let all_deps_visited = deps.iter().all(|d| visited.contains(*d));
                if all_deps_visited {
                    ordered.push(node);
                    visited.insert(node.id.as_str());
                }
            }
        }

        ordered
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::workers::WorkerKind;

    #[test]
    fn test_empty_graph_is_complete() {
        let graph = TaskGraph::new("g1".into(), "test".into());
        assert!(graph.is_complete());
        assert!(graph.ready_nodes().is_empty());
    }

    #[test]
    fn test_ready_nodes_no_deps() {
        let mut graph = TaskGraph::new("g2".into(), "test".into());
        graph.add_node(TaskNode::new(
            "a".into(),
            "A".into(),
            WorkerKind::Thinker,
            "think".into(),
        ));
        graph.add_node(TaskNode::new(
            "b".into(),
            "B".into(),
            WorkerKind::Thinker,
            "think".into(),
        ));
        let ready = graph.ready_nodes();
        assert_eq!(ready.len(), 2);
    }

    #[test]
    fn test_ready_nodes_with_deps() {
        let mut graph = TaskGraph::new("g3".into(), "test".into());
        graph.add_node(TaskNode::new(
            "a".into(),
            "A".into(),
            WorkerKind::Thinker,
            "think".into(),
        ));
        graph.add_node(TaskNode::new(
            "b".into(),
            "B".into(),
            WorkerKind::Thinker,
            "think".into(),
        ));
        graph.add_edge("a", "b");
        let ready = graph.ready_nodes();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "a");

        graph.get_node_mut("a").unwrap().status = NodeStatus::Completed;
        let ready = graph.ready_nodes();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "b");
    }

    #[test]
    fn test_topological_order() {
        let mut graph = TaskGraph::new("g4".into(), "test".into());
        graph.add_node(TaskNode::new(
            "a".into(),
            "A".into(),
            WorkerKind::Thinker,
            "think".into(),
        ));
        graph.add_node(TaskNode::new(
            "b".into(),
            "B".into(),
            WorkerKind::Thinker,
            "think".into(),
        ));
        graph.add_node(TaskNode::new(
            "c".into(),
            "C".into(),
            WorkerKind::Thinker,
            "think".into(),
        ));
        graph.add_edge("a", "c");
        graph.add_edge("b", "c");
        let order = graph.topological_order();
        assert_eq!(order.len(), 3);
        assert_eq!(order[2].id, "c");
    }

    #[test]
    fn test_has_failures() {
        let mut graph = TaskGraph::new("g5".into(), "test".into());
        graph.add_node(TaskNode::new(
            "a".into(),
            "A".into(),
            WorkerKind::Thinker,
            "think".into(),
        ));
        assert!(!graph.has_failures());
        graph.get_node_mut("a").unwrap().status = NodeStatus::Failed("error".into());
        assert!(graph.has_failures());
    }
}
