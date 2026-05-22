// ── Upgrade Check: Detect outdated runtime versions and suggest upgrades ──

use crate::environment::lifecycle;
use crate::environment::registry::RuntimeRegistry;
use crate::environment::{RuntimeManager, RuntimeType};
use crate::error::Result;

/// Result of a version update check for a single runtime.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VersionUpdate {
    pub runtime_type: RuntimeType,
    pub current_version: String,
    pub latest_version: String,
    pub reason: String,
}

/// Check for version updates across all runtimes.
/// For each runtime, detects the current version and compares against the
/// latest available from the registry. If the current version is EOL or in
/// maintenance mode and a newer version exists, a VersionUpdate is emitted.
pub async fn check_updates(
    runtime_manager: &RuntimeManager,
    registry: &RuntimeRegistry,
) -> Result<Vec<VersionUpdate>> {
    let mut updates = Vec::new();

    for rt in RuntimeType::all() {
        let info = runtime_manager.detect(rt).await;

        if let Some(ref current) = info.version {
            // Get lifecycle info for current version
            let lc = lifecycle::for_runtime(rt, current, None);

            // Only suggest upgrade if current version is EOL or Maintenance
            if !matches!(
                lc,
                lifecycle::VersionLifecycle::Eol { .. }
                    | lifecycle::VersionLifecycle::Maintenance { .. }
            ) {
                continue;
            }

            // Fetch available versions
            let versions = registry.get_versions(rt).await;
            let versions = match versions {
                Ok(v) => v,
                Err(_) => continue, // Skip if we can't fetch
            };

            // Find newest stable version
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

/// Compare two version strings, handling non-standard formats gracefully.
fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    match (semver::Version::parse(a), semver::Version::parse(b)) {
        (Ok(va), Ok(vb)) => va.cmp(&vb),
        _ => a.cmp(b),
    }
}
