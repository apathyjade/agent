// ── Rust Version Source ──
//
// Rust versions are managed via rustup. Provides a curated list of stable
// and nightly toolchains. Optionally fetches the latest channel metadata
// from the official Rust static archive.

use async_trait::async_trait;

use crate::environment::http_client::get_http_client;
use crate::environment::registry::{RuntimeVersion, VersionSource};
use crate::environment::RuntimeType;
use crate::error::Result;

pub struct RustupVersionSource;

#[async_trait]
impl VersionSource for RustupVersionSource {
    fn runtime_type(&self) -> RuntimeType {
        RuntimeType::Rust
    }

    async fn fetch_versions(&self) -> Result<Vec<RuntimeVersion>> {
        // Try fetching the latest stable version from the Rust channel manifest
        let channel_versions = self.fetch_channel_versions().await;

        let mut versions: Vec<RuntimeVersion> = Vec::new();

        // Add channel entries (stable and nightly) first
        versions.push(RuntimeVersion {
            runtime_type: RuntimeType::Rust,
            version: "stable".to_string(),
            display_name: "Rust stable".to_string(),
            url: String::new(),
            lts: None,
            is_stable: true,
            release_date: None,
            file_size: None,
        });

        versions.push(RuntimeVersion {
            runtime_type: RuntimeType::Rust,
            version: "nightly".to_string(),
            display_name: "Rust nightly".to_string(),
            url: String::new(),
            lts: None,
            is_stable: false,
            release_date: None,
            file_size: None,
        });

        // Add specific versions if we fetched them, otherwise use fallback list
        let numbered = if let Ok(fetched) = channel_versions {
            fetched
        } else {
            // Fallback: known recent stable versions
            vec![
                "1.85.0".to_string(),
                "1.84.1".to_string(),
                "1.83.0".to_string(),
                "1.82.0".to_string(),
                "1.81.0".to_string(),
            ]
        };

        for ver in numbered {
            versions.push(RuntimeVersion {
                runtime_type: RuntimeType::Rust,
                version: ver.clone(),
                display_name: format!("Rust {}", ver),
                url: String::new(),
                lts: None,
                is_stable: true,
                release_date: None,
                file_size: None,
            });
        }

        Ok(versions)
    }
}

impl RustupVersionSource {
    /// Attempt to parse the latest stable rust version from the official channel manifest.
    async fn fetch_channel_versions(&self) -> Result<Vec<String>> {
        let url = "https://static.rust-lang.org/dist/channel-rust-stable.toml";
        let client = get_http_client();
        let response = client
            .get(url)
            .send()
            .await
            .map_err(|e| crate::error::AppError::Http(e))?;

        let text = response
            .text()
            .await
            .map_err(|e| crate::error::AppError::Http(e))?;

        // Parse the TOML to find "pkg.rust.version"
        // The format looks like:
        // [pkg.rust]
        // version = "1.85.0"
        let mut versions = Vec::new();
        for line in text.lines() {
            if line.trim().starts_with("version") && line.contains('=') {
                if let Some(ver) = line.split('=').nth(1) {
                    let ver = ver.trim().trim_matches('"').trim();
                    if !ver.is_empty() {
                        versions.push(ver.to_string());
                        // Only need the first version match
                        break;
                    }
                }
            }
        }

        if versions.is_empty() {
            return Err(crate::error::AppError::NotFound(
                "无法解析 Rust 稳定版本".to_string(),
            ));
        }

        Ok(versions)
    }
}
