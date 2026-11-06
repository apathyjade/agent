//! 前后端 IPC 通信的数据结构。
//!
//! 这些类型经 Tauri 的 `invoke` 与 `emit` 在前后端之间传递，
//! 属于对外契约——改动字段需要考虑前端兼容性。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 流式响应的一个增量片段。
///
/// 前端 `src-ui/src/types/index.ts` 中的 `StreamChunk` 与之对应。
#[derive(Clone, Serialize, Deserialize)]
pub struct StreamChunk {
    /// 本片段的文本内容；纯事件通知时为空串。
    pub content: String,
    /// 是否为流结束标记。
    pub done: bool,
    /// 本片段携带的工具调用事件；无工具事件时为 `None`。
    pub tool_calls: Option<Vec<ToolCallEvent>>,
    /// 当前阶段标识（如 `thinking` / `executing_tool` / `completed`），无阶段变化时为 `None`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
}

/// 一次工具调用的状态事件。
///
/// 前端按 `status` 区分渲染样式（`calling` / `completed` / `failed`）。
#[derive(Clone, Serialize, Deserialize)]
pub struct ToolCallEvent {
    /// 工具调用 id，与模型侧的调用 id 对应。
    pub id: String,
    /// 工具名称。
    pub name: String,
    /// 调用状态：`calling` / `completed` / `failed`。
    pub status: String,
    /// 工具返回内容；尚未返回时为 `None`。
    pub result: Option<String>,
    /// 工具是否执行失败。
    ///
    /// 来自 Rig 的 `ToolResult::is_error`，此前项目丢弃了该字段，
    /// 导致工具失败被一律上报为 `completed`。
    /// 带 `#[serde(default)]` 以兼容不认识该字段的旧前端。
    #[serde(default)]
    pub is_error: bool,
    /// 模型给出的参数；仅 `calling` 事件携带。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
}
