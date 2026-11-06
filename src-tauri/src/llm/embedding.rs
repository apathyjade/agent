//! 共享的文本 embedding 服务。
//!
//! 由 `AppState` 持有，并注入 `MemoryManager` 与意图路由器，使 OpenAI client 只
//! 初始化一次。所有失败路径都返回 `None`；调用方降级处理，而不是让请求失败。

use std::sync::Arc;

use async_trait::async_trait;
use rig::driver::DynModel;
use rig::operation::Embedding;

const DEFAULT_EMBEDDING_MODEL: &str = "text-embedding-3-small";

/// 最小化的 embedding 边界，便于测试注入确定性的替身。
#[async_trait]
pub trait TextEmbedder: Send + Sync {
    async fn embed(&self, text: &str) -> Option<Vec<f32>>;
}

struct RigEmbedder {
    model: DynModel<Embedding>,
}

#[async_trait]
impl TextEmbedder for RigEmbedder {
    async fn embed(&self, text: &str) -> Option<Vec<f32>> {
        match self.model.embed_text(text).await {
            Ok(embedding) => Some(embedding.vec.iter().map(|&value| value as f32).collect()),
            Err(error) => {
                log::warn!("EmbeddingService: embedding request failed: {error}");
                None
            }
        }
    }
}

/// 记忆检索与意图召回共用的 embedding 边界。
pub struct EmbeddingService {
    embedder: Option<Arc<dyn TextEmbedder>>,
}

impl EmbeddingService {
    /// 构造生产环境实例。`None` 或空 key 表示服务保持不可用，此时每次
    /// `embed` 都返回 `None`。
    pub fn new(openai_api_key: Option<String>, model_id: Option<&str>) -> Self {
        let key = match openai_api_key.filter(|key| !key.is_empty()) {
            Some(key) => key,
            None => return Self { embedder: None },
        };

        let client = rig::providers::openai::OpenAI::new(key);
        let model = client
            .embedding(
                model_id
                    .filter(|id| !id.is_empty())
                    .unwrap_or(DEFAULT_EMBEDDING_MODEL),
                None,
            )
            .erase();

        log::info!(
            "EmbeddingService: initialised with model '{}'",
            model_id.unwrap_or(DEFAULT_EMBEDDING_MODEL)
        );

        Self {
            embedder: Some(Arc::new(RigEmbedder { model })),
        }
    }

    #[cfg(test)]
    pub fn with_embedder(embedder: Arc<dyn TextEmbedder>) -> Self {
        Self {
            embedder: Some(embedder),
        }
    }

    /// 服务是否可用；不可用时 `embed` 恒为 `None`，调用方无需反复重试。
    pub fn available(&self) -> bool {
        self.embedder.is_some()
    }

    /// 对一段文本做 embedding，任何失败都返回 `None`。空白文本直接跳过。
    pub async fn embed(&self, text: &str) -> Option<Vec<f32>> {
        let embedder = self.embedder.as_ref()?;
        if text.trim().is_empty() {
            return None;
        }
        embedder.embed(text).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeEmbedder {
        fail: bool,
    }

    #[async_trait]
    impl TextEmbedder for FakeEmbedder {
        async fn embed(&self, text: &str) -> Option<Vec<f32>> {
            if self.fail {
                None
            } else {
                Some(vec![text.len() as f32, 1.0])
            }
        }
    }

    #[tokio::test]
    async fn unavailable_service_returns_none() {
        let service = EmbeddingService::new(None, None);
        assert!(!service.available());
        assert_eq!(service.embed("hello").await, None);
    }

    #[tokio::test]
    async fn empty_key_is_unavailable() {
        let service = EmbeddingService::new(Some(String::new()), None);
        assert!(!service.available());
    }

    #[tokio::test]
    async fn injected_embedder_is_used() {
        let service =
            EmbeddingService::with_embedder(Arc::new(FakeEmbedder { fail: false }));
        assert!(service.available());
        assert_eq!(service.embed("hello").await, Some(vec![5.0, 1.0]));
    }

    #[tokio::test]
    async fn embedder_failure_returns_none() {
        let service = EmbeddingService::with_embedder(Arc::new(FakeEmbedder { fail: true }));
        assert_eq!(service.embed("hello").await, None);
    }

    #[tokio::test]
    async fn blank_text_is_skipped() {
        let service =
            EmbeddingService::with_embedder(Arc::new(FakeEmbedder { fail: false }));
        assert_eq!(service.embed("   ").await, None);
    }
}
