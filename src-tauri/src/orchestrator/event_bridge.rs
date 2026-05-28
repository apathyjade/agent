use tauri::{Emitter, AppHandle};
use tokio::sync::mpsc;

use crate::orchestrator::agent::{OrchestrationEvent, OrchestrationPhase};

/// Spawn a task that forwards OrchestrationEvents from a channel to Tauri events.
///
/// Maps each OrchestrationEvent variant to a frontend-consumable Tauri event:
/// - PhaseChanged → `orchestrator_phase`
/// - Thinking { content } → `stream_chunk` (reuses existing streaming event)
/// - TaskStarted / TaskCompleted / TaskFailed → `orchestrator_task_*`
/// - CritiqueReceived → `orchestrator_critique`
/// - SynthesizedOutput → `orchestrator_done`
pub fn spawn_event_bridge(
    app_handle: AppHandle,
    mut rx: mpsc::Receiver<OrchestrationEvent>,
) {
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                OrchestrationEvent::PhaseChanged(phase) => {
                    let phase_str = match phase {
                        OrchestrationPhase::Idle => "idle",
                        OrchestrationPhase::Analyzing => "analyzing",
                        OrchestrationPhase::Planning => "planning",
                        OrchestrationPhase::Executing => "executing",
                        OrchestrationPhase::Reflecting => "reflecting",
                        OrchestrationPhase::Synthesizing => "synthesizing",
                        OrchestrationPhase::Done => "done",
                    };
                    let _ = app_handle.emit("orchestrator_phase", phase_str);
                }
                OrchestrationEvent::Thinking { content } => {
                    let _ = app_handle.emit("stream_chunk", crate::commands::StreamChunk {
                        content,
                        done: false,
                        tool_calls: None,
                        phase: Some("thinking".to_string()),
                    });
                }
                OrchestrationEvent::TaskStarted { task_id, label, worker } => {
                    let _ = app_handle.emit("orchestrator_task_start", serde_json::json!({
                        "task_id": task_id,
                        "label": label,
                        "worker": format!("{:?}", worker),
                    }));
                }
                OrchestrationEvent::TaskCompleted { task_id, label, result_summary, duration_ms } => {
                    let _ = app_handle.emit("orchestrator_task_complete", serde_json::json!({
                        "task_id": task_id,
                        "label": label,
                        "summary": result_summary,
                        "duration_ms": duration_ms,
                    }));
                }
                OrchestrationEvent::TaskFailed { task_id, label, error } => {
                    let _ = app_handle.emit("orchestrator_task_fail", serde_json::json!({
                        "task_id": task_id,
                        "label": label,
                        "error": error,
                    }));
                }
                OrchestrationEvent::CritiqueReceived { task_id, decision, issues } => {
                    let _ = app_handle.emit("orchestrator_critique", serde_json::json!({
                        "task_id": task_id,
                        "decision": format!("{:?}", decision),
                        "issues": issues,
                    }));
                }
                OrchestrationEvent::SynthesizedOutput(output) => {
                    let _ = app_handle.emit("orchestrator_done", &output);
                }
            }
        }
    });
}
