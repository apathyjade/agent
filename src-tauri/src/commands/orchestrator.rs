use tauri::{Emitter, AppHandle, State};
use tokio::sync::mpsc;

use crate::error::Result;
use crate::orchestrator::agent::OrchestrationEvent;
use crate::orchestrator::event_bridge::spawn_event_bridge;
use crate::state::AppState;

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
    let _ = app_handle.emit("stream_chunk", crate::commands::StreamChunk {
        content: result.clone(),
        done: true,
        tool_calls: None,
        phase: Some("done".to_string()),
    });

    Ok(result)
}
