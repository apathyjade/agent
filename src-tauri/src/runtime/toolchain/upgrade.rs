//! 升级检查：发现过期的运行时版本并给出升级建议。

use crate::core::error::Result;
use crate::runtime::toolchain::lifecycle;
use crate::runtime::toolchain::registry::RuntimeRegistry;
use crate::runtime::toolchain::version::compare_versions;
use crate::runtime::toolchain::{RuntimeManager, RuntimeType};

/// 单个运行时版本更新检查的结果。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VersionUpdate {
    pub runtime_type: RuntimeType,
    pub current_version: String,
    pub latest_version: String,
    pub reason: String,
}

/// 检查所有运行时的版本更新。
///
/// 逐个运行时检测当前版本，并与注册表中的最新可用版本比较。仅当当前版本
/// 已 EOL 或处于维护期、且存在更新版本时才产生一条 `VersionUpdate`。
/// 该函数会访问网络（通过注册表），但注册表带有 TTL 缓存。
pub async fn check_updates(
    runtime_manager: &RuntimeManager,
    registry: &RuntimeRegistry,
) -> Result<Vec<VersionUpdate>> {
    let mut updates = Vec::new();

    for rt in RuntimeType::all() {
        let info = runtime_manager.detect(rt).await;

        if let Some(ref current) = info.version {
            // 获取当前版本的生命周期信息
            let lc = lifecycle::for_runtime(rt, current, None);

            // 仅当当前版本已 EOL 或处于维护期时才建议升级
            if !matches!(
                lc,
                lifecycle::VersionLifecycle::Eol { .. }
                    | lifecycle::VersionLifecycle::Maintenance { .. }
            ) {
                continue;
            }

            // 拉取可用版本
            let versions = registry.get_versions(rt).await;
            let versions = match versions {
                Ok(v) => v,
                Err(_) => continue, // 拉取失败则跳过该运行时
            };

            // 找到最新的稳定版本
            let newest = versions
                .iter()
                .filter(|v| v.is_stable)
                .max_by(|a, b| compare_versions(&a.version, &b.version));

            if let Some(nv) = newest {
                if nv.version != *current {
                    updates.push(VersionUpdate {
                        runtime_type: rt.clone(),
                        current_version: current.clone(),
                        latest_version: nv.version.clone(),
                        reason: format!(
                            "{} {}（当前: {}，最新: {}）",
                            rt.display_name(),
                            lc.label(),
                            current,
                            nv.version
                        ),
                    });
                }
            }
        }
    }

    Ok(updates)
}
