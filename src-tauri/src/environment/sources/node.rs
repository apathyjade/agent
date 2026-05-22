// ── Node.js Version Source ──
//
// Fetches available Node.js versions from the official nodejs.org dist index.
// Supports three discovery strategies:
//   1. Standalone: direct download from nodejs.org (original behavior)
//   2. WrapExisting: delegate to fnm/volta/nvm
//   3. Hybrid: combine external + standalone with dedup
//
// Also supports:
//   - Mirrored download sources (e.g., npmmirror.com for China)
//   - SHASUMS256.txt signature verification
//   - Windows junction-based activation fallback

use async_trait::async_trait;
use serde::Deserialize;

use crate::environment::http_client::get_http_client;
use crate::environment::node_integration::{ExternalNodeManager, ExternalManager, NodeIntegrationStrategy};
use crate::environment::registry::{RuntimeVersion, VersionSource};
use crate::environment::RuntimeType;
use crate::error::Result;

// ── Download Mirror ──

/// Node.js download mirror source.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum NodeMirror {
    /// Official nodejs.org (default)
    Official,
    /// NPMMirror (Chinese mirror, formerly taobao)
    NpmMirror,
    /// Custom mirror
    Custom {
        index_url: String,
        download_base: String,
    },
}

impl NodeMirror {
    pub fn index_url(&self) -> &str {
        match self {
            NodeMirror::Official => "https://nodejs.org/dist/index.json",
            NodeMirror::NpmMirror => "https://npmmirror.com/mirrors/node/index.json",
            NodeMirror::Custom { index_url, .. } => index_url,
        }
    }

    pub fn download_url(&self, version: &str, platform: &str, ext: &str) -> String {
        let base = match self {
            NodeMirror::Official => "https://nodejs.org/dist",
            NodeMirror::NpmMirror => "https://npmmirror.com/mirrors/node",
            NodeMirror::Custom { download_base, .. } => download_base.trim_end_matches('/'),
        };
        format!("{base}/v{version}/node-v{version}-{platform}.{ext}")
    }

    pub fn shasums_url(&self, version: &str) -> String {
        let base = match self {
            NodeMirror::Official => "https://nodejs.org/dist",
            NodeMirror::NpmMirror => "https://npmmirror.com/mirrors/node",
            NodeMirror::Custom { download_base, .. } => download_base.trim_end_matches('/'),
        };
        format!("{base}/v{version}/SHASUMS256.txt")
    }
}

impl Default for NodeMirror {
    fn default() -> Self {
        Self::Official
    }
}

// ── Windows Activation Strategy ──

/// Strategy for making a Node.js version active on Windows.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum WindowsActivationStrategy {
    /// Symbolic link (requires admin or Developer Mode)
    Symlink,
    /// Directory junction + PATH injection (recommended, no admin needed)
    PathJunction,
    /// Temporary PATH modification (fallback, process-scoped)
    ProcessPath,
}

// ── Raw API Types ──

/// Raw response entry from https://nodejs.org/dist/index.json
#[derive(Debug, Deserialize)]
struct NodeDistEntry {
    version: String,        // "v20.18.3"
    lts: serde_json::Value, // false or a string like "Jod"
    date: String,           // "2025-01-15"
    files: Vec<String>,     // ["win-x64.zip", "linux-x64.tar.gz", ...]
}

// ── NodeVersionSource ──

pub struct NodeVersionSource {
    /// Integration strategy: standalone / wrap-existing / hybrid
    pub strategy: NodeIntegrationStrategy,
    /// Download mirror (for geo-optimized downloads)
    pub mirror: NodeMirror,
    /// Whether to verify SHASUMS256.txt signatures
    pub verify_checksum: bool,
    /// Windows activation strategy
    #[cfg(target_os = "windows")]
    pub windows_activation: WindowsActivationStrategy,
}

impl NodeVersionSource {
    pub fn new() -> Self {
        Self {
            strategy: NodeIntegrationStrategy::default(),
            mirror: NodeMirror::default(),
            verify_checksum: true,
            #[cfg(target_os = "windows")]
            windows_activation: WindowsActivationStrategy::PathJunction,
        }
    }

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

