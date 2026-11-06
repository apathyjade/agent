//! 计划执行控制 IPC 命令。
//!
//! 负责「执行一个已生成的计划」以及暂停 / 恢复 / 取消 / 查询状态。
//! 执行句柄存在 `AppState::active_executions` 里（session_id → handle），
//! 因此同一会话同一时刻只能有一个受控执行。
//!
//! 这些命令的错误类型是 `Result<_, String>`：前端只需要一句可展示的文案，
//! 不需要结构化的错误分支。

use tauri::{Emitter, State};
use tokio::sync::mpsc;

use crate::agent::orchestrator::plan_types::*;
use crate::agent::orchestrator::runtime::ExecutionRuntime;
use crate::app::state::AppState;

/// 在后台执行一份计划 JSON，立即返回。
///
/// `plan_json` 是前端确认过的 `ExecutionPlan` 序列化结果；解析失败直接
/// 返回 `Err`。执行进度通过 `plan_progress` 事件推送，执行句柄会注册到
/// `active_executions[session_id]`，供后续 pause/resume/cancel 使用。
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

    // 执行放到后台任务，命令本身立即返回。
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
                    PlanProgressEvent::new(plan_id.clone(), sid.clone(), "plan_failed", 0, 0)
                        .with_error(e.to_string()),
                );
            }
        }
    });

    // 把 runtime 的进度事件原样转发给前端。
    let app_handle_clone = state.app_handle.clone();
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let _ = app_handle_clone.emit("plan_progress", event);
        }
    });

    // 注册执行句柄。
    {
        let mut executions = state.active_executions.lock().await;
        executions.insert(session_id, handle);
    }

    Ok(())
}

/// 暂停该会话正在执行的计划。
///
/// 只置暂停标志，执行器会在下一个步骤边界停下；没有活跃执行时返回 `Err`。
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

/// 恢复被暂停的计划；没有活跃执行时返回 `Err`。
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

/// 取消计划并从 `active_executions` 中移除句柄。
///
/// 取消后该会话不再能 pause/resume，需要重新发起执行。
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

/// 读取会话持久化的执行状态 JSON（`{"type":"idle"}` 等）；会话不存在时返回 `Ok(None)`。
#[tauri::command]
pub async fn get_execution_status(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Option<String>, String> {
    let db = state.db.lock().await;
    let sess = db.get_session(&session_id).map_err(|e| e.to_string())?;
    Ok(sess.map(|s| s.execution_status))
}

/// 读取计划详情：`{ "plan": ExecutionPlan, "step_records": [...] }`。
///
/// 计划不存在时返回 `Ok(None)`；`step_records` 是该计划每一步的落库记录
/// （状态、结果），与 `plan` 的静态定义配合展示。
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
