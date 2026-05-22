// ── Python Version Source ──
//
// Fetches available Python versions from the python-build-standalone GitHub releases.
// Falls back to a curated list of known versions on API failure.

use async_trait::async_trait;
use serde::Deserialize;

use crate::environment::http_client::get_http_client;
use crate::environment::registry::{RuntimeVersion, VersionSource};
use crate::environment::RuntimeType;
use crate::error::Result;

/// GitHub release API response entry.
#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    published_at: String,
}

/// Fallback known versions when GitHub API is unreachable.
const FALLBACK_VERSIONS: &[&str] = &["3.13.3", "3.12.10", "3.11.12", "3.10.20"];

pub struct PythonVersionSource;

impl PythonVersionSource {
    fn platform_label(&self) -> &'static str {
        #[cfg(target_os = "windows")]
        { "x86_64-pc-windows-msvc" }
        #[cfg(target_os = "linux")]
        { "x86_64-unknown-linux-gnu" }
        #[cfg(target_os = "macos")]
        { "aarch64-apple-darwin" }
        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        { "x86_64-unknown-linux-gnu" }
    }

    fn archive_ext(&self) -> &'static str {
        "tar.gz"
    }

    /// Build a download URL for a given Python version using python-build-standalone.
    fn build_download_url(&self, version: &str, tag: &str) -> String {
        let platform = self.platform_label();
        let ext = self.archive_ext();
        let archive_name = format!("cpython-{}+{}-{}-install_only", version, tag, platform);
        format!(
            "https://github.com/astral-sh/python-build-standalone/releases/download/{}/{}",
            tag, archive_name
        )
        + "." + ext
    }

    /// Parse a version from a release tag like "20250115" -> no direct version mapping.
    /// We extract version info from the release name/body.
    fn parse_versions_from_release(&self, release: &GitHubRelease) -> Vec<(String, String)> {
        let tag = &release.tag_name;
        // python-build-standalone tags don't encode Python versions directly.
        // For a proper implementation, we'd parse the release body or use a manifest.
        // For now, return the fallback list with this release's tag.
        FALLBACK_VERSIONS
            .iter()
            .map(|v| (v.to_string(), tag.clone()))
            .collect()
    }

    /// Create RuntimeVersion entries from version + tag pairs.
    fn build_versions(&self, version_tag_pairs: Vec<(String, String)>) -> Vec<RuntimeVersion> {
        version_tag_pairs
            .into_iter()
            .map(|(version, tag)| {
                let url = self.build_download_url(&version, &tag);
                RuntimeVersion {
                    runtime_type: RuntimeType::Python,
                    version: version.clone(),
                    display_name: format!("Python {}", version),
                    url,
                    lts: None,
                    is_stable: true,
                    release_date: None,
                    file_size: None,
                }
            })
            .collect()
    }
}

#[async_trait]
impl VersionSource for PythonVersionSource {
    fn runtime_type(&self) -> RuntimeType {
        RuntimeType::Python
    }

    async fn fetch_versions(&self) -> Result<Vec<RuntimeVersion>> {
        // Try GitHub API first
        let client = get_http_client();
        match client
            .get("https://api.github.com/repos/astral-sh/python-build-standalone/releases?per_page=5")
            .header("User-Agent", "agent-runtime-manager/1.0")
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {
                match response.json::<Vec<GitHubRelease>>().await {
                    Ok(releases) => {
                        if let Some(latest) = releases.first() {
                            let pairs = self.parse_versions_from_release(latest);
                            return Ok(self.build_versions(pairs));
                        }
                        // Fall through to fallback
                    }
                    Err(_) => { /* Fall through to fallback */ }
                }
            }
            Ok(response) => {
                // Rate limited or other error - use fallback
                log::warn!(
                    "GitHub API returned {} for Python versions, using fallback",
                    response.status()
                );
            }
            Err(e) => {
                log::warn!("Failed to fetch Python versions from GitHub: {}, using fallback", e);
            }
        }

        // Fallback: use known curated list with a default tag
        let default_tag = "20260510";
        let pairs: Vec<(String, String)> = FALLBACK_VERSIONS
            .iter()
            .map(|v| (v.to_string(), default_tag.to_string()))
            .collect();
        Ok(self.build_versions(pairs))
    }
}
