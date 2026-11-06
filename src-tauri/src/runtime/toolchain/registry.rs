//! 版本注册表：版本源 trait、注册表与缓存。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::core::error::Result;
use crate::runtime::toolchain::RuntimeType;

use super::sources;

/// 带元数据的可下载版本。
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

/// 每个运行时版本源都要实现的 trait。
/// 每个源负责获取某一运行时的可用版本列表。
#[async_trait]
pub trait VersionSource: Send + Sync {
    /// 从远端 API 获取可用版本。
    ///
    /// 这是网络操作；具体源可在失败时返回内置回退列表，调用方不应假设一定
    /// 访问了网络。
    async fn fetch_versions(&self) -> Result<Vec<RuntimeVersion>>;
    /// 返回该源提供版本的运行时类型。
    fn runtime_type(&self) -> RuntimeType;
}

/// 带 TTL 记录的版本缓存。
#[derive(Debug, Clone)]
pub struct CachedVersions {
    pub fetched_at: Instant,
    pub versions: Vec<RuntimeVersion>,
    pub ttl_secs: u64,
}

/// 管理版本源并提供缓存访问的中心注册表。
pub struct RuntimeRegistry {
    sources: HashMap<RuntimeType, Box<dyn VersionSource>>,
    cache: Arc<Mutex<HashMap<RuntimeType, CachedVersions>>>,
}

impl Default for RuntimeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeRegistry {
    /// 创建注册表并注册所有内置版本源。
    pub fn new() -> Self {
        let mut sources: HashMap<RuntimeType, Box<dyn VersionSource>> = HashMap::new();

        sources.insert(
            RuntimeType::Node,
            Box::new(sources::node::NodeVersionSource::new()),
        );
        sources.insert(
            RuntimeType::Python,
            Box::new(sources::python::PythonVersionSource),
        );
        sources.insert(RuntimeType::Go, Box::new(sources::go::GoVersionSource));
        sources.insert(
            RuntimeType::Rust,
            Box::new(sources::rust::RustupVersionSource),
        );
        sources.insert(
            RuntimeType::Java,
            Box::new(sources::java::JavaVersionSource),
        );
        sources.insert(
            RuntimeType::Deno,
            Box::new(sources::deno::DenoVersionSource),
        );
        sources.insert(RuntimeType::Bun, Box::new(sources::bun::BunVersionSource));
        sources.insert(
            RuntimeType::Ruby,
            Box::new(sources::ruby::RubyVersionSource),
        );

        Self {
            sources,
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 获取某个运行时的版本列表。
    ///
    /// 缓存未过期（在 TTL 内）时直接返回缓存副本，否则从版本源重新拉取。
    /// 返回的是克隆结果，调用方修改不会影响缓存。
    pub async fn get_versions(&self, rt: &RuntimeType) -> Result<Vec<RuntimeVersion>> {
        // 先查缓存
        {
            let cache = self.cache.lock().await;
            if let Some(cached) = cache.get(rt) {
                if cached.fetched_at.elapsed().as_secs() < cached.ttl_secs {
                    return Ok(cached.versions.clone());
                }
            }
        }

        // 缓存未命中或已过期 —— 重新拉取
        self.fetch_and_cache(rt).await
    }

    /// 强制刷新：绕过缓存，直接从远端 API 拉取。
    pub async fn force_refresh(&self, rt: &RuntimeType) -> Result<Vec<RuntimeVersion>> {
        self.fetch_and_cache(rt).await
    }

    /// 内部：从版本源拉取并更新缓存。
    async fn fetch_and_cache(&self, rt: &RuntimeType) -> Result<Vec<RuntimeVersion>> {
        let source = self.sources.get(rt).ok_or_else(|| {
            crate::core::error::AppError::NotFound(format!("没有可用的版本源: {}", rt.display_name()))
        })?;

        let versions = self.fetch_with_retry(source.as_ref(), rt).await?;

        // 更新缓存，默认 TTL 为 1 小时
        {
            let mut cache = self.cache.lock().await;
            cache.insert(
                rt.clone(),
                CachedVersions {
                    fetched_at: Instant::now(),
                    versions: versions.clone(),
                    ttl_secs: 3600, // 默认 1 小时
                },
            );
        }

        Ok(versions)
    }

    /// 带重试的拉取：先试一次，等待 1 秒后再试一次。
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
            crate::core::error::AppError::Provider("版本源暂时不可用，请稍后重试".to_string())
        }))
    }

    /// 检查某个运行时是否有对应的版本源。
    pub fn has_source(&self, rt: &RuntimeType) -> bool {
        self.sources.contains_key(rt)
    }

    /// 注册额外的版本源（用于扩展）。
    pub fn register_source(&mut self, source: Box<dyn VersionSource>) {
        let rt = source.runtime_type();
        self.sources.insert(rt, source);
    }

    /// 获取已注册版本源的数量。
    pub fn source_count(&self) -> usize {
        self.sources.len()
    }
}
