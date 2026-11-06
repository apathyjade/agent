//! Node.js 版本源。
//!
//! 从官方 nodejs.org dist 索引获取可用 Node.js 版本，支持三种发现策略：
//!   1. Standalone：直接从 nodejs.org 下载（原始行为）
//!   2. WrapExisting：委托给 fnm / volta / nvm
//!   3. Hybrid：外部管理器与内置下载合并去重
//!
//! 另外支持：
//!   - 镜像下载源（例如国内使用的 npmmirror.com）
//!   - SHASUMS256.txt 校验和验证
//!   - Windows 上基于 junction 的激活回退方案

use async_trait::async_trait;
use serde::Deserialize;

use crate::core::error::Result;
use crate::runtime::toolchain::http_client::get_http_client;
use crate::runtime::toolchain::node_integration::{
    ExternalManager, ExternalNodeManager, NodeIntegrationStrategy,
};
use crate::runtime::toolchain::platform;
use crate::runtime::toolchain::registry::{RuntimeVersion, VersionSource};
use crate::runtime::toolchain::version::sort_versions_desc;
use crate::runtime::toolchain::RuntimeType;

// ── 下载镜像 ──

/// Node.js 下载镜像源。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub enum NodeMirror {
    /// 官方 nodejs.org（默认）
    #[default]
    Official,
    /// NPMMirror（国内镜像，原淘宝源）
    NpmMirror,
    /// 自定义镜像
    Custom {
        index_url: String,
        download_base: String,
    },
}

impl NodeMirror {
    /// 版本索引 URL。
    pub fn index_url(&self) -> &str {
        match self {
            NodeMirror::Official => "https://nodejs.org/dist/index.json",
            NodeMirror::NpmMirror => "https://npmmirror.com/mirrors/node/index.json",
            NodeMirror::Custom { index_url, .. } => index_url,
        }
    }

    /// 按镜像地址拼接某个版本的下载 URL。
    pub fn download_url(&self, version: &str, platform: &str, ext: &str) -> String {
        let base = match self {
            NodeMirror::Official => "https://nodejs.org/dist",
            NodeMirror::NpmMirror => "https://npmmirror.com/mirrors/node",
            NodeMirror::Custom { download_base, .. } => download_base.trim_end_matches('/'),
        };
        format!("{base}/v{version}/node-v{version}-{platform}.{ext}")
    }

    /// 按镜像地址拼接 SHASUMS256.txt 的 URL。
    pub fn shasums_url(&self, version: &str) -> String {
        let base = match self {
            NodeMirror::Official => "https://nodejs.org/dist",
            NodeMirror::NpmMirror => "https://npmmirror.com/mirrors/node",
            NodeMirror::Custom { download_base, .. } => download_base.trim_end_matches('/'),
        };
        format!("{base}/v{version}/SHASUMS256.txt")
    }
}

// ── Windows 激活策略 ──

/// 在 Windows 上让某个 Node.js 版本生效的策略。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum WindowsActivationStrategy {
    /// 符号链接（需要管理员权限或开发者模式）
    Symlink,
    /// 目录 junction + PATH 注入（推荐，无需管理员权限）
    PathJunction,
    /// 临时修改 PATH（回退方案，仅进程内有效）
    ProcessPath,
}

// ── 原始 API 类型 ──

/// https://nodejs.org/dist/index.json 的原始响应条目
#[derive(Debug, Deserialize)]
struct NodeDistEntry {
    version: String,        // "v20.18.3"
    lts: serde_json::Value, // false 或类似 "Jod" 的字符串
    date: String,           // "2025-01-15"
    files: Vec<String>,     // ["win-x64.zip", "linux-x64.tar.gz", ...]
}

// ── NodeVersionSource（Node.js 版本来源） ──

/// Node.js 版本源，按配置的集成策略、镜像与校验选项工作。
pub struct NodeVersionSource {
    /// 集成策略：standalone / wrap-existing / hybrid
    pub strategy: NodeIntegrationStrategy,
    /// 下载镜像（用于按地区优化下载）
    pub mirror: NodeMirror,
    /// 是否校验 SHASUMS256.txt
    pub verify_checksum: bool,
    /// Windows 激活策略
    #[cfg(target_os = "windows")]
    pub windows_activation: WindowsActivationStrategy,
}

impl NodeVersionSource {
    /// 使用默认配置创建版本源：Hybrid 策略、官方镜像、开启校验。
    pub fn new() -> Self {
        Self {
            strategy: NodeIntegrationStrategy::default(),
            mirror: NodeMirror::default(),
            verify_checksum: true,
            #[cfg(target_os = "windows")]
            windows_activation: WindowsActivationStrategy::PathJunction,
        }
    }

