// ── Bun Version Source ──
//
// Fetches available Bun versions from the GitHub Releases API.
// Bun does not have LTS — all stable releases are "Active".

use async_trait::async_trait;
use serde::Deserialize;

use crate::environment::http_client::get_http_client;
use crate::environment::registry::{RuntimeVersion, VersionSource};
use crate::environment::RuntimeType;
use crate::error::Result;

/// A GitHub release entry.
#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    prerelease: bool,
    #[allow(dead_code)]
    published_at: Option<String>,
}

pub struct BunVersionSource;

impl BunVersionSource {
    /// Determine the platform-specific asset filename for Bun downloads.
    fn platform_asset_name() -> &'static str {
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        { "bun-windows-x64.zip" }
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        { "bun-linux-x64.zip" }
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        { "bun-darwin-arm64.zip" }
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        { "bun-darwin-x64.zip" }
        #[cfg(not(any(
            all(target_os = "windows", target_arch = "x86_64"),
            all(target_os = "linux", target_arch = "x86_64"),
            all(target_os = "macos", any(target_arch = "aarch64", target_arch = "x86_64")),
        )))]
        { "bun-windows-x64.zip" } // fallback
    }

    fn archive_ext() -> &'static str {
        if Self::platform_asset_name().ends_with(".zip") {
            "zip"
        } else {
            "tar.gz"
        }
    }
}

#[async_trait]
impl VersionSource for BunVersionSource {
    fn runtime_type(&self) -> RuntimeType {
        RuntimeType::Bun
    }

    async fn fetch_versions(&self) -> Result<Vec<RuntimeVersion>> {
        let asset_name = Self::platform_asset_name();
        let ext = Self::archive_ext();

        let client = get_http_client();
        let response = client
            .get("https://api.github.com/repos/oven-sh/bun/releases?per_page=20")
            .header("User-Agent", "agent-runtime-manager/1.0")
            .send()
            .await
            .map_err(|e| crate::error::AppError::Http(e))?;

        let releases: Vec<GitHubRelease> = response
            .json()
            .await
            .map_err(|e| crate::error::AppError::Http(e))?;

        let mut versions: Vec<RuntimeVersion> = releases
            .into_iter()
            .filter(|r| !r.prerelease)
            .filter_map(|r| {
                let tag = r.tag_name.clone();
                // Tags are like "bun-v1.2.0" — strip "bun-v" prefix
                let version = tag
                    .strip_prefix("bun-v")
                    .or_else(|| tag.strip_prefix('v'))
                    .unwrap_or(&tag)
                    .to_string();

                let url = format!(
                    "https://github.com/oven-sh/bun/releases/download/{}/{}",
                    r.tag_name, asset_name
                );

                Some(RuntimeVersion {
                    runtime_type: RuntimeType::Bun,
                    version,
                    display_name: format!("Bun {}", tag),
                    url,
                    lts: None,
                    is_stable: true,
                    release_date: r.published_at,
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

        // Fallback if no versions from API
        if versions.is_empty() {
            return Ok(Self::fallback_versions(ext, asset_name));
        }

        Ok(versions)
    }
}

impl BunVersionSource {
    fn fallback_versions(_ext: &'static str, asset_name: &str) -> Vec<RuntimeVersion> {
        ["1.2.5", "1.1.42", "1.0.36", "0.8.1"]
            .iter()
            .map(|v| {
                RuntimeVersion {
                    runtime_type: RuntimeType::Bun,
                    version: v.to_string(),
                    display_name: format!("Bun v{}", v),
                    url: format!(
                        "https://github.com/oven-sh/bun/releases/download/bun-v{}/{}",
                        v, asset_name
                    ),
                    lts: None,
                    is_stable: true,
                    release_date: None,
                    file_size: None,
                }
            })
            .collect()
    }
}
