//! Types for the conversation service.

use alpha_common::error::AlphaError;
use alpha_common::types::AlphaId;
use alpha_model_router::types::{ChatMessage, ChatOptions, ChatResponse, ChatStream};

/// Trait abstracting LLM inference and embedding generation.
///
/// `ModelRouter` implements this for production use.
/// Tests provide a mock implementation to avoid requiring a live Ollama instance.
///
///
/// Both `Send` and `Sync` are required because the service wraps the
/// provider in `Arc<M>` and passes it to `tokio::spawn` for background
/// memory writes. `ModelRouter` satisfies this once `Aris` wraps its
/// connection in a `Mutex` (handled during alpha-core integration).
pub trait ModelProvider: Send + Sync {
    /// Send a non-streaming chat completion request.
    fn chat(
        &self,
        messages: &[ChatMessage],
        options: &ChatOptions,
    ) -> impl std::future::Future<Output = Result<ChatResponse, AlphaError>> + Send;

    /// Send a streaming chat completion request.
    ///
    /// Returns a `ChatStream` — a `Pin<Box<dyn Stream<Item = Result<ChatStreamChunk, AlphaError>> + Send>>`
    /// that yields individual token chunks. The final chunk has `done == true`
    /// and contains token count and duration metadata.
    fn chat_stream(
        &self,
        messages: &[ChatMessage],
        options: &ChatOptions,
    ) -> impl std::future::Future<Output = Result<ChatStream, AlphaError>> + Send;

    /// Generate an embedding vector for a single text.
    fn embed_single(
        &self,
        text: &str,
    ) -> impl std::future::Future<Output = Result<Vec<f32>, AlphaError>> + Send;

    /// The default model name used for chat.
    fn default_model(&self) -> &str;
}

/// A user's conversation request.
#[derive(Debug, Clone)]
pub struct ConversationRequest {
    /// The user's message text.
    pub message: String,
    /// Optional session ID. If `None`, the active session is used or created.
    pub session_id: Option<AlphaId>,
}

/// Alpha's conversation response.
#[derive(Debug, Clone)]
pub struct ConversationResponse {
    /// The session this exchange belongs to.
    pub session_id: AlphaId,
    /// Alpha's response text.
    pub response: String,
    /// The model that generated the response.
    pub model: String,
    /// Number of tokens generated.
    pub tokens_used: u32,
    /// Response time in milliseconds.
    pub duration_ms: u64,
    /// IDs of memories used in context assembly.
    pub memory_ids_used: Vec<AlphaId>,
    /// IDs of relationship records used in context assembly.
    pub relationship_ids_used: Vec<AlphaId>,
}

/// Events emitted during a streaming conversation exchange.
///
/// Delivered via `tokio::sync::mpsc::UnboundedReceiver<StreamEvent>`.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// The streaming pipeline has started. Emitted once before any tokens.
    Started {
        /// The session ID for this exchange.
        session_id: AlphaId,
    },
    /// A single token chunk from the model.
    Token(String),
    /// The streaming response is complete.
    Done(ConversationResponse),
    /// An error occurred during streaming.
    Error(String),
}

// ── Session Browsing Types ──

/// Lightweight summary of a conversation session for listing.
///
/// This is a UI-friendly projection of `DialogSession` — it contains
/// only the fields needed for a session list or sidebar display.
#[derive(Debug, Clone)]
pub struct SessionSummary {
    /// Session identifier.
    pub id: AlphaId,
    /// Session title (may be empty).
    pub title: String,
    /// Current session status (active or closed).
    pub status: String,
    /// Number of turns in the session.
    pub turn_count: u32,
    /// When the session was last updated (ISO 8601).
    pub updated_at: String,
    /// When the session was created (ISO 8601).
    pub created_at: String,
}

/// Lightweight summary of a single turn for display.
///
/// Strips internal metadata (ID, session_id, metadata JSON) and presents
/// only the fields a UI needs to render a message bubble.
#[derive(Debug, Clone)]
pub struct TurnSummary {
    /// Who spoke: `"user"` or `"alpha"`.
    pub role: String,
    /// The text content of the turn.
    pub content: String,
    /// Model used for generation (empty for user turns).
    pub model_used: String,
    /// Tokens consumed (0 for user turns).
    pub tokens_used: u32,
    /// When this turn was created (ISO 8601).
    pub created_at: String,
}

/// Full session history — metadata plus all turns in chronological order.
///
/// Returned by `ConversationService::load_session()`.
#[derive(Debug, Clone)]
pub struct SessionHistory {
    /// Session identifier.
    pub session_id: AlphaId,
    /// Session title.
    pub title: String,
    /// Current session status.
    pub status: String,
    /// When the session was created (ISO 8601).
    pub created_at: String,
    /// When the session was last updated (ISO 8601).
    pub updated_at: String,
    /// All turns in chronological order.
    pub turns: Vec<TurnSummary>,
}

