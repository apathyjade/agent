// ── Node.js Version Source ──
//
// Fetches available Node.js versions from the official nodejs.org dist index.
// Filters to stable versions matching the current platform.

use async_trait::async_trait;
use serde::Deserialize;

use crate::environment::http_client::get_http_client;
use crate::environment::registry::{RuntimeVersion, VersionSource};
use crate::environment::RuntimeType;
use crate::error::Result;

/// Raw response entry from https://nodejs.org/dist/index.json
#[derive(Debug, Deserialize)]
struct NodeDistEntry {
    version: String,        // "v20.18.3"
    lts: serde_json::Value, // false or a string like "Jod"
    date: String,           // "2025-01-15"
    files: Vec<String>,     // ["win-x64.zip", "linux-x64.tar.gz", ...]
}

pub struct NodeVersionSource;

impl NodeVersionSource {
    fn platform_file_pattern(&self) -> &'static str {
        #[cfg(target_os = "windows")]
        { "win-x64" }
        #[cfg(target_os = "linux")]
        { "linux-x64" }
        #[cfg(target_os = "macos")]
        { "darwin-arm64" }
        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        { "linux-x64" }
    }

    fn archive_ext(&self) -> &'static str {
        #[cfg(target_os = "windows")]
        { "zip" }
        #[cfg(not(target_os = "windows"))]
        { "tar.gz" }
    }
}

#[async_trait]
impl VersionSource for NodeVersionSource {
    fn runtime_type(&self) -> RuntimeType {
        RuntimeType::Node
    }

    async fn fetch_versions(&self) -> Result<Vec<RuntimeVersion>> {
        let platform = self.platform_file_pattern();
        let ext = self.archive_ext();

        let client = get_http_client();
        let response = client
            .get("https://nodejs.org/dist/index.json")
            .send()
            .await
            .map_err(|e| crate::error::AppError::Http(e))?;

        let entries: Vec<NodeDistEntry> = response
            .json()
            .await
            .map_err(|e| crate::error::AppError::Http(e))?;

        let mut versions: Vec<RuntimeVersion> = entries
            .into_iter()
            .filter(|e| {
                // Filter: only versions with files matching current platform
                let has_platform_file = e.files.iter().any(|f| f.contains(platform));
                // Filter out non-stable (lts must be a string to be considered stable)
                let is_stable = !matches!(&e.lts, serde_json::Value::Bool(false));
                has_platform_file && is_stable
            })
            .filter_map(|e| {
                let version = e.version.strip_prefix('v').unwrap_or(&e.version).to_string();

                // Extract numeric version parts for sorting later
                // lts is either a string (codename) or false (no LTS)
                let lts = e.lts.as_str().map(|s| s.to_string());

                let url = format!(
                    "https://nodejs.org/dist/v{version}/node-v{version}-{platform}.{ext}",
                    version = version,
                    platform = platform,
                    ext = ext
                );

                Some(RuntimeVersion {
                    runtime_type: RuntimeType::Node,
                    version: version.clone(),
                    display_name: if let Some(ref codename) = lts {
                        format!("Node.js v{} (LTS - {})", version, codename)
                    } else {
                        format!("Node.js v{}", version)
                    },
                    url,
                    lts,
                    is_stable: true,
                    release_date: Some(e.date),
                    file_size: None,
                })
            })
            .collect();

        // Sort: newest version first (semver comparison)
        versions.sort_by(|a, b| {
            let a_parts: Vec<u64> = a.version.split('.').filter_map(|s| s.parse().ok()).collect();
            let b_parts: Vec<u64> = b.version.split('.').filter_map(|s| s.parse().ok()).collect();
            for (av, bv) in a_parts.iter().zip(b_parts.iter()) {
                if av != bv {
                    return bv.cmp(av);
                }
            }
            b_parts.len().cmp(&a_parts.len())
        });

        Ok(versions)
    }
}
