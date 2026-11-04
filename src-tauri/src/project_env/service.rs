// ── Project runtime binding service ──
//
// Scans project files for runtime requirements, persists bindings and applies
// them. This is the app-specific layer above the standalone toolchain engine.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::core::db::models::BoundProjectModel;
use crate::core::db::repository::Database;
use crate::core::error::{AppError, Result};
use crate::toolchain::RuntimeServices;

use super::detector::{
    BoundProject, ProjectDetector, ProjectRuntimeRequirement, SyncAction, SyncResult,
};

/// List bound projects with decoded requirements.
pub async fn list(db: &Arc<Mutex<Database>>) -> Result<Vec<BoundProject>> {
    let db = db.lock().await;
    let models = db.list_bound_projects()?;
    Ok(models.into_iter().map(bound_project_from_model).collect())
}

/// Scan a directory and persist it as a bound project.
pub async fn bind(db: &Arc<Mutex<Database>>, path: &str) -> Result<BoundProject> {
    let project_path = std::path::PathBuf::from(path);
    let name = project_path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    // Scan before locking so a slow filesystem cannot block other DB users.
    let scan = ProjectDetector::scan(&project_path).await?;

    let db = db.lock().await;
    let model = db.add_bound_project(path, &name)?;
    if scan.requirements.is_empty() {
        return Ok(bound_project_from_model(model));
    }

    let updated = BoundProjectModel {
        requirements: Some(serde_json::to_string(&scan.requirements)?),
        ..model
    };
    db.update_bound_project(&updated)?;
    Ok(BoundProject {
        id: updated.id,
        path: updated.path,
        name: updated.name,
        auto_sync: updated.auto_sync,
        last_scan: updated.last_scan,
        requirements: scan.requirements,
        created_at: updated.created_at,
        updated_at: updated.updated_at,
    })
}

pub async fn remove(db: &Arc<Mutex<Database>>, id: &str) -> Result<()> {
    let db = db.lock().await;
    db.remove_bound_project(id)
}

/// Apply a project's requirements.
///
/// For each requirement: resolve the spec, switch to the target version and,
/// when the version is not installed locally, install it first. The result
/// reports every requirement so callers can show partial success.
pub async fn sync(
    services: &RuntimeServices,
    db: &Arc<Mutex<Database>>,
    id: &str,
    active_managers: &HashMap<String, String>,
) -> Result<SyncResult> {
    let model = {
        let db = db.lock().await;
        db.get_bound_project(id)?
            .ok_or_else(|| AppError::NotFound(format!("项目 {} 未找到", id)))?
    };
    let requirements = decode_requirements(model.requirements.as_deref());

    let mut actions = Vec::new();
    let mut all_success = true;

    for requirement in requirements {
        let rt = requirement.runtime_type.clone();
        let active_manager = active_managers.get(&rt.to_string()).map(String::as_str);
        let current = services.detect(&rt).await.version.unwrap_or_default();

        let target = match services.resolve(&rt, &requirement.version_spec).await {
            Ok(target) => target,
            Err(_) => {
                all_success = false;
                actions.push(skipped(
                    &requirement,
                    current,
                    requirement.version_spec.clone(),
                ));
                continue;
            }
        };

        if current == target {
            actions.push(SyncAction {
                runtime_type: rt,
                action: "already_matched".into(),
                from_version: Some(current),
                to_version: target,
                success: true,
            });
            continue;
        }

        match services.switch(&rt, &target, active_manager).await {
            Ok(_) => actions.push(SyncAction {
                runtime_type: rt,
                action: "switch".into(),
                from_version: Some(current),
                to_version: target,
                success: true,
            }),
            Err(_) => {
                // The target version is not installed: install it, then retry.
                match services
                    .install(&rt, Some(target.clone()), active_manager, |_| {})
                    .await
                {
                    Ok(_) => match services.switch(&rt, &target, active_manager).await {
                        Ok(_) => actions.push(SyncAction {
                            runtime_type: rt,
                            action: "install".into(),
                            from_version: Some(current),
                            to_version: target,
                            success: true,
                        }),
                        Err(_) => {
                            all_success = false;
                            actions.push(skipped(&requirement, current, target));
                        }
                    },
                    Err(_) => {
                        all_success = false;
                        actions.push(skipped(&requirement, current, target));
                    }
                }
            }
        }
    }

    {
        let db = db.lock().await;
        let updated = BoundProjectModel {
            last_scan: Some(chrono::Utc::now().to_rfc3339()),
            ..model
        };
        db.update_bound_project(&updated)?;
    }

    Ok(SyncResult {
        project_id: id.to_string(),
        actions,
        success: all_success,
        error: if all_success {
            None
        } else {
            Some("部分运行时未能同步".into())
        },
    })
}

fn skipped(requirement: &ProjectRuntimeRequirement, current: String, target: String) -> SyncAction {
    SyncAction {
        runtime_type: requirement.runtime_type.clone(),
        action: "skipped".into(),
        from_version: Some(current),
        to_version: target,
        success: false,
    }
}

fn bound_project_from_model(model: BoundProjectModel) -> BoundProject {
    BoundProject {
        id: model.id,
        path: model.path,
        name: model.name,
        auto_sync: model.auto_sync,
        last_scan: model.last_scan,
        requirements: decode_requirements(model.requirements.as_deref()),
        created_at: model.created_at,
        updated_at: model.updated_at,
    }
}

pub fn decode_requirements(raw: Option<&str>) -> Vec<ProjectRuntimeRequirement> {
    raw.and_then(|value| serde_json::from_str(value).ok())
        .unwrap_or_default()
}
