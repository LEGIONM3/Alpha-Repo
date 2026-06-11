//! # alpha-model-router
//!
//! Ollama HTTP client for Project Alpha.
//!
//! All LLM inference (chat, streaming, embeddings) and model discovery
//! flows through this crate. Sprint 2 targets a single backend (Ollama).
//!
//! ## Public API
//!
//! - [`ModelRouter::chat`] — non-streaming chat completion
//! - [`ModelRouter::chat_stream`] — streaming chat completion
//! - [`ModelRouter::embed`] — generate embeddings for multiple texts
//! - [`ModelRouter::embed_single`] — generate embedding for one text
//! - [`ModelRouter::list_models`] — list locally available models
//! - [`ModelRouter::health_check`] — check server + model availability
//!
//! ## Events Published
//!
//! - `alpha.model.inference.started` — before each chat call
//! - `alpha.model.inference.completed` — after each chat completes

pub mod types;
pub(crate) mod client;
pub(crate) mod streaming;

use std::sync::Arc;

use alpha_aris::Aris;
use alpha_common::{AlphaError, Event};
use alpha_event_bus::EventBus;
use serde_json::json;
use tracing::{debug, info, warn};

pub use types::{
    ChatMessage, ChatOptions, ChatResponse, ChatRole, ChatStream, ChatStreamChunk,
    EmbedResponse, HealthStatus, ModelInfo,
};

use client::OllamaClient;

/// Model Router — routes inference requests to Ollama.
///
/// Sprint 2: single-backend (Ollama only). No multi-model routing,
/// no fallback chains, no load balancing.
pub struct ModelRouter {
    /// Low-level Ollama HTTP client.
    ollama: OllamaClient,
    /// ARIS registry for model metadata.
    #[allow(dead_code)]
    aris: Arc<Aris>,
    /// Event bus for publishing inference events.
    event_bus: Arc<EventBus>,
    /// Default model name for chat/generate (e.g., `"llama3.1:8b"`).
    default_model: String,
    /// Model name for embeddings (e.g., `"nomic-embed-text"`).
    embedding_model: String,
}

impl ModelRouter {
    /// Create a new ModelRouter.
    ///
    /// # Arguments
    ///
    /// - `endpoint`: Ollama base URL (e.g., `"http://localhost:11434"`)
    /// - `default_model`: model name for chat/generate (e.g., `"llama3.1:8b"`)
    /// - `embedding_model`: model name for embeddings (e.g., `"nomic-embed-text"`)
    /// - `aris`: ARIS registry (for future model selection)
    /// - `event_bus`: event bus for publishing inference events
    ///
    /// # Timeouts (Sprint 2 Amendment §1)
    ///
    /// - `connect_timeout`: 5 seconds — fail fast if Ollama is unreachable
    /// - `timeout`: 120 seconds — total request timeout for large inference
    pub fn new(
        endpoint: &str,
        default_model: &str,
        embedding_model: &str,
        aris: Arc<Aris>,
        event_bus: Arc<EventBus>,
    ) -> Result<Self, AlphaError> {
        let ollama = OllamaClient::new(endpoint)?;

        info!(
            default_model = %default_model,
            embedding_model = %embedding_model,
            "ModelRouter created"
        );

        Ok(Self {
            ollama,
            aris,
            event_bus,
            default_model: default_model.to_string(),
            embedding_model: embedding_model.to_string(),
        })
    }

    /// The default chat model name.
    pub fn default_model(&self) -> &str {
        &self.default_model
    }

    /// The embedding model name.
    pub fn embedding_model(&self) -> &str {
        &self.embedding_model
    }

    /// Send a chat completion request (non-streaming).
    ///
    /// Publishes `alpha.model.inference.started` before the request and
    /// `alpha.model.inference.completed` after the response.
    ///
    /// Returns the full response content.
    pub async fn chat(
        &self,
        messages: &[ChatMessage],
        options: &ChatOptions,
    ) -> Result<ChatResponse, AlphaError> {
        let model = options
            .model
            .as_deref()
            .unwrap_or(&self.default_model);

        // Publish inference started event.
        let _ = self.event_bus.publish(Event::new(
            "alpha.model.inference.started",
            "alpha-model-router",
            json!({
                "model": model,
                "message_count": messages.len(),
                "streaming": false,
            }),
        )).await;

        debug!(model = %model, messages = messages.len(), "Starting chat");

        let result = self.ollama.chat(model, messages, options).await;

        match &result {
            Ok(resp) => {
                let _ = self.event_bus.publish(Event::new(
                    "alpha.model.inference.completed",
                    "alpha-model-router",
                    json!({
                        "model": resp.model,
                        "eval_count": resp.eval_count,
                        "total_duration_ns": resp.total_duration_ns,
                        "success": true,
                    }),
                )).await;
            }
            Err(e) => {
                warn!(error = %e, "Chat inference failed");
                let _ = self.event_bus.publish(Event::new(
                    "alpha.model.inference.completed",
                    "alpha-model-router",
                    json!({
                        "model": model,
                        "success": false,
                        "error": e.to_string(),
                    }),
                )).await;
            }
        }

        result
    }

