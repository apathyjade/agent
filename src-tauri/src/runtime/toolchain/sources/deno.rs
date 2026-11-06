//! Deno 版本源。
//!
//! 通过 GitHub Releases API 获取可用的 Deno 版本，过滤掉预发布版本，
//! 并按当前平台拼出下载地址。

use async_trait::async_trait;
use serde::Deserialize;

use crate::core::error::Result;
use crate::runtime::toolchain::http_client::get_http_client;
use crate::runtime::toolchain::platform;
use crate::runtime::toolchain::registry::{RuntimeVersion, VersionSource};
use crate::runtime::toolchain::version::sort_versions_desc;
use crate::runtime::toolchain::RuntimeType;

/// GitHub release 条目。
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

/// GitHub release 中的单个资产。
#[derive(Debug, Deserialize)]
struct GitHubAsset {
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    browser_download_url: String,
    #[allow(dead_code)]
    size: Option<u64>,
}

/// Deno 版本源：从 GitHub Releases 拉取稳定版本。
pub struct DenoVersionSource;


#[async_trait]
impl VersionSource for DenoVersionSource {
    fn runtime_type(&self) -> RuntimeType {
        RuntimeType::Deno
    }

    async fn fetch_versions(&self) -> Result<Vec<RuntimeVersion>> {
        let suffix = platform::deno_asset_suffix();
        let ext = platform::archive_ext(&RuntimeType::Deno);

        // GitHub API 要求携带 User-Agent 头
        let client = get_http_client();
        let response = client
            .get("https://api.github.com/repos/denoland/deno/releases?per_page=20")
            .header("User-Agent", "agent-runtime-manager/1.0")
            .send()
            .await
            .map_err(crate::core::error::AppError::Http)?;

        let releases: Vec<GitHubRelease> = response
            .json()
            .await
            .map_err(crate::core::error::AppError::Http)?;

        let mut versions: Vec<RuntimeVersion> = releases
            .into_iter()
            .filter(|r| !r.prerelease)
            .map(|r| {
                let tag = r
                    .tag_name
                    .strip_prefix('v')
                    .unwrap_or(&r.tag_name)
                    .to_string();

                // 下载地址按 Deno 固定的发布命名规则拼接
                let url = format!(
                    "https://github.com/denoland/deno/releases/download/v{}/deno-{}",
                    tag, suffix
                );

                RuntimeVersion {
                    runtime_type: RuntimeType::Deno,
                    version: tag.clone(),
                    display_name: format!("Deno {}", tag),
                    url,
                    lts: None,
                    is_stable: true,
                    release_date: None,
                    file_size: None,
                }
            })
            .collect();

        // 排序：新版本在前（逐段数值比较）
        sort_versions_desc(&mut versions);

        // API 没有返回任何版本时使用内置回退列表
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
            .map(|v| RuntimeVersion {
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
            })
            .collect()
    }
}
