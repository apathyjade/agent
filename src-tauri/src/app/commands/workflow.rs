//! 工作流 IPC 命令。
//!
//! 工作流是 YAML 描述的多步骤管道，可在本机手动触发。变量与密钥存在
//! `AppConfig` 里；密钥的读取接口只回传 key，不回传值。

use tauri::State;

use crate::core::error::Result;
use crate::features::workflow::models::{StepProgress, WorkflowInfo, WorkflowRunRecord};
use crate::app::state::AppState;

/// 列出可用的工作流定义（扫描工作流目录）。
#[tauri::command]
pub async fn list_workflows(_state: State<'_, AppState>) -> Result<Vec<WorkflowInfo>> {
    crate::features::workflow::scanner::list_workflows()
}

/// 按名称执行一个工作流，返回文本形式的执行摘要。
///
/// 名称在扫描结果里找不到时返回 `NotFound`。当前进度只写日志，尚未 emit
/// 到前端。
#[tauri::command]
pub async fn run_workflow(
    state: State<'_, AppState>,
    name: String,
) -> Result<String> {
    // 在扫描结果里按名称找到工作流定义。
    let workflows = crate::features::workflow::scanner::scan_workflow_files()?;
    let (path, workflow) = workflows
        .into_iter()
        .find(|(_, wf)| wf.name == name)
        .ok_or_else(|| {
            crate::core::error::AppError::NotFound(format!("Workflow '{}' not found", name))
        })?;

    // 建立步骤进度事件通道。
    let (event_tx, mut event_rx) =
        tokio::sync::mpsc::channel::<StepProgress>(100);

    // 目前只把进度写进日志（后续可改为 emit 给前端）。
    tokio::spawn(async move {
        while let Some(progress) = event_rx.recv().await {
            log::info!(
                "Step '{}' status: {} (duration: {:?})",
                progress.step_id,
                progress.status,
                progress.duration_ms,
            );
        }
    });

    // 构造带 provider 与事件通道的执行引擎。
    let engine = crate::features::workflow::PipelineEngine::new(
        state.tools.clone(),
        state.db.clone(),
        state.providers.clone(),
        event_tx,
    );

    let results = engine.run(&workflow).await?;

    // 汇总各步骤结果。
    let summary: Vec<String> = results
        .iter()
        .map(|(k, v)| format!("{}: {}", k, serde_json::to_string(v).unwrap_or_default()))
        .collect();

    let log_path = path.to_string_lossy().to_string();
    Ok(format!(
        "Workflow '{}' completed.\nFile: {}\nResults:\n{}",
        name,
        log_path,
        summary.join("\n")
    ))
}

/// 列出最近的 50 次工作流执行记录。
#[tauri::command]
pub async fn list_workflow_runs(state: State<'_, AppState>) -> Result<Vec<WorkflowRunRecord>> {
    let db = state.db.lock().await;
    db.list_workflow_runs(50)
}

/// 请求暂停某工作流的调度。
///
/// 调度器尚未实现，当前只记日志并返回 `Ok`（幂等）。
#[tauri::command]
pub async fn pause_workflow_schedule(
    _state: State<'_, AppState>,
    name: String,
) -> Result<()> {
    log::info!(
        "Workflow schedule pause requested for '{}' (not yet implemented)",
        name
    );
    Ok(())
}

/// 请求恢复某工作流的调度。
///
/// 调度器尚未实现，当前只记日志并返回 `Ok`（幂等）。
#[tauri::command]
pub async fn resume_workflow_schedule(
    _state: State<'_, AppState>,
    name: String,
) -> Result<()> {
    log::info!(
        "Workflow schedule resume requested for '{}' (not yet implemented)",
        name
    );
    Ok(())
}

/// 读取单次执行记录；不存在时返回 `NotFound`。
#[tauri::command]
pub async fn get_workflow_run_detail(
    state: State<'_, AppState>,
    id: String,
) -> Result<WorkflowRunRecord> {
    let db = state.db.lock().await;
    db.get_workflow_run(&id)?.ok_or_else(|| {
        crate::core::error::AppError::NotFound(format!("Workflow run '{}' not found", id))
    })
}

