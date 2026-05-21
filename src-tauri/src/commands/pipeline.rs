use tauri::State;

use crate::error::Result;
use crate::pipeline::models::{StepProgress, WorkflowInfo, WorkflowRunRecord};
use crate::state::AppState;

#[tauri::command]
pub async fn list_workflows(_state: State<'_, AppState>) -> Result<Vec<WorkflowInfo>> {
    crate::pipeline::scanner::list_workflows()
}

#[tauri::command]
pub async fn run_workflow(
    state: State<'_, AppState>,
    name: String,
) -> Result<String> {
    // Find the workflow by name in scanned files
    let workflows = crate::pipeline::scanner::scan_workflow_files()?;
    let (path, workflow) = workflows
        .into_iter()
        .find(|(_, wf)| wf.name == name)
        .ok_or_else(|| {
            crate::error::AppError::NotFound(format!("Workflow '{}' not found", name))
        })?;

    // Create event channel for step progress
    let (event_tx, mut event_rx) =
        tokio::sync::mpsc::channel::<StepProgress>(100);

    // Spawn a task to log progress events (future: emit to frontend)
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

    // Create engine with providers and event channel
    let engine = crate::pipeline::PipelineEngine::new(
        state.tools.clone(),
        state.db.clone(),
        state.providers.clone(),
        event_tx,
    );

    let results = engine.run(&workflow).await?;

    // Format results summary
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

#[tauri::command]
pub async fn list_workflow_runs(state: State<'_, AppState>) -> Result<Vec<WorkflowRunRecord>> {
    let db = state.db.lock().await;
    db.list_workflow_runs(50)
}

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

#[tauri::command]
pub async fn get_workflow_run_detail(
    state: State<'_, AppState>,
    id: String,
) -> Result<WorkflowRunRecord> {
    let db = state.db.lock().await;
    db.get_workflow_run(&id)?.ok_or_else(|| {
        crate::error::AppError::NotFound(format!("Workflow run '{}' not found", id))
    })
}

// ── Workflow Variables ──

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

#[tauri::command]
pub async fn list_workflow_vars(
    state: State<'_, AppState>,
) -> Result<std::collections::HashMap<String, String>> {
    let config = state.config.lock().await;
    Ok(config.workflow_vars.clone())
}

// ── Workflow Secrets ──

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

#[tauri::command]
pub async fn list_workflow_secrets(
    state: State<'_, AppState>,
) -> Result<Vec<String>> {
    // Only return keys, not values (security)
    let config = state.config.lock().await;
    Ok(config.workflow_secrets.keys().cloned().collect())
}

// ── AI Workflow Generation ──

#[tauri::command]
pub async fn generate_workflow(
    state: State<'_, AppState>,
    description: String,
) -> Result<String> {
    // Get the default provider to call LLM
    let response = {
        let providers = state.providers.lock().await;
        let mid = providers.default_model_id();
        if mid.is_empty() {
            return Err(crate::error::AppError::Provider(
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

        let request = crate::api::types::ChatRequest {
            messages: vec![
                crate::api::types::Message {
                    id: None,
                    role: crate::api::types::MessageRole::System,
                    content: system_prompt.to_string(),
                    tool_calls: None,
                    tool_call_id: None,
                },
                crate::api::types::Message {
                    id: None,
                    role: crate::api::types::MessageRole::User,
                    content: description,
                    tool_calls: None,
                    tool_call_id: None,
                },
            ],
            model: mid.to_string(),
            tools: None,
            stream: Some(false),
            max_tokens: Some(2000),
            temperature: Some(0.3),
        };

        provider.chat(request).await
    }?;

    let yaml = response
        .choices
        .first()
        .map(|c| c.message.content.clone())
        .unwrap_or_default();

    // Save to workflows directory
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
        Err(crate::error::AppError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Home directory not found",
        )))
    }
}
