//! Java 版本源。
//!
//! 通过 Adoptium API 获取可用的 JDK 版本，覆盖主要版本 8、11、17、21、23。
//! API 调用失败时回退到内置的版本列表。

use async_trait::async_trait;
use serde::Deserialize;

use crate::core::error::Result;
use crate::runtime::toolchain::http_client::get_http_client;
use crate::runtime::toolchain::platform;
use crate::runtime::toolchain::registry::{RuntimeVersion, VersionSource};
use crate::runtime::toolchain::version::sort_versions_desc;
use crate::runtime::toolchain::RuntimeType;

/// Adoptium API 的顶层响应。
#[derive(Debug, Deserialize)]
struct AdoptiumResponse {
    #[allow(dead_code)]
    count: Option<i64>,
    assets: Option<Vec<AdoptiumAsset>>,
}

/// Adoptium API 返回的单个资产条目。
#[derive(Debug, Deserialize)]
struct AdoptiumAsset {
    version: AdoptiumVersion,
    binaries: Option<Vec<AdoptiumBinary>>,
    #[allow(dead_code)]
    release_date: Option<String>,
    #[allow(dead_code)]
    release_name: Option<String>,
}

/// Adoptium 返回的版本元数据。
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

/// Adoptium 返回的二进制信息。
#[derive(Debug, Deserialize)]
struct AdoptiumBinary {
    os: Option<String>,
    architecture: Option<String>,
    package: Option<AdoptiumPackage>,
}

/// 下载包信息。
#[derive(Debug, Deserialize)]
struct AdoptiumPackage {
    #[allow(dead_code)]
    name: Option<String>,
    link: Option<String>,
}

/// Java（JDK）版本源：从 Adoptium API 拉取版本。
pub struct JavaVersionSource;

impl JavaVersionSource {
    /// 需要查询的 feature 版本。
    fn feature_versions() -> &'static [i64] {
        &[23, 21, 17, 11, 8]
    }
}

#[async_trait]
impl VersionSource for JavaVersionSource {
    fn runtime_type(&self) -> RuntimeType {
        RuntimeType::Java
    }

    async fn fetch_versions(&self) -> Result<Vec<RuntimeVersion>> {
        let os = platform::java_platform_os();

        // 逐个 feature 版本尝试从 API 获取
        let mut versions = Vec::new();

        for &feature_ver in Self::feature_versions() {
            match self.fetch_feature_version(feature_ver, os).await {
                Ok(Some(rv)) => versions.push(rv),
                Ok(None) => {} // 没有匹配的二进制，跳过
                Err(_) => {
                    // 单个 feature 版本请求失败时继续下一个
                }
            }
        }

        // API 完全没有结果时使用回退列表
        if versions.is_empty() {
            versions = Self::fallback_versions();
        }

        // 排序：新版本在前（逐段数值比较）
        sort_versions_desc(&mut versions);

        Ok(versions)
    }
}

impl JavaVersionSource {
    /// 从 Adoptium API 获取单个 feature 版本。
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
            .map_err(crate::core::error::AppError::Http)?;

        let adopt_response: AdoptiumResponse = response
            .json()
            .await
            .map_err(crate::core::error::AppError::Http)?;

        let assets = match adopt_response.assets {
            Some(a) => a,
            None => return Ok(None),
        };

        // 找到第一个匹配当前平台与架构的资产
        for asset in assets {
            let semver = match asset.version.semver {
                Some(ref s) => s.clone(),
                None => continue,
            };

            let binaries = match asset.binaries {
                Some(ref b) => b,
                None => continue,
            };

            // 找到匹配当前操作系统与架构的二进制
            let matching_binary = binaries
                .iter()
                .find(|b| b.os.as_deref() == Some(os) && b.architecture.as_deref() == Some("x64"));

            if let Some(binary) = matching_binary {
                let download_url = binary
                    .package
                    .as_ref()
                    .and_then(|p| p.link.clone())
                    .unwrap_or_default();

                // 生成形如 "JDK 21.0.7 (LTS)" 的展示名
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

    /// API 不可用时使用的内置版本列表。
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