    /// Fetch versions from the official Node.js dist index.
    async fn fetch_from_nodejs_org(&self) -> Result<Vec<RuntimeVersion>> {
        let platform = self.platform_file_pattern();
        let ext = self.archive_ext();

        let client = get_http_client();
        let response = client
            .get(self.mirror.index_url())
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
                let has_platform_file = e.files.iter().any(|f| f.contains(platform));
                let is_stable = !matches!(&e.lts, serde_json::Value::Bool(false));
                has_platform_file && is_stable
            })
            .filter_map(|e| {
                let version = e.version.strip_prefix('v').unwrap_or(&e.version).to_string();
                let lts = e.lts.as_str().map(|s| s.to_string());
                let url = self.mirror.download_url(&version, platform, ext);

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

    /// Fetch versions from the best available external manager.
    async fn fetch_from_external_manager(&self) -> Result<Vec<RuntimeVersion>> {
        let managers = ExternalNodeManager::detect_all().await;
        let available: Vec<ExternalManager> = managers.into_iter()
            .filter(|(_, available)| *available)
            .map(|(mgr, _)| mgr)
            .collect();

        if available.is_empty() {
            return Err(crate::error::AppError::NotFound(
                "未检测到外部 Node.js 版本管理器 (fnm/volta/nvm)".into()
            ));
        }

        let mut combined = Vec::new();

        for manager in &available {
            let versions = ExternalNodeManager::list_manager_versions(manager).await;
            for ver in versions {
                let url = self.mirror.download_url(&ver, self.platform_file_pattern(), self.archive_ext());
                combined.push(RuntimeVersion {
                    runtime_type: RuntimeType::Node,
                    version: ver.clone(),
                    display_name: format!("Node.js v{} [{}]", ver, manager.name()),
                    url,
                    lts: None,
                    is_stable: true,
                    release_date: None,
                    file_size: None,
                });
            }
        }

        // Deduplicate by version (keep first occurrence = external manager priority)
        combined.sort_by(|a, b| a.version.cmp(&b.version));
        combined.dedup_by(|a, b| a.version == b.version);

        if combined.is_empty() {
            return Err(crate::error::AppError::NotFound(
                "外部管理器已检测到但没有可用的 Node.js 版本".into()
            ));
        }

        Ok(combined)
    }

    /// Verify a downloaded binary against the official SHASUMS256.txt.
    pub async fn verify_download(&self, version: &str, binary: &[u8]) -> Result<bool> {
        if !self.verify_checksum {
            return Ok(true);
        }

        let platform = self.platform_file_pattern();
        let ext = self.archive_ext();
        let shasums_url = self.mirror.shasums_url(version);
        let filename = format!("node-v{}-{}.{}", version, platform, ext);

        let client = get_http_client();
        let response = client
            .get(&shasums_url)
            .send()
            .await
            .map_err(|e| crate::error::AppError::Http(e))?;

        if !response.status().is_success() {
            log::warn!("无法获取 SHASUMS256.txt ({}), 跳过校验", response.status());
            return Ok(true);
        }

        let text = response
            .text()
            .await
            .map_err(|e| crate::error::AppError::Http(e))?;

        // Find expected hash for our file
        let expected_hash = text
            .lines()
            .find(|line| line.contains(&filename))
            .and_then(|line| line.split_whitespace().next())
            .ok_or_else(|| {
                crate::error::AppError::NotFound(format!(
                    "未在 SHASUMS256.txt 中找到 {} 的校验和", filename
                ))
            })?;

        // Compute actual SHA256
        let actual_hash = {
            use sha2::Digest;
            let mut hasher = sha2::Sha256::new();
            hasher.update(binary);
            let result = hasher.finalize();
            format!("{:x}", result)
        };

        if actual_hash != expected_hash {
            return Err(crate::error::AppError::InvalidInput(
                format!("SHA256 校验失败: 期望 {}，实际 {}", expected_hash, actual_hash)
            ));
        }

        log::info!("SHA256 校验通过: {}", filename);
        Ok(true)
    }

    /// Check if symlinks are supported on Windows (admin or Developer Mode).
    #[cfg(target_os = "windows")]
    pub fn can_create_symlink() -> bool {
        let tmp_dir = std::env::temp_dir();
        let tmp_link = tmp_dir.join("_agent_symlink_test_node");
        let tmp_target = tmp_dir.join("_agent_symlink_target_node");
        let _ = std::fs::write(&tmp_target, b"test");
        let result = std::os::windows::fs::symlink_file(&tmp_target, &tmp_link);
        let _ = std::fs::remove_file(&tmp_link);
        let _ = std::fs::remove_file(&tmp_target);
        result.is_ok()
    }
}

impl Default for NodeVersionSource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl VersionSource for NodeVersionSource {
    fn runtime_type(&self) -> RuntimeType {
        RuntimeType::Node
    }

    async fn fetch_versions(&self) -> Result<Vec<RuntimeVersion>> {
        match &self.strategy {
            NodeIntegrationStrategy::Standalone => {
                self.fetch_from_nodejs_org().await
            }
            NodeIntegrationStrategy::WrapExisting => {
                self.fetch_from_external_manager().await
            }
            NodeIntegrationStrategy::Hybrid => {
                // Try external managers first, fall back to self-managed
                let external_result = self.fetch_from_external_manager().await;

                match external_result {
                    Ok(mut ext_versions) => {
                        // Supplement with standalone versions (dedup)
                        if let Ok(standalone) = self.fetch_from_nodejs_org().await {
                            let existing: std::collections::HashSet<String> =
                                ext_versions.iter().map(|v| v.version.clone()).collect();
                            for v in standalone {
                                if !existing.contains(&v.version) {
                                    ext_versions.push(v);
                                }
                            }
                        }
                        // Resorted
                        ext_versions.sort_by(|a, b| {
                            let a_p: Vec<u64> = a.version.split('.').filter_map(|s| s.parse().ok()).collect();
                            let b_p: Vec<u64> = b.version.split('.').filter_map(|s| s.parse().ok()).collect();
                            for (av, bv) in a_p.iter().zip(b_p.iter()) {
                                if av != bv { return bv.cmp(av); }
                            }
                            b_p.len().cmp(&a_p.len())
                        });
                        Ok(ext_versions)
                    }
                    Err(_) => {
                        // No external manager available, use standalone
                        self.fetch_from_nodejs_org().await
                    }
                }
            }
        }
    }
}
