// ── Version Registry: Source Trait, Registry, and Caching ──

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::environment::RuntimeType;
use crate::error::Result;

use super::sources;

/// A version available for download with metadata.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RuntimeVersion {
    pub runtime_type: RuntimeType,
    pub version: String,
    pub display_name: String,
    pub url: String,
    pub lts: Option<String>,
    pub is_stable: bool,
    pub release_date: Option<String>,
    pub file_size: Option<u64>,
}

/// Trait implemented by each runtime version source.
/// Each source knows how to fetch available versions for a specific runtime type.
#[async_trait]
pub trait VersionSource: Send + Sync {
    /// Fetch available versions from the remote API.
    async fn fetch_versions(&self) -> Result<Vec<RuntimeVersion>>;
    /// Return the runtime type this source provides versions for.
    fn runtime_type(&self) -> RuntimeType;
}

/// Cached versions with TTL tracking.
#[derive(Debug, Clone)]
pub struct CachedVersions {
    pub fetched_at: Instant,
    pub versions: Vec<RuntimeVersion>,
    pub ttl_secs: u64,
}

/// Central registry that manages version sources and provides cached access.
pub struct RuntimeRegistry {
    sources: HashMap<RuntimeType, Box<dyn VersionSource>>,
    cache: Arc<Mutex<HashMap<RuntimeType, CachedVersions>>>,
}

impl RuntimeRegistry {
    /// Create a new registry with all built-in version sources registered.
    pub fn new() -> Self {
        let mut sources: HashMap<RuntimeType, Box<dyn VersionSource>> = HashMap::new();

        sources.insert(RuntimeType::Node, Box::new(sources::node::NodeVersionSource));
        sources.insert(RuntimeType::Python, Box::new(sources::python::PythonVersionSource));
        sources.insert(RuntimeType::Go, Box::new(sources::go::GoVersionSource));
        sources.insert(RuntimeType::Rust, Box::new(sources::rust::RustupVersionSource));
        sources.insert(RuntimeType::Java, Box::new(sources::java::JavaVersionSource));
        sources.insert(RuntimeType::Deno, Box::new(sources::deno::DenoVersionSource));
        sources.insert(RuntimeType::Bun, Box::new(sources::bun::BunVersionSource));
        sources.insert(RuntimeType::Ruby, Box::new(sources::ruby::RubyVersionSource));

        Self {
            sources,
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get versions for a runtime type.
    /// Uses cache if available and fresh (within TTL), otherwise fetches from the source.
    pub async fn get_versions(&self, rt: &RuntimeType) -> Result<Vec<RuntimeVersion>> {
        // Check cache first
        {
            let cache = self.cache.lock().await;
            if let Some(cached) = cache.get(rt) {
                if cached.fetched_at.elapsed().as_secs() < cached.ttl_secs {
                    return Ok(cached.versions.clone());
                }
            }
        }

        // Cache miss or expired — fetch fresh
        self.fetch_and_cache(rt).await
    }

    /// Force refresh: bypass cache and fetch from remote API.
    pub async fn force_refresh(&self, rt: &RuntimeType) -> Result<Vec<RuntimeVersion>> {
        self.fetch_and_cache(rt).await
    }

    /// Internal: fetch from source and update cache.
    async fn fetch_and_cache(&self, rt: &RuntimeType) -> Result<Vec<RuntimeVersion>> {
        let source = self.sources.get(rt).ok_or_else(|| {
            crate::error::AppError::NotFound(format!(
                "没有可用的版本源: {}",
                rt.display_name()
            ))
        })?;

        let versions = self.fetch_with_retry(source.as_ref(), rt).await?;

        // Update cache with default 1-hour TTL
        {
            let mut cache = self.cache.lock().await;
            cache.insert(
                rt.clone(),
                CachedVersions {
                    fetched_at: Instant::now(),
                    versions: versions.clone(),
                    ttl_secs: 3600, // 1 hour default
                },
            );
        }

        Ok(versions)
    }

    /// Fetch versions with retry: try once, wait 1s, retry once.
    async fn fetch_with_retry(
        &self,
        source: &dyn VersionSource,
        rt: &RuntimeType,
    ) -> Result<Vec<RuntimeVersion>> {
        let mut last_err = None;
        for attempt in 0..2 {
            match source.fetch_versions().await {
                Ok(v) => return Ok(v),
                Err(e) => {
                    log::warn!(
                        "获取 {} 版本列表失败 (尝试 {}/2): {}",
                        rt.display_name(),
                        attempt + 1,
                        e
                    );
                    last_err = Some(e);
                    if attempt == 0 {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        }
        Err(last_err.unwrap_or_else(|| {
            crate::error::AppError::Provider(
                "版本源暂时不可用，请稍后重试".to_string(),
            )
        }))
    }

    /// Check if a source exists for the given runtime type.
    pub fn has_source(&self, rt: &RuntimeType) -> bool {
        self.sources.contains_key(rt)
    }

    /// Register an additional version source (for extensibility).
    pub fn register_source(&mut self, source: Box<dyn VersionSource>) {
        let rt = source.runtime_type();
        self.sources.insert(rt, source);
    }

    /// Get the number of registered sources.
    pub fn source_count(&self) -> usize {
        self.sources.len()
    }
}
