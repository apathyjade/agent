pub mod code_explorer;
pub mod thinker;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::Result;

/// Lightweight feedback struct for worker retry (no dependency on critic module).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerFeedback {
    pub issues: Vec<String>,
    pub suggestions: Vec<String>,
}

impl WorkerFeedback {
    pub fn new(issues: Vec<String>, suggestions: Vec<String>) -> Self {
        Self { issues, suggestions }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkerKind {
    Thinker,
    CodeExplorer,
    CodeEditor,
    Shell,
    Web,
    Memory,
    McpBridge,
}

impl WorkerKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkerKind::Thinker => "thinker",
            WorkerKind::CodeExplorer => "code_explorer",
            WorkerKind::CodeEditor => "code_editor",
            WorkerKind::Shell => "shell",
            WorkerKind::Web => "web",
            WorkerKind::Memory => "memory",
            WorkerKind::McpBridge => "mcp_bridge",
        }
    }
}

/// Result returned by a worker after executing a sub-task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerResult {
    pub worker: WorkerKind,
    pub task_id: String,
    pub content: String,
    #[serde(default)]
    pub metadata: Option<Value>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
}

/// Sub-task dispatched by the Orchestrator to a Worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubTask {
    pub id: String,
    pub label: String,
    pub instruction: String,
    pub worker_kind: WorkerKind,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub context: Option<Value>,
}

#[async_trait]
pub trait WorkerAgent: Send + Sync {
    fn kind(&self) -> WorkerKind;
    fn description(&self) -> &str;

    async fn execute(&self, task: SubTask) -> Result<WorkerResult>;

    async fn execute_with_feedback(
        &self,
        task: SubTask,
        feedback: &WorkerFeedback,
    ) -> Result<WorkerResult> {
        // Default: prepend feedback to instruction and retry
        let mut amended = task.clone();
        amended.instruction = format!(
            "Previous attempt had these issues:\n{}\n\nFeedback:\n{}\n\nPlease fix and retry.\n\n{}",
            feedback.issues.join("\n"),
            feedback.suggestions.join("\n"),
            task.instruction
        );
        self.execute(amended).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worker_kind_as_str() {
        assert_eq!(WorkerKind::Thinker.as_str(), "thinker");
        assert_eq!(WorkerKind::CodeExplorer.as_str(), "code_explorer");
        assert_eq!(WorkerKind::CodeEditor.as_str(), "code_editor");
        assert_eq!(WorkerKind::Shell.as_str(), "shell");
        assert_eq!(WorkerKind::Web.as_str(), "web");
        assert_eq!(WorkerKind::Memory.as_str(), "memory");
        assert_eq!(WorkerKind::McpBridge.as_str(), "mcp_bridge");
    }

    #[test]
    fn test_sub_task_serialization() {
        let task = SubTask {
            id: "test_1".into(),
            label: "Test task".into(),
            instruction: "Do something".into(),
            worker_kind: WorkerKind::Thinker,
            model_id: None,
            max_tokens: Some(1024),
            temperature: Some(0.3),
            context: None,
        };
        let json = serde_json::to_string(&task).unwrap();
        assert!(json.contains("\"worker_kind\":\"thinker\""));
        let deserialized: SubTask = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "test_1");
    }
}
