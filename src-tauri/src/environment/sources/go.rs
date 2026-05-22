// ── Go Version Source ──
//
// Fetches available Go versions from the official go.dev DL API.
// Filters to stable versions matching the current platform.

use async_trait::async_trait;
use serde::Deserialize;

use crate::environment::http_client::get_http_client;
use crate::environment::registry::{RuntimeVersion, VersionSource};
use crate::environment::RuntimeType;
use crate::error::Result;

/// Raw response entry from https://go.dev/dl/?mode=json
#[derive(Debug, Deserialize)]
struct GoDistEntry {
    version: String,       // "go1.22.4"
    stable: bool,
    files: Vec<GoFile>,
}

#[derive(Debug, Deserialize)]
struct GoFile {
    filename: String,
    os: String,
    arch: String,
    #[allow(dead_code)]
    sha256: String,
    size: Option<u64>,
}

pub struct GoVersionSource;

impl GoVersionSource {
    fn current_os(&self) -> &'static str {
        #[cfg(target_os = "windows")]
        { "windows" }
        #[cfg(target_os = "linux")]
        { "linux" }
        #[cfg(target_os = "macos")]
        { "darwin" }
        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        { "linux" }
    }

    fn current_arch(&self) -> &'static str {
        #[cfg(target_os = "windows")]
        { "amd64" }
        #[cfg(target_os = "linux")]
        { "amd64" }
        #[cfg(target_os = "macos")]
        { "arm64" }
        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        { "amd64" }
    }

    /// Strip "go" prefix from version string.
    /// "go1.22.4" -> "1.22.4"
    fn strip_go_prefix(version: &str) -> String {
        version.strip_prefix("go").unwrap_or(version).to_string()
    }
}

#[async_trait]
impl VersionSource for GoVersionSource {
    fn runtime_type(&self) -> RuntimeType {
        RuntimeType::Go
    }

    async fn fetch_versions(&self) -> Result<Vec<RuntimeVersion>> {
        let current_os = self.current_os();
        let current_arch = self.current_arch();

        let client = get_http_client();
        let response = client
            .get("https://go.dev/dl/?mode=json")
            .send()
            .await
            .map_err(|e| crate::error::AppError::Http(e))?;

        let entries: Vec<GoDistEntry> = response
            .json()
            .await
            .map_err(|e| crate::error::AppError::Http(e))?;

        let mut versions: Vec<RuntimeVersion> = entries
            .into_iter()
            .filter(|e| e.stable)
            .filter_map(|e| {
                let version = Self::strip_go_prefix(&e.version);

                // Find the file that matches current platform
                let matching_file = e.files.iter().find(|f| {
                    f.os == current_os && f.arch == current_arch
                })?;

                // Build download URL from filename
                let url = format!(
                    "https://dl.google.com/go/{}",
                    matching_file.filename
                );

                Some(RuntimeVersion {
                    runtime_type: RuntimeType::Go,
                    version: version.clone(),
                    display_name: format!("Go {}", version),
                    url,
                    lts: None,
                    is_stable: true,
                    release_date: None,
                    file_size: matching_file.size,
                })
            })
            .collect();

        // Sort: newest version first
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
