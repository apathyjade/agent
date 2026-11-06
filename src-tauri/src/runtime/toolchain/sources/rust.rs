//! Rust 版本源。
//!
//! Rust 版本由 rustup 管理，因此这里提供一份稳定版 / nightly 工具链列表，
//! 并在可用时从官方静态归档读取最新的 channel 元数据。

use async_trait::async_trait;

use crate::core::error::Result;
use crate::runtime::toolchain::http_client::get_http_client;
use crate::runtime::toolchain::registry::{RuntimeVersion, VersionSource};
use crate::runtime::toolchain::RuntimeType;

/// Rust 版本源：返回 rustup 可安装的工具链列表。
pub struct RustupVersionSource;

#[async_trait]
impl VersionSource for RustupVersionSource {
    fn runtime_type(&self) -> RuntimeType {
        RuntimeType::Rust
    }

    async fn fetch_versions(&self) -> Result<Vec<RuntimeVersion>> {
        // 尝试从 Rust channel manifest 获取最新稳定版本
        let channel_versions = self.fetch_channel_versions().await;

        let mut versions: Vec<RuntimeVersion> = Vec::new();

        // 先加入 channel 条目（stable 与 nightly）
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

        // 若成功获取到具体版本就使用它们，否则使用回退列表
        let numbered = if let Ok(fetched) = channel_versions {
            fetched
        } else {
            // 回退：已知的近期稳定版本
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
    /// 尝试从官方 channel manifest 解析最新的稳定 Rust 版本。
    async fn fetch_channel_versions(&self) -> Result<Vec<String>> {
        let url = "https://static.rust-lang.org/dist/channel-rust-stable.toml";
        let client = get_http_client();
        let response = client
            .get(url)
            .send()
            .await
            .map_err(crate::core::error::AppError::Http)?;

        let text = response
            .text()
            .await
            .map_err(crate::core::error::AppError::Http)?;

        // 解析 TOML 中的 "pkg.rust.version"
        // 格式形如：
        // [pkg.rust]
        // version = "1.85.0"
        let mut versions = Vec::new();
        for line in text.lines() {
            if line.trim().starts_with("version") && line.contains('=') {
                if let Some(ver) = line.split('=').nth(1) {
                    let ver = ver.trim().trim_matches('"').trim();
                    if !ver.is_empty() {
                        versions.push(ver.to_string());
                        // 只需要第一个版本号
                        break;
                    }
                }
            }
        }

        if versions.is_empty() {
            return Err(crate::core::error::AppError::NotFound(
                "无法解析 Rust 稳定版本".to_string(),
            ));
        }

        Ok(versions)
    }
}
