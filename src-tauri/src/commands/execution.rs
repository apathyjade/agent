use tauri::{Emitter, State};
use tokio::sync::mpsc;

use crate::execution::runtime::ExecutionRuntime;
use crate::execution::types::*;
use crate::state::AppState;

#[tauri::command]
pub async fn execute_plan(
    state: State<'_, AppState>,
    session_id: String,
    plan_json: String,
) -> Result<(), String> {
    let plan: ExecutionPlan =
        serde_json::from_str(&plan_json).map_err(|e| format!("Invalid plan JSON: {}", e))?;

    let handle = ExecutionHandle::new(session_id.clone(), plan.id.clone());

    let runtime = ExecutionRuntime::new(state.providers.clone(), state.tools.clone(), state.db.clone());

    let (event_tx, mut event_rx) = mpsc::channel::<PlanProgressEvent>(32);

    // Spawn execution in background
    let plan_id = plan.id.clone();
    let sid = session_id.clone();
    let cf = handle.cancel_flag.clone();
    let pf = handle.pause_flag.clone();
    let app_handle = state.app_handle.clone();

    tokio::spawn(async move {
        match runtime.execute(plan, event_tx, cf, pf).await {
            Ok(()) => {
                log::info!("Plan {} completed successfully", plan_id);
            }
            Err(e) => {
                log::error!("Plan {} failed: {}", plan_id, e);
                let _ = app_handle.emit(
                    "plan_progress",
                    PlanProgressEvent {
                        plan_id: plan_id.clone(),
                        session_id: sid.clone(),
                        event_type: "plan_failed".to_string(),
                        step_index: None,
                        step_label: None,
                        result_summary: None,
                        error: Some(e.to_string()),
                        total_steps: 0,
                        completed_steps: 0,
                    },
                );
            }
        }
    });

    // Forward events from runtime to frontend
    let app_handle_clone = state.app_handle.clone();
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let _ = app_handle_clone.emit("plan_progress", event);
        }
    });

    // Store execution handle
    {
        let mut executions = state.active_executions.lock().await;
        executions.insert(session_id, handle);
    }

    Ok(())
}

#[tauri::command]
pub async fn pause_execution(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    let executions = state.active_executions.lock().await;
    if let Some(exec) = executions.get(&session_id) {
        exec.pause();
        Ok(())
    } else {
        Err("No active execution for this session".to_string())
    }
}

#[tauri::command]
pub async fn resume_execution(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    let executions = state.active_executions.lock().await;
    if let Some(exec) = executions.get(&session_id) {
        exec.resume();
        Ok(())
    } else {
        Err("No active execution for this session".to_string())
    }
}

#[tauri::command]
pub async fn cancel_execution(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    let mut executions = state.active_executions.lock().await;
    if let Some(exec) = executions.remove(&session_id) {
        exec.cancel();
        Ok(())
    } else {
        Err("No active execution for this session".to_string())
    }
}

#[tauri::command]
pub async fn get_execution_status(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Option<String>, String> {
    let db = state.db.lock().await;
    let sess = db.get_session(&session_id).map_err(|e| e.to_string())?;
    Ok(sess.map(|s| s.execution_status))
}

#[tauri::command]
pub async fn get_plan_detail(
    state: State<'_, AppState>,
    plan_id: String,
) -> Result<Option<serde_json::Value>, String> {
    let db = state.db.lock().await;

    let plan_record = db
        .get_execution_plan(&plan_id)
        .map_err(|e| e.to_string())?;

    let plan_record = match plan_record {
        Some(p) => p,
        None => return Ok(None),
    };

    let plan: ExecutionPlan =
        serde_json::from_str(&plan_record.plan_json).map_err(|e| format!("Failed to parse plan: {}", e))?;

    let steps = db.get_plan_steps(&plan_id).map_err(|e| e.to_string())?;

    Ok(Some(serde_json::json!({
        "plan": plan,
        "step_records": steps,
    })))
}
