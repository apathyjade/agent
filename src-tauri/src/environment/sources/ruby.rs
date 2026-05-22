// ── Ruby Version Source ──
//
// Ruby does not have a simple JSON API for download versions.
// Uses a curated list of known stable versions with URLs from ruby-lang.org.
//
// Future improvement: could parse https://www.ruby-lang.org/en/downloads/releases/

use async_trait::async_trait;

use crate::environment::registry::{RuntimeVersion, VersionSource};
use crate::environment::RuntimeType;
use crate::error::Result;

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
    /// Returns a curated list of known stable Ruby versions.
    /// Source tarballs from ruby-lang.org.
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
