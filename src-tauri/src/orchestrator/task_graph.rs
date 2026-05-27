use serde::{Deserialize, Serialize};

use crate::workers::{SubTask, WorkerKind};

/// Status of a single task node in the DAG.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    Pending,
    Running,
    Completed,
    Failed(String),
    Skipped,
}

impl Default for NodeStatus {
    fn default() -> Self {
        Self::Pending
    }
}

/// A single node in the task graph — one atomic sub-task to be executed by a worker.
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
}

impl TaskNode {
    pub fn new(id: String, label: String, worker_kind: WorkerKind, instruction: String) -> Self {
        Self {
            id,
            label,
            worker_kind,
            instruction,
            status: NodeStatus::Pending,
            result_summary: None,
            error: None,
            duration_ms: None,
        }
    }

    pub fn to_sub_task(&self) -> SubTask {
        SubTask {
            id: self.id.clone(),
            label: self.label.clone(),
            instruction: self.instruction.clone(),
            worker_kind: self.worker_kind.clone(),
            model_id: None,
            max_tokens: None,
            temperature: None,
            context: None,
        }
    }
}

/// Directed edge between two task nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEdge {
    pub from: String,
    pub to: String,
}

impl TaskEdge {
    pub fn new(from: String, to: String) -> Self {
        Self { from, to }
    }
}

/// A DAG (directed acyclic graph) of sub-tasks to be orchestrated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskGraph {
    pub id: String,
    pub goal: String,
    pub nodes: Vec<TaskNode>,
    pub edges: Vec<TaskEdge>,
}

impl TaskGraph {
    pub fn new(id: String, goal: String) -> Self {
        Self {
            id,
            goal,
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }

    pub fn add_node(&mut self, node: TaskNode) {
        self.nodes.push(node);
    }

    pub fn add_edge(&mut self, from: &str, to: &str) {
        self.edges.push(TaskEdge::new(from.into(), to.into()));
    }

    /// Returns nodes that are Pending AND whose dependencies (if any) are all Completed.
    pub fn ready_nodes(&self) -> Vec<&TaskNode> {
        self.nodes
            .iter()
            .filter(|node| {
                if node.status != NodeStatus::Pending {
                    return false;
                }
                // All incoming edges must have their `from` node completed
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

    /// Returns true when all nodes are Completed.
    pub fn is_complete(&self) -> bool {
        self.nodes.iter().all(|n| n.status == NodeStatus::Completed)
    }

    /// Returns true when any node has Failed status.
    pub fn has_failures(&self) -> bool {
        self.nodes
            .iter()
            .any(|n| matches!(n.status, NodeStatus::Failed(_)))
    }

    pub fn get_node(&self, id: &str) -> Option<&TaskNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn get_node_mut(&mut self, id: &str) -> Option<&mut TaskNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    /// Simple topological sort: repeatedly pick nodes whose dependencies are already in the result.
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
    use crate::workers::WorkerKind;

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
