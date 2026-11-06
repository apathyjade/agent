//! 分层 agent 编排 IPC 命令。
//!
//! 与 `send_message` 的 fast/deep 路径不同，这里直接把目标交给
//! `OrchestratorAgent`（planner + 多 worker + critic），走完整的编排流程。

use tauri::{Emitter, AppHandle, State};
use tokio::sync::mpsc;

use crate::core::error::Result;
use crate::agent::orchestrator::agent::OrchestrationEvent;
use crate::agent::orchestrator::event_bridge::spawn_event_bridge;
use crate::app::state::AppState;

/// 用自然语言目标驱动 `OrchestratorAgent`，返回最终文本。
///
/// 编排事件通过 `spawn_event_bridge` 转发给前端（`orchestrator_*` 系列），
/// 结束时额外 emit 一个 `done` 阶段的 `stream_chunk`，让聊天窗口走统一的
/// 收尾逻辑。`_session_id` 目前不参与编排，仅保留给调用方标识来源。
#[tauri::command]
pub async fn orchestrate_message(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    goal: String,
    _session_id: String,
) -> Result<String> {
    // 建立事件通道并桥接到前端。
    let (tx, rx) = mpsc::channel::<OrchestrationEvent>(64);
    spawn_event_bridge(app_handle.clone(), rx);

    state.orchestrator.set_event_tx(tx);

    // 执行。
    let result = state.orchestrator.process_goal(&goal, None).await?;

    // 发出结束事件。
    let _ = app_handle.emit("orchestrator_done", &result);
    let _ = app_handle.emit("stream_chunk", crate::app::commands::StreamChunk {
        content: result.clone(),
        done: true,
        tool_calls: None,
        phase: Some("done".to_string()),
    });

    Ok(result)
}
