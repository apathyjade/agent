//! Shared text-embedding service.
//!
//! Owned by `AppState` and injected into both `MemoryManager` and the intent
//! router so the OpenAI client is initialised once. Every failure path returns
//! `None`; callers degrade instead of failing the request.

use std::sync::Arc;

use async_trait::async_trait;
use rig::driver::DynModel;
use rig::operation::Embedding;

const DEFAULT_EMBEDDING_MODEL: &str = "text-embedding-3-small";

/// Minimal embedding boundary, so tests can inject a deterministic fake.
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

/// Shared embedding boundary used by memory retrieval and intent recall.
pub struct EmbeddingService {
    embedder: Option<Arc<dyn TextEmbedder>>,
}

impl EmbeddingService {
    /// Build the production service. `None` or an empty key means the service
    /// stays unavailable and every `embed` returns `None`.
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

    pub fn available(&self) -> bool {
        self.embedder.is_some()
    }

    /// Embed one text, returning `None` on any failure.
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