// ── 工作流变量 ──

/// 设置一个工作流变量（明文，落 `config.json`）。
#[tauri::command]
pub async fn set_workflow_var(
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> Result<()> {
    let mut config = state.config.lock().await;
    config.workflow_vars.insert(key, value);
    config.save()?;
    Ok(())
}

/// 删除一个工作流变量（不存在时也是成功）。
#[tauri::command]
pub async fn delete_workflow_var(
    state: State<'_, AppState>,
    key: String,
) -> Result<()> {
    let mut config = state.config.lock().await;
    config.workflow_vars.remove(&key);
    config.save()?;
    Ok(())
}

/// 列出全部工作流变量（含值）。
#[tauri::command]
pub async fn list_workflow_vars(
    state: State<'_, AppState>,
) -> Result<std::collections::HashMap<String, String>> {
    let config = state.config.lock().await;
    Ok(config.workflow_vars.clone())
}

// ── 工作流密钥 ──

/// 设置一个工作流密钥。
#[tauri::command]
pub async fn set_workflow_secret(
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> Result<()> {
    let mut config = state.config.lock().await;
    config.workflow_secrets.insert(key, value);
    config.save()?;
    Ok(())
}

/// 删除一个工作流密钥。
#[tauri::command]
pub async fn delete_workflow_secret(
    state: State<'_, AppState>,
    key: String,
) -> Result<()> {
    let mut config = state.config.lock().await;
    config.workflow_secrets.remove(&key);
    config.save()?;
    Ok(())
}

/// 列出全部工作流密钥的 **key**（出于安全考虑永不回传值）。
#[tauri::command]
pub async fn list_workflow_secrets(
    state: State<'_, AppState>,
) -> Result<Vec<String>> {
    // 只返回 key，不返回值（安全考虑）。
    let config = state.config.lock().await;
    Ok(config.workflow_secrets.keys().cloned().collect())
}

// ── AI 生成工作流 ──

/// 用默认模型把自然语言描述生成成工作流 YAML，保存到
/// `~/.config/agent/workflows/ai-generated-<时间戳>.yaml`。
///
/// 返回「保存路径 + YAML 内容」。没有默认模型时返回 `Provider` 错误；
/// 取不到 home 目录时返回 `Io` 错误。
#[tauri::command]
pub async fn generate_workflow(
    state: State<'_, AppState>,
    description: String,
) -> Result<String> {
    // 取默认 provider 来调用 LLM。
    let yaml = {
        let providers = state.providers.lock().await;
        let mid = providers.default_model_id();
        if mid.is_empty() {
            return Err(crate::core::error::AppError::Provider(
                "No default model configured".to_string(),
            ));
        }
        let provider = providers.get(mid)?;

        let system_prompt = "You are a workflow generator. Given a natural language description, generate a valid workflow YAML following this schema:
```yaml
name: string
description: string
trigger:
  type: manual|cron
  schedule: string (cron expression, only if type=cron)
steps:
  - id: string
    type: tool_call|llm_call|condition
    # For tool_call:
    tool: string
    params: object
    # For llm_call:
    prompt: string
    # For condition:
    condition: string
    on_false: end
```

Only output the YAML, nothing else.";

        provider.prompt(system_prompt, &description).await
    }?;

    // 写入工作流目录。
    if let Some(home) = dirs::home_dir() {
        let dir = home.join(".config").join("agent").join("workflows");
        std::fs::create_dir_all(&dir)?;
        let file_name = format!(
            "ai-generated-{}.yaml",
            chrono::Utc::now().format("%Y%m%d-%H%M%S")
        );
        let path = dir.join(&file_name);
        std::fs::write(&path, &yaml)?;
        Ok(format!(
            "Workflow saved to: {}\n\n```yaml\n{}\n```",
            path.display(),
            yaml
        ))
    } else {
        Err(crate::core::error::AppError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Home directory not found",
        )))
    }
}
