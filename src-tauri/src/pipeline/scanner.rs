use std::path::PathBuf;

use crate::error::Result;
use crate::pipeline::models::{TriggerDef, WorkflowDef, WorkflowInfo};

const PIPELINE_DIRS: &[&str] = &[".config/agent/workflows"];

/// Scan standard directories for workflow YAML files.
/// Returns a list of discovered workflow definitions with their metadata.
pub fn scan_workflow_files() -> Result<Vec<(PathBuf, WorkflowDef)>> {
    let mut results = Vec::new();

    if let Some(home) = dirs::home_dir() {
        for rel in PIPELINE_DIRS {
            let dir = home.join(rel);
            if !dir.exists() || !dir.is_dir() {
                continue;
            }
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("yaml")
                        || path.extension().and_then(|e| e.to_str()) == Some("yml")
                    {
                        match std::fs::read_to_string(&path) {
                            Ok(content) => {
                                match serde_yaml::from_str::<WorkflowDef>(&content) {
                                    Ok(wf) => {
                                        results.push((path, wf));
                                    }
                                    Err(e) => {
                                        log::warn!(
                                            "Failed to parse workflow YAML '{}': {}",
                                            path.display(),
                                            e
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                log::warn!(
                                    "Failed to read workflow file '{}': {}",
                                    path.display(),
                                    e
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(results)
}

/// Scan and return WorkflowInfo for frontend display.
pub fn list_workflows() -> Result<Vec<WorkflowInfo>> {
    let workflows = scan_workflow_files()?;
    let infos = workflows
        .into_iter()
        .map(|(path, def)| WorkflowInfo {
            name: def.name.clone(),
            description: def.description.clone(),
            step_count: def.steps.len(),
            file_path: path.to_string_lossy().to_string(),
            trigger: match &def.trigger {
                TriggerDef::Manual => "manual".to_string(),
                TriggerDef::Cron { schedule } => format!("cron: {}", schedule),
                TriggerDef::FileWatch { path, pattern } => {
                    format!("file_watch: {} ({})", path, pattern)
                }
            },
            next_run_at: None,
            last_run_status: None,
            last_run_at: None,
        })
        .collect();
    Ok(infos)
}