    /// Send a chat completion request with streaming.
    ///
    /// Returns an async stream of response tokens. Each chunk contains
    /// a partial message. The final chunk has `done: true` and includes
    /// timing statistics.
    ///
    /// Publishes `alpha.model.inference.started` before the request and
    /// `alpha.model.inference.completed` when the final chunk is received.
    pub async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        options: &ChatOptions,
    ) -> Result<ChatStream, AlphaError> {
        let model = options
            .model
            .as_deref()
            .unwrap_or(&self.default_model)
            .to_string();

        // Publish inference started event.
        let _ = self.event_bus.publish(Event::new(
            "alpha.model.inference.started",
            "alpha-model-router",
            json!({
                "model": model,
                "message_count": messages.len(),
                "streaming": true,
            }),
        )).await;

        debug!(model = %model, messages = messages.len(), "Starting streaming chat");

        let inner_stream = self.ollama.chat_stream(&model, messages, options).await?;

        // Wrap stream to publish completion event on final chunk.
        let event_bus = Arc::clone(&self.event_bus);
        let model_name = model.clone();

        use futures::StreamExt;
        let wrapped = inner_stream.then(move |result| {
            let event_bus = Arc::clone(&event_bus);
            let model_name = model_name.clone();
            async move {
                if let Ok(ref chunk) = result {
                    if chunk.done {
                        let _ = event_bus.publish(Event::new(
                            "alpha.model.inference.completed",
                            "alpha-model-router",
                            json!({
                                "model": model_name,
                                "eval_count": chunk.eval_count,
                                "total_duration": chunk.total_duration,
                                "streaming": true,
                                "success": true,
                            }),
                        )).await;
                    }
                }
                result
            }
        });

        Ok(Box::pin(wrapped))
    }

    /// Generate embeddings for one or more texts.
    ///
    /// Uses the configured `embedding_model`. Returns one embedding
    /// vector per input text.
    pub async fn embed(
        &self,
        texts: &[String],
    ) -> Result<Vec<Vec<f32>>, AlphaError> {
        self.ollama.embed(&self.embedding_model, texts).await
    }

    /// Generate embedding for a single text.
    ///
    /// Convenience wrapper around [`Self::embed`].
    pub async fn embed_single(
        &self,
        text: &str,
    ) -> Result<Vec<f32>, AlphaError> {
        let results = self.embed(&[text.to_string()]).await?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| AlphaError::Other("Embed returned no vectors".to_string()))
    }

    /// Ping the Ollama server to verify reachability.
    ///
    /// Returns `Ok(())` if the server responds, `Err` otherwise.
    pub async fn ping(&self) -> Result<(), AlphaError> {
        self.ollama.ping().await
    }

    /// List models available on the Ollama server.
    ///
    /// Calls `GET /api/tags` and returns model metadata.
    pub async fn list_models(&self) -> Result<Vec<ModelInfo>, AlphaError> {
        self.ollama.list_models().await
    }

    /// Check Ollama server reachability and model availability.
    ///
    /// 1. `GET /api/tags` — if reachable, `server_reachable = true`
    /// 2. Check if `default_model` appears in the model list
    /// 3. Check if `embedding_model` appears in the model list
    ///
    /// Never returns an error — connectivity failures produce
    /// `HealthStatus::unreachable()`.
    pub async fn health_check(&self) -> HealthStatus {
        let models = match self.ollama.list_models().await {
            Ok(m) => m,
            Err(e) => {
                warn!(error = %e, "Health check: Ollama unreachable");
                return HealthStatus::unreachable();
            }
        };

        let chat_available = models
            .iter()
            .any(|m| m.name == self.default_model);
        let embed_available = models
            .iter()
            .any(|m| m.name == self.embedding_model);

        let status = HealthStatus {
            server_reachable: true,
            chat_model_available: chat_available,
            embedding_model_available: embed_available,
        };

        info!(
            server = status.server_reachable,
            chat = status.chat_model_available,
            embed = status.embedding_model_available,
            "Health check complete"
        );

        status
    }
}
