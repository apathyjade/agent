//! 项目运行时绑定服务。
//!
//! 扫描项目文件中的运行时需求，持久化绑定关系并应用它们。这是独立工具链
//! 引擎之上的项目级封装。

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::core::db::models::BoundProjectModel;
use crate::core::db::repository::Database;
use crate::core::error::{AppError, Result};
use crate::runtime::toolchain::RuntimeServices;

use super::detector::{
    BoundProject, ProjectDetector, ProjectRuntimeRequirement, SyncAction, SyncResult,
};

/// 列出已绑定项目，并解码其运行时需求。
pub async fn list(db: &Arc<Mutex<Database>>) -> Result<Vec<BoundProject>> {
    let db = db.lock().await;
    let models = db.list_bound_projects()?;
    Ok(models.into_iter().map(bound_project_from_model).collect())
}

/// 扫描目录并将其持久化为绑定项目。
pub async fn bind(db: &Arc<Mutex<Database>>, path: &str) -> Result<BoundProject> {
    let project_path = std::path::PathBuf::from(path);
    let name = project_path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    // 先扫描再取锁，避免慢速文件系统阻塞其它数据库使用者。
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

/// 删除一个绑定项目。
pub async fn remove(db: &Arc<Mutex<Database>>, id: &str) -> Result<()> {
    let db = db.lock().await;
    db.remove_bound_project(id)
}

/// 应用项目的运行时需求。
///
/// 对每条需求：解析版本规格、切换到目标版本；若本地未安装则先安装。结果会
/// 逐条上报，便于调用方展示部分成功。该过程可能访问网络（安装/解析）并阻塞
/// 直到所有需求处理完毕。
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
                // 目标版本未安装：先安装再重试
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

/// 构造一条“跳过”的同步动作。
fn skipped(requirement: &ProjectRuntimeRequirement, current: String, target: String) -> SyncAction {
    SyncAction {
        runtime_type: requirement.runtime_type.clone(),
        action: "skipped".into(),
        from_version: Some(current),
        to_version: target,
        success: false,
    }
}

/// 把数据库模型转换为 `BoundProject`。
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

/// 解码数据库中存储的需求 JSON；内容缺失或非法时返回空列表。
pub fn decode_requirements(raw: Option<&str>) -> Vec<ProjectRuntimeRequirement> {
    raw.and_then(|value| serde_json::from_str(value).ok())
        .unwrap_or_default()
}
