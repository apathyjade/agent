//! Python 版本源。
//!
//! 从 python-build-standalone 的 GitHub Releases 获取可用版本；API 失败时
//! 回退到一份人工维护的版本列表。

use async_trait::async_trait;
use serde::Deserialize;

use crate::core::error::Result;
use crate::runtime::toolchain::http_client::get_http_client;
use crate::runtime::toolchain::platform;
use crate::runtime::toolchain::registry::{RuntimeVersion, VersionSource};
use crate::runtime::toolchain::RuntimeType;

/// GitHub release API 响应条目。
#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    published_at: String,
}

/// GitHub API 不可用时的已知版本回退列表。
const FALLBACK_VERSIONS: &[&str] = &["3.13.3", "3.12.10", "3.11.12", "3.10.20"];

/// Python 版本源：从 python-build-standalone 拉取版本。
pub struct PythonVersionSource;

impl PythonVersionSource {
    /// 拼接某个 Python 版本的下载地址（python-build-standalone 命名规则）。
    fn build_download_url(&self, version: &str, tag: &str) -> String {
        let platform_label = platform::rust_triple();
        let ext = platform::archive_ext(&RuntimeType::Python);
        let archive_name = format!("cpython-{}+{}-{}-install_only", version, tag, platform_label);
        format!(
            "https://github.com/astral-sh/python-build-standalone/releases/download/{}/{}",
            tag, archive_name
        ) + "."
            + ext
    }

    /// 从 release 标签解析版本号。
    ///
    /// python-build-standalone 的 tag 只表示构建日期（如 "20250115"），不直接
    /// 编码 Python 版本，因此这里沿用回退列表，并把该 tag 作为下载路径的一部分。
    fn parse_versions_from_release(&self, release: &GitHubRelease) -> Vec<(String, String)> {
        let tag = &release.tag_name;
        // python-build-standalone 的 tag 不直接编码 Python 版本。
        // 若要完整实现，应解析 release body 或使用 manifest。
        // 当前实现返回回退列表，并复用本次 release 的 tag。
        FALLBACK_VERSIONS
            .iter()
            .map(|v| (v.to_string(), tag.clone()))
            .collect()
    }

    /// 把「版本号 + tag」组合转换成 `RuntimeVersion` 列表。
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
        // 优先尝试 GitHub API
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
                        // 继续走回退逻辑
                    }
                    Err(_) => { /* 继续走回退逻辑 */ }
                }
            }
            Ok(response) => {
                // 被限流或其它错误 —— 使用回退列表
                log::warn!(
                    "GitHub API returned {} for Python versions, using fallback",
                    response.status()
                );
            }
            Err(e) => {
                log::warn!("Failed to fetch Python versions from GitHub: {}, using fallback", e);
            }
        }

        // 回退：使用已知版本列表和默认 tag
        let default_tag = "20260510";
        let pairs: Vec<(String, String)> = FALLBACK_VERSIONS
            .iter()
            .map(|v| (v.to_string(), default_tag.to_string()))
            .collect();
        Ok(self.build_versions(pairs))
    }
}
