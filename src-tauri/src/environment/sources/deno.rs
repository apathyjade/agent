// ── Deno Version Source ──
//
// Fetches available Deno versions from the GitHub Releases API.
// Filters for stable releases and matches the current platform.

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
    html_url: Option<String>,
    #[allow(dead_code)]
    published_at: Option<String>,
    #[allow(dead_code)]
    assets: Option<Vec<GitHubAsset>>,
}

/// An asset within a GitHub release.
#[derive(Debug, Deserialize)]
struct GitHubAsset {
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    browser_download_url: String,
    #[allow(dead_code)]
    size: Option<u64>,
}

pub struct DenoVersionSource;

impl DenoVersionSource {
    /// Determine the platform-specific asset suffix for Deno downloads.
    fn platform_asset_suffix() -> &'static str {
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        { "x86_64-pc-windows-msvc.zip" }
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        { "x86_64-unknown-linux-gnu.tar.gz" }
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        { "aarch64-apple-darwin.tar.gz" }
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        { "x86_64-apple-darwin.tar.gz" }
        #[cfg(not(any(
            all(target_os = "windows", target_arch = "x86_64"),
            all(target_os = "linux", target_arch = "x86_64"),
            all(target_os = "macos", any(target_arch = "aarch64", target_arch = "x86_64")),
        )))]
        { "x86_64-pc-windows-msvc.zip" } // fallback
    }

    fn archive_ext() -> &'static str {
        if Self::platform_asset_suffix().ends_with(".zip") {
            "zip"
        } else {
            "tar.gz"
        }
    }
}

#[async_trait]
impl VersionSource for DenoVersionSource {
    fn runtime_type(&self) -> RuntimeType {
        RuntimeType::Deno
    }

    async fn fetch_versions(&self) -> Result<Vec<RuntimeVersion>> {
        let suffix = Self::platform_asset_suffix();
        let ext = Self::archive_ext();

        // GitHub API requires a User-Agent header
        let client = get_http_client();
        let response = client
            .get("https://api.github.com/repos/denoland/deno/releases?per_page=20")
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
                let tag = r.tag_name.strip_prefix('v').unwrap_or(&r.tag_name).to_string();

                // URL is constructed from the standard Deno release pattern
                let url = format!(
                    "https://github.com/denoland/deno/releases/download/v{}/deno-{}",
                    tag, suffix
                );

                Some(RuntimeVersion {
                    runtime_type: RuntimeType::Deno,
                    version: tag.clone(),
                    display_name: format!("Deno {}", tag),
                    url,
                    lts: None,
                    is_stable: true,
                    release_date: None,
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

        // If we got no versions from the API, use fallback
        if versions.is_empty() {
            return Ok(Self::fallback_versions(ext, suffix));
        }

        Ok(versions)
    }
}

impl DenoVersionSource {
    fn fallback_versions(_ext: &'static str, suffix: &str) -> Vec<RuntimeVersion> {
        ["2.2.0", "2.1.10", "2.0.6", "1.46.3"]
            .iter()
            .map(|v| {
                RuntimeVersion {
                    runtime_type: RuntimeType::Deno,
                    version: v.to_string(),
                    display_name: format!("Deno {}", v),
                    url: format!(
                        "https://github.com/denoland/deno/releases/download/v{}/deno-{}",
                        v, suffix
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
