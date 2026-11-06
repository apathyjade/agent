//! Go 版本源。
//!
//! 通过官方 go.dev DL API 获取可用的 Go 版本，只保留稳定版本，并筛选出
//! 与当前平台匹配的下载文件。

use async_trait::async_trait;
use serde::Deserialize;

use crate::core::error::Result;
use crate::runtime::toolchain::http_client::get_http_client;
use crate::runtime::toolchain::platform;
use crate::runtime::toolchain::registry::{RuntimeVersion, VersionSource};
use crate::runtime::toolchain::version::sort_versions_desc;
use crate::runtime::toolchain::RuntimeType;

/// https://go.dev/dl/?mode=json 的原始响应条目
#[derive(Debug, Deserialize)]
struct GoDistEntry {
    version: String, // "go1.22.4"
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

/// Go 版本源：从官方 DL API 拉取稳定版本。
pub struct GoVersionSource;

impl GoVersionSource {
    /// 去掉版本号里的 "go" 前缀。
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
        let current_os = platform::go_platform_os();
        let current_arch = platform::go_platform_arch();

        let client = get_http_client();
        let response = client
            .get("https://go.dev/dl/?mode=json")
            .send()
            .await
            .map_err(crate::core::error::AppError::Http)?;

        let entries: Vec<GoDistEntry> = response
            .json()
            .await
            .map_err(crate::core::error::AppError::Http)?;

        let mut versions: Vec<RuntimeVersion> = entries
            .into_iter()
            .filter(|e| e.stable)
            .filter_map(|e| {
                let version = Self::strip_go_prefix(&e.version);

                // 找到与当前平台匹配的下载文件
                let matching_file = e
                    .files
                    .iter()
                    .find(|f| f.os == current_os && f.arch == current_arch)?;

                // 用文件名拼接下载地址
                let url = format!("https://dl.google.com/go/{}", matching_file.filename);

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

        // 排序：新版本在前（逐段数值比较）
        sort_versions_desc(&mut versions);

        Ok(versions)
    }
}
