//! Ruby 版本源。
//!
//! Ruby 没有便于解析的下载版本 JSON API，因此使用一份人工维护的稳定版本
//! 列表，下载地址来自 ruby-lang.org。
//!
//! 后续改进：可以解析 https://www.ruby-lang.org/en/downloads/releases/ 。

use async_trait::async_trait;

use crate::core::error::Result;
use crate::runtime::toolchain::registry::{RuntimeVersion, VersionSource};
use crate::runtime::toolchain::RuntimeType;

/// Ruby 版本源：返回人工维护的稳定版本列表。
pub struct RubyVersionSource;

#[async_trait]
impl VersionSource for RubyVersionSource {
    fn runtime_type(&self) -> RuntimeType {
        RuntimeType::Ruby
    }

    async fn fetch_versions(&self) -> Result<Vec<RuntimeVersion>> {
        Ok(Self::curated_versions())
    }
}

impl RubyVersionSource {
    /// 返回已知的稳定 Ruby 版本列表，源码包来自 ruby-lang.org。
    fn curated_versions() -> Vec<RuntimeVersion> {
        let known_versions = [
            ("3.4.2", "3.4"),
            ("3.3.7", "3.3"),
            ("3.2.6", "3.2"),
            ("3.1.6", "3.1"),
        ];

        known_versions
            .iter()
            .map(|(version, major_minor)| {
                let url = format!(
                    "https://cache.ruby-lang.org/pub/ruby/{}/ruby-{}.tar.gz",
                    major_minor, version
                );
                RuntimeVersion {
                    runtime_type: RuntimeType::Ruby,
                    version: (*version).to_string(),
                    display_name: format!("Ruby {}", version),
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
