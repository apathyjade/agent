//! worker 实现：每个 worker 负责一类子任务，并支持 `@@command` 形式的内嵌指令。
//!
//! 公共契约在 [`WorkerAgent`]；具体 worker 只实现 `execute`，
//! 需要「带反馈重试」时使用 trait 的默认实现。

pub mod code_editor;
pub mod code_explorer;
pub mod mcp_bridge;
pub mod memory;
pub mod shell;
pub mod thinker;
pub mod web;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core::error::Result;

/// worker 重试时使用的轻量反馈结构（不依赖 critic 模块）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerFeedback {
    pub issues: Vec<String>,
    pub suggestions: Vec<String>,
}

impl WorkerFeedback {
    /// 用问题列表与建议列表构造反馈。
    pub fn new(issues: Vec<String>, suggestions: Vec<String>) -> Self {
        Self { issues, suggestions }
    }
}

/// worker 类型，决定任务由谁执行。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
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
    /// 返回用于日志与持久化的稳定字符串标识。
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

/// worker 执行完子任务后返回的结果。
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

/// 编排器下发给 worker 的子任务。
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

/// worker 的统一契约。
///
/// 实现方只需提供 `kind`、`description` 与 `execute`；
/// [`WorkerAgent::execute_with_feedback`] 提供了默认的「拼接反馈后重试」实现。
#[async_trait]
pub trait WorkerAgent: Send + Sync {
    /// 该 worker 的类型。
    fn kind(&self) -> WorkerKind;

    /// 供前端展示的能力描述。
    fn description(&self) -> &str;

    /// 执行一个子任务。
    async fn execute(&self, task: SubTask) -> Result<WorkerResult>;

    /// 带评审反馈重新执行：默认把反馈拼到指令前再调用 `execute`。
    async fn execute_with_feedback(
        &self,
        task: SubTask,
        feedback: &WorkerFeedback,
    ) -> Result<WorkerResult> {
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