    /// 从官方 Node.js dist 索引获取版本。
    async fn fetch_from_nodejs_org(&self) -> Result<Vec<RuntimeVersion>> {
        let platform_label = platform::node_dist_platform();
        let ext = platform::archive_ext(&RuntimeType::Node);

        let client = get_http_client();
        let response = client
            .get(self.mirror.index_url())
            .send()
            .await
            .map_err(crate::core::error::AppError::Http)?;

        let entries: Vec<NodeDistEntry> = response
            .json()
            .await
            .map_err(crate::core::error::AppError::Http)?;

        let mut versions: Vec<RuntimeVersion> = entries
            .into_iter()
            .filter(|e| {
                let has_platform_file = e.files.iter().any(|f| f.contains(platform_label));
                let is_stable = !matches!(&e.lts, serde_json::Value::Bool(false));
                has_platform_file && is_stable
            })
            .map(|e| {
                let version = e
                    .version
                    .strip_prefix('v')
                    .unwrap_or(&e.version)
                    .to_string();
                let lts = e.lts.as_str().map(|s| s.to_string());
                let url = self.mirror.download_url(&version, platform_label, ext);

                RuntimeVersion {
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
                }
            })
            .collect();

        // 排序：新版本在前（逐段数值比较）
        sort_versions_desc(&mut versions);

        Ok(versions)
    }

    /// 从可用的外部版本管理器中获取版本。
    async fn fetch_from_external_manager(&self) -> Result<Vec<RuntimeVersion>> {
        let managers = ExternalNodeManager::detect_all().await;
        let available: Vec<ExternalManager> = managers
            .into_iter()
            .filter(|(_, available)| *available)
            .map(|(mgr, _)| mgr)
            .collect();

        if available.is_empty() {
            return Err(crate::core::error::AppError::NotFound(
                "未检测到外部 Node.js 版本管理器 (fnm/volta/nvm)".into(),
            ));
        }

        let mut combined = Vec::new();

        for manager in &available {
            let versions = ExternalNodeManager::list_manager_versions(manager).await;
            for ver in versions {
                let url = self.mirror.download_url(
                    &ver,
                    platform::node_dist_platform(),
                    platform::archive_ext(&RuntimeType::Node),
                );
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

        // 按版本号去重（保留首次出现，即外部管理器优先级更高）
        combined.sort_by(|a, b| a.version.cmp(&b.version));
        combined.dedup_by(|a, b| a.version == b.version);

        if combined.is_empty() {
            return Err(crate::core::error::AppError::NotFound(
                "外部管理器已检测到但没有可用的 Node.js 版本".into(),
            ));
        }

        Ok(combined)
    }

    /// 用官方 SHASUMS256.txt 校验下载的二进制。
    pub async fn verify_download(&self, version: &str, binary: &[u8]) -> Result<bool> {
        if !self.verify_checksum {
            return Ok(true);
        }

        let platform_label = platform::node_dist_platform();
        let ext = platform::archive_ext(&RuntimeType::Node);
        let shasums_url = self.mirror.shasums_url(version);
        let filename = format!("node-v{}-{}.{}", version, platform_label, ext);

        let client = get_http_client();
        let response = client
            .get(&shasums_url)
            .send()
            .await
            .map_err(crate::core::error::AppError::Http)?;

        if !response.status().is_success() {
            log::warn!("无法获取 SHASUMS256.txt ({}), 跳过校验", response.status());
            return Ok(true);
        }

        let text = response
            .text()
            .await
            .map_err(crate::core::error::AppError::Http)?;

        // 找到本文件对应的期望哈希
        let expected_hash = text
            .lines()
            .find(|line| line.contains(&filename))
            .and_then(|line| line.split_whitespace().next())
            .ok_or_else(|| {
                crate::core::error::AppError::NotFound(format!(
                    "未在 SHASUMS256.txt 中找到 {} 的校验和",
                    filename
                ))
            })?;

        // 计算实际的 SHA256
        let actual_hash = {
            use sha2::Digest;
            let mut hasher = sha2::Sha256::new();
            hasher.update(binary);
            let result = hasher.finalize();
            format!("{:x}", result)
        };

        if actual_hash != expected_hash {
            return Err(crate::core::error::AppError::InvalidInput(format!(
                "SHA256 校验失败: 期望 {}，实际 {}",
                expected_hash, actual_hash
            )));
        }

        log::info!("SHA256 校验通过: {}", filename);
        Ok(true)
    }

    /// 检查 Windows 上是否支持创建符号链接（管理员权限或开发者模式）。
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
            NodeIntegrationStrategy::Standalone => self.fetch_from_nodejs_org().await,
            NodeIntegrationStrategy::WrapExisting => self.fetch_from_external_manager().await,
            NodeIntegrationStrategy::Hybrid => {
                // 优先使用外部管理器，失败时回退到内置下载
                let external_result = self.fetch_from_external_manager().await;

                match external_result {
                    Ok(mut ext_versions) => {
                        // 用内置版本补充（去重）
                        if let Ok(standalone) = self.fetch_from_nodejs_org().await {
                            let existing: std::collections::HashSet<String> =
                                ext_versions.iter().map(|v| v.version.clone()).collect();
                            for v in standalone {
                                if !existing.contains(&v.version) {
                                    ext_versions.push(v);
                                }
                            }
                        }
                        // 重新排序
                        sort_versions_desc(&mut ext_versions);
                        Ok(ext_versions)
                    }
                    Err(_) => {
                        // 没有可用的外部管理器，使用内置下载
                        self.fetch_from_nodejs_org().await
                    }
                }
            }
        }
    }
}
