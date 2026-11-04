use tauri::{Emitter, AppHandle, State};
use tokio::sync::mpsc;

use crate::core::error::Result;
use crate::agent::orchestrator::agent::OrchestrationEvent;
use crate::agent::orchestrator::event_bridge::spawn_event_bridge;
use crate::app::state::AppState;

/// IPC command: invoke the OrchestratorAgent with a natural language goal.
/// Routes the message through the hierarchical agent system for deep reasoning.
/// Forwards orchestration events to the frontend via Tauri events.
#[tauri::command]
pub async fn orchestrate_message(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    goal: String,
    _session_id: String,
) -> Result<String> {
    // Create event channel and wire to frontend
    let (tx, rx) = mpsc::channel::<OrchestrationEvent>(64);
    spawn_event_bridge(app_handle.clone(), rx);

    state.orchestrator.set_event_tx(tx);

    // Execute
    let result = state.orchestrator.process_goal(&goal, None).await?;

    // Emit done event
    let _ = app_handle.emit("orchestrator_done", &result);
    let _ = app_handle.emit("stream_chunk", crate::app::commands::StreamChunk {
        content: result.clone(),
        done: true,
        tool_calls: None,
        phase: Some("done".to_string()),
    });

    Ok(result)
}
