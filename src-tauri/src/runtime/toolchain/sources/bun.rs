//! Bun 版本源。
//!
//! 通过 GitHub Releases API 获取可用的 Bun 版本。Bun 没有 LTS 概念，
//! 所有稳定版本都视为 Active。

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
    published_at: Option<String>,
}

/// Bun 版本源：从 GitHub Releases 拉取稳定版本。
pub struct BunVersionSource;


#[async_trait]
impl VersionSource for BunVersionSource {
    fn runtime_type(&self) -> RuntimeType {
        RuntimeType::Bun
    }

    async fn fetch_versions(&self) -> Result<Vec<RuntimeVersion>> {
        let asset_name = platform::bun_asset_name();
        let ext = platform::archive_ext(&RuntimeType::Bun);

        let client = get_http_client();
        let response = client
            .get("https://api.github.com/repos/oven-sh/bun/releases?per_page=20")
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
                let tag = r.tag_name.clone();
                // 标签形如 "bun-v1.2.0"，需要去掉 "bun-v" 前缀
                let version = tag
                    .strip_prefix("bun-v")
                    .or_else(|| tag.strip_prefix('v'))
                    .unwrap_or(&tag)
                    .to_string();

                let url = format!(
                    "https://github.com/oven-sh/bun/releases/download/{}/{}",
                    r.tag_name, asset_name
                );

                RuntimeVersion {
                    runtime_type: RuntimeType::Bun,
                    version,
                    display_name: format!("Bun {}", tag),
                    url,
                    lts: None,
                    is_stable: true,
                    release_date: r.published_at,
                    file_size: None,
                }
            })
            .collect();

        // 排序：新版本在前（逐段数值比较）
        sort_versions_desc(&mut versions);

        // API 没有返回任何版本时使用内置回退列表
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
            .map(|v| RuntimeVersion {
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
            })
            .collect()
    }
}
