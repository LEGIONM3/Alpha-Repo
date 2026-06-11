//! Thread-safe `ModelProvider` wrapper for `ModelRouter`.
//!
//! `ModelRouter` contains `Aris` which holds a `Mutex<rusqlite::Connection>`.
//! The `ModelProvider` trait requires `Send + Sync` because
//! `ConversationService` shares the provider via `Arc<M>` across
//! `tokio::spawn` boundaries.
//!
//! `SyncModelProvider` wraps `ModelRouter` behind a `tokio::sync::Mutex`
//! to provide `Sync` safely. This serializes access to the underlying
//! router, which is acceptable since Ollama is a single-server backend.

use alpha_common::error::AlphaError;
use alpha_conversation::ModelProvider;
use alpha_model_router::types::{ChatMessage, ChatOptions, ChatResponse, ChatStream};
use alpha_model_router::ModelRouter;

/// Thread-safe wrapper around `ModelRouter` that implements `ModelProvider`.
///
/// Uses `tokio::sync::Mutex` to provide `Sync` for `Arc<SyncModelProvider>`.
pub struct SyncModelProvider {
    inner: tokio::sync::Mutex<ModelRouter>,
    /// Cached model name (immutable after construction).
    default_model_name: String,
}

impl SyncModelProvider {
    /// Wrap a `ModelRouter` in a thread-safe provider.
    pub fn new(router: ModelRouter) -> Self {
        let default_model_name = router.default_model().to_string();
        Self {
            inner: tokio::sync::Mutex::new(router),
            default_model_name,
        }
    }
}

impl ModelProvider for SyncModelProvider {
    async fn chat(
        &self,
        messages: &[ChatMessage],
        options: &ChatOptions,
    ) -> Result<ChatResponse, AlphaError> {
        let router = self.inner.lock().await;
        router.chat(messages, options).await
    }

    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        options: &ChatOptions,
    ) -> Result<ChatStream, AlphaError> {
        let router = self.inner.lock().await;
        // chat_stream returns an owned ChatStream that is independent of
        // the router after creation — the lock is released when this
        // future resolves and `router` (the MutexGuard) drops.
        router.chat_stream(messages, options).await
    }

    async fn embed_single(&self, text: &str) -> Result<Vec<f32>, AlphaError> {
        let router = self.inner.lock().await;
        router.embed_single(text).await
    }

    fn default_model(&self) -> &str {
        &self.default_model_name
    }
}
