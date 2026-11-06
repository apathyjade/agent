//! 工作流文件扫描。
//!
//! 在固定的用户目录下查找 `*.yaml` / `*.yml`，解析成 [`WorkflowDef`]。
//! 单个文件的读取或解析失败只记日志并跳过，不影响其他工作流。

use std::path::{Path, PathBuf};

use crate::core::error::Result;
use crate::features::workflow::models::{TriggerDef, WorkflowDef, WorkflowInfo};

/// 相对用户主目录的工作流搜索路径。
const PIPELINE_DIRS: &[&str] = &[".config/agent/workflows"];

/// 扫描标准目录下的工作流 YAML 文件，返回路径与解析后的定义。
pub fn scan_workflow_files() -> Result<Vec<(PathBuf, WorkflowDef)>> {
    let mut results = Vec::new();

    let Some(home) = dirs::home_dir() else {
        return Ok(results);
    };

    for rel in PIPELINE_DIRS {
        let dir = home.join(rel);
        if !dir.is_dir() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !is_workflow_file(&path) {
                continue;
            }

            let content = match std::fs::read_to_string(&path) {
                Ok(content) => content,
                Err(e) => {
                    log::warn!(
                        "Failed to read workflow file '{}': {}",
                        path.display(),
                        e
                    );
                    continue;
                }
            };

            match serde_yaml::from_str::<WorkflowDef>(&content) {
                Ok(wf) => results.push((path, wf)),
                Err(e) => log::warn!(
                    "Failed to parse workflow YAML '{}': {}",
                    path.display(),
                    e
                ),
            }
        }
    }

    Ok(results)
}

/// 只接受 `.yaml` / `.yml` 扩展名。
fn is_workflow_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("yaml" | "yml")
    )
}

/// 扫描并转换成前端展示用的 [`WorkflowInfo`]。
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
