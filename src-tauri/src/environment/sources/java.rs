// ── Java Version Source ──
//
// Fetches available JDK versions from the Adoptium API.
// Supports major versions: 8, 11, 17, 21, 23.
// Falls back to hardcoded versions on API failure.

use async_trait::async_trait;
use serde::Deserialize;

use crate::environment::http_client::get_http_client;
use crate::environment::registry::{RuntimeVersion, VersionSource};
use crate::environment::RuntimeType;
use crate::error::Result;

/// Top-level response from Adoptium API.
#[derive(Debug, Deserialize)]
struct AdoptiumResponse {
    #[allow(dead_code)]
    count: Option<i64>,
    assets: Option<Vec<AdoptiumAsset>>,
}

/// A single asset entry from the Adoptium API.
#[derive(Debug, Deserialize)]
struct AdoptiumAsset {
    version: AdoptiumVersion,
    binaries: Option<Vec<AdoptiumBinary>>,
    #[allow(dead_code)]
    release_date: Option<String>,
    #[allow(dead_code)]
    release_name: Option<String>,
}

/// Version metadata from Adoptium.
#[derive(Debug, Deserialize)]
struct AdoptiumVersion {
    semver: Option<String>,
    #[allow(dead_code)]
    major: Option<i64>,
    #[allow(dead_code)]
    minor: Option<i64>,
    #[allow(dead_code)]
    patch: Option<i64>,
    #[allow(dead_code)]
    raw: Option<String>,
}

/// Binary info from Adoptium.
#[derive(Debug, Deserialize)]
struct AdoptiumBinary {
    os: Option<String>,
    architecture: Option<String>,
    package: Option<AdoptiumPackage>,
}

/// Package download info.
#[derive(Debug, Deserialize)]
struct AdoptiumPackage {
    #[allow(dead_code)]
    name: Option<String>,
    link: Option<String>,
}

pub struct JavaVersionSource;

impl JavaVersionSource {
    /// Feature versions to query.
    fn feature_versions() -> &'static [i64] {
        &[23, 21, 17, 11, 8]
    }

    fn current_os() -> &'static str {
        #[cfg(target_os = "windows")]
        { "windows" }
        #[cfg(target_os = "linux")]
        { "linux" }
        #[cfg(target_os = "macos")]
        { "mac" }
    }
}

#[async_trait]
impl VersionSource for JavaVersionSource {
    fn runtime_type(&self) -> RuntimeType {
        RuntimeType::Java
    }

    async fn fetch_versions(&self) -> Result<Vec<RuntimeVersion>> {
        let os = Self::current_os();

        // Try to fetch from API for each feature version
        let mut versions = Vec::new();

        for &feature_ver in Self::feature_versions() {
            match self.fetch_feature_version(feature_ver, os).await {
                Ok(Some(rv)) => versions.push(rv),
                Ok(None) => {} // skip if no matching binary
                Err(_) => {
                    // API failure for this feature version — continue to next
                }
            }
        }

        // If API returned nothing, use fallback
        if versions.is_empty() {
            versions = Self::fallback_versions();
        }

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

impl JavaVersionSource {
    /// Fetch a single feature version from the Adoptium API.
    async fn fetch_feature_version(
        &self,
        feature_ver: i64,
        os: &str,
    ) -> Result<Option<RuntimeVersion>> {
        let url = format!(
            "https://api.adoptium.net/v3/assets/version/{}?os={}&arch=x64&image_type=jdk",
            feature_ver, os
        );

        let client = get_http_client();
        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| crate::error::AppError::Http(e))?;

        let adopt_response: AdoptiumResponse = response
            .json()
            .await
            .map_err(|e| crate::error::AppError::Http(e))?;

        let assets = match adopt_response.assets {
            Some(a) => a,
            None => return Ok(None),
        };

        // Find the first asset with a matching binary for our platform
        for asset in assets {
            let semver = match asset.version.semver {
                Some(ref s) => s.clone(),
                None => continue,
            };

            let binaries = match asset.binaries {
                Some(ref b) => b,
                None => continue,
            };

            // Find a matching binary for our OS and architecture
            let matching_binary = binaries.iter().find(|b| {
                b.os.as_deref() == Some(os) && b.architecture.as_deref() == Some("x64")
            });

            if let Some(binary) = matching_binary {
                let download_url = binary
                    .package
                    .as_ref()
                    .and_then(|p| p.link.clone())
                    .unwrap_or_default();

                // Build display name like "JDK 21.0.7 (LTS)"
                let display_name = match feature_ver {
                    21 | 17 | 11 | 8 => format!("JDK {} (LTS)", semver),
                    _ => format!("JDK {}", semver),
                };

                return Ok(Some(RuntimeVersion {
                    runtime_type: RuntimeType::Java,
                    version: semver,
                    display_name,
                    url: download_url,
                    lts: match feature_ver {
                        21 | 17 | 11 | 8 => Some(format!("JDK {}", feature_ver)),
                        _ => None,
                    },
                    is_stable: true,
                    release_date: None,
                    file_size: None,
                }));
            }
        }

        Ok(None)
    }

    /// Fallback hardcoded versions when the API is unavailable.
    fn fallback_versions() -> Vec<RuntimeVersion> {
        vec![
            RuntimeVersion {
                runtime_type: RuntimeType::Java,
                version: "21.0.7".to_string(),
                display_name: "JDK 21.0.7 (LTS)".to_string(),
                url: "https://github.com/adoptium/temurin21-binaries/releases/download/jdk-21.0.7%2B9/OpenJDK21U-jdk_x64_windows_hotspot_21.0.7_9.zip".to_string(),
                lts: Some("JDK 21".to_string()),
                is_stable: true,
                release_date: None,
                file_size: None,
            },
            RuntimeVersion {
                runtime_type: RuntimeType::Java,
                version: "17.0.14".to_string(),
                display_name: "JDK 17.0.14 (LTS)".to_string(),
                url: "https://github.com/adoptium/temurin17-binaries/releases/download/jdk-17.0.14%2B7/OpenJDK17U-jdk_x64_windows_hotspot_17.0.14_7.zip".to_string(),
                lts: Some("JDK 17".to_string()),
                is_stable: true,
                release_date: None,
                file_size: None,
            },
            RuntimeVersion {
                runtime_type: RuntimeType::Java,
                version: "11.0.26".to_string(),
                display_name: "JDK 11.0.26 (LTS)".to_string(),
                url: "https://github.com/adoptium/temurin11-binaries/releases/download/jdk-11.0.26%2B4/OpenJDK11U-jdk_x64_windows_hotspot_11.0.26_4.zip".to_string(),
                lts: Some("JDK 11".to_string()),
                is_stable: true,
                release_date: None,
                file_size: None,
            },
            RuntimeVersion {
                runtime_type: RuntimeType::Java,
                version: "8.0.442".to_string(),
                display_name: "JDK 8.0.442 (LTS)".to_string(),
                url: "https://github.com/adoptium/temurin8-binaries/releases/download/jdk8u442-b06/OpenJDK8U-jdk_x64_windows_hotspot_8u442b06.zip".to_string(),
                lts: Some("JDK 8".to_string()),
                is_stable: true,
                release_date: None,
                file_size: None,
            },
        ]
    }
}
