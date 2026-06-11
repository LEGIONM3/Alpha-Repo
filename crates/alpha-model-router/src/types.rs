//! Request/response types for the Ollama model router.
//!
//! These types define the public interface for chat, embedding, and
//! model discovery operations. They are independent of the HTTP client
//! implementation so they can be used for testing with mock backends.

use std::pin::Pin;

use alpha_common::AlphaError;
use futures::Stream;
use serde::{Deserialize, Serialize};

// ── Chat Types ──

/// A single message in a chat conversation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    /// The role of this message's author.
    pub role: ChatRole,
    /// The text content of the message.
    pub content: String,
}

impl ChatMessage {
    /// Create a system message.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::System,
            content: content.into(),
        }
    }

    /// Create a user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::User,
            content: content.into(),
        }
    }

    /// Create an assistant message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::Assistant,
            content: content.into(),
        }
    }
}

/// Chat participant role.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    /// System prompt / instructions.
    System,
    /// Human user input.
    User,
    /// AI assistant response.
    Assistant,
}

/// Options for a chat request.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ChatOptions {
    /// Override the model to use (`None` = use `default_model`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// System prompt (prepended to messages if set).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// Temperature (0.0 = deterministic, higher = more creative).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Max tokens to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_predict: Option<i32>,
    /// Response format: `None` = text, `Some("json")` = JSON mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// How long to keep model loaded (default: `"5m"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_alive: Option<String>,
}

/// Full chat response (non-streaming).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    /// Model that generated this response.
    pub model: String,
    /// The assistant's reply message.
    pub message: ChatMessage,
    /// Whether generation is complete.
    pub done: bool,
    /// Total duration in nanoseconds (present in final response).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_duration_ns: Option<u64>,
    /// Number of tokens in the prompt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_eval_count: Option<u32>,
    /// Number of tokens generated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eval_count: Option<u32>,
}

/// A single token chunk from a streaming chat response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatStreamChunk {
    /// Model that generated this chunk.
    pub model: String,
    /// Partial message (typically one token in `content`).
    pub message: ChatMessage,
    /// Whether this is the final chunk.
    pub done: bool,
    /// Total duration in nanoseconds (present only in the final chunk).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_duration: Option<u64>,
    /// Number of tokens in the prompt (present only in the final chunk).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_eval_count: Option<u32>,
    /// Number of tokens generated (present only in the final chunk).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eval_count: Option<u32>,
}

/// A streaming chat response — an async stream of token chunks.
pub type ChatStream =
    Pin<Box<dyn Stream<Item = Result<ChatStreamChunk, AlphaError>> + Send>>;

// ── Embedding Types ──

/// Embedding response from Ollama `/api/embed`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbedResponse {
    /// Model used for embedding.
    pub model: String,
    /// One embedding vector per input text.
    pub embeddings: Vec<Vec<f32>>,
}

// ── Model Discovery Types ──

/// Information about a locally available model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelInfo {
    /// Model name (e.g., `"llama3.1:8b"`).
    pub name: String,
    /// Model size in bytes.
    pub size: u64,
    /// Model digest (SHA256).
    pub digest: String,
    /// When the model was last modified (ISO 8601).
    pub modified_at: String,
}

// ── Health Check Types ──

/// Health status of the Ollama backend.
///
/// Reports server reachability and availability of the configured
/// chat and embedding models.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HealthStatus {
    /// Whether the Ollama HTTP server is reachable.
    pub server_reachable: bool,
    /// Whether the configured chat model is available locally.
    pub chat_model_available: bool,
    /// Whether the configured embedding model is available locally.
    pub embedding_model_available: bool,
}

impl HealthStatus {
    /// All systems go.
    pub fn all_healthy() -> Self {
        Self {
            server_reachable: true,
            chat_model_available: true,
            embedding_model_available: true,
        }
    }

    /// Server unreachable — everything is unavailable.
    pub fn unreachable() -> Self {
        Self {
            server_reachable: false,
            chat_model_available: false,
            embedding_model_available: false,
        }
    }
}

// ── Ollama Internal Request Types ──
// These are used by the HTTP client to build Ollama API requests.

/// Internal: Ollama `/api/chat` request body.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct OllamaChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<OllamaOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_alive: Option<String>,
}

/// Internal: Ollama model runtime options.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_predict: Option<i32>,
}

/// Internal: Ollama `/api/embed` request body.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct OllamaEmbedRequest {
    pub model: String,
    pub input: Vec<String>,
}

/// Internal: Ollama `/api/chat` response body (non-streaming).
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct OllamaChatResponse {
    pub model: String,
    pub message: ChatMessage,
    pub done: bool,
    pub total_duration: Option<u64>,
    pub prompt_eval_count: Option<u32>,
    pub eval_count: Option<u32>,
}

/// Internal: Ollama `/api/tags` response body.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct OllamaTagsResponse {
    pub models: Vec<ModelInfo>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_message_roundtrip() {
        let msg = ChatMessage::user("Hello, Alpha!");
        let json = serde_json::to_string(&msg).expect("serialize");
        let deserialized: ChatMessage = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(msg, deserialized);
        assert_eq!(deserialized.role, ChatRole::User);
        assert_eq!(deserialized.content, "Hello, Alpha!");

        // Verify role serialization format.
        assert!(json.contains("\"user\""), "role should serialize as lowercase");
    }

    #[test]
    fn test_chat_message_roles() {
        let system = ChatMessage::system("You are Alpha.");
        assert_eq!(system.role, ChatRole::System);

        let user = ChatMessage::user("Hi");
        assert_eq!(user.role, ChatRole::User);

        let assistant = ChatMessage::assistant("Hello!");
        assert_eq!(assistant.role, ChatRole::Assistant);
    }

    #[test]
    fn test_chat_role_serialization() {
        let roles = [
            (ChatRole::System, "\"system\""),
            (ChatRole::User, "\"user\""),
            (ChatRole::Assistant, "\"assistant\""),
        ];
        for (role, expected) in &roles {
            let json = serde_json::to_string(role).expect("serialize");
            assert_eq!(&json, expected, "ChatRole::{role:?} should serialize to {expected}");
        }
    }

    #[test]
    fn test_chat_options_roundtrip() {
        let opts = ChatOptions {
            model: Some("llama3.1:8b".to_string()),
            system: Some("You are Alpha.".to_string()),
            temperature: Some(0.7),
            num_predict: Some(256),
            format: Some("json".to_string()),
            keep_alive: Some("10m".to_string()),
        };
        let json = serde_json::to_string(&opts).expect("serialize");
        let deserialized: ChatOptions = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(opts, deserialized);
    }

    #[test]
    fn test_chat_options_default_is_empty() {
        let opts = ChatOptions::default();
        let json = serde_json::to_string(&opts).expect("serialize");
        // All fields are None → should serialize to empty object.
        assert_eq!(json, "{}");
    }

    #[test]
    fn test_chat_options_skip_none_fields() {
        let opts = ChatOptions {
            temperature: Some(0.5),
            ..Default::default()
        };
        let json = serde_json::to_string(&opts).expect("serialize");
        assert!(json.contains("temperature"));
        assert!(!json.contains("model"));
        assert!(!json.contains("system"));
        assert!(!json.contains("num_predict"));
    }

    #[test]
    fn test_health_status_roundtrip() {
        let status = HealthStatus {
            server_reachable: true,
            chat_model_available: true,
            embedding_model_available: false,
        };
        let json = serde_json::to_string(&status).expect("serialize");
        let deserialized: HealthStatus = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(status, deserialized);
    }

    #[test]
    fn test_health_status_constructors() {
        let healthy = HealthStatus::all_healthy();
        assert!(healthy.server_reachable);
        assert!(healthy.chat_model_available);
        assert!(healthy.embedding_model_available);

        let down = HealthStatus::unreachable();
        assert!(!down.server_reachable);
        assert!(!down.chat_model_available);
        assert!(!down.embedding_model_available);
    }

    #[test]
    fn test_model_info_roundtrip() {
        let info = ModelInfo {
            name: "llama3.1:8b".to_string(),
            size: 4_500_000_000,
            digest: "sha256:abc123def456".to_string(),
            modified_at: "2025-01-15T10:30:00Z".to_string(),
        };
        let json = serde_json::to_string(&info).expect("serialize");
        let deserialized: ModelInfo = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(info, deserialized);
    }

    #[test]
    fn test_embed_response_deserialization() {
        let json = r#"{
            "model": "nomic-embed-text",
            "embeddings": [[0.1, 0.2, 0.3], [0.4, 0.5, 0.6]]
        }"#;
        let resp: EmbedResponse = serde_json::from_str(json).expect("deserialize");
        assert_eq!(resp.model, "nomic-embed-text");
        assert_eq!(resp.embeddings.len(), 2);
        assert_eq!(resp.embeddings[0].len(), 3);
    }

    #[test]
    fn test_chat_stream_chunk_deserialization() {
        // Non-final chunk.
        let json = r#"{"model":"llama3.1:8b","message":{"role":"assistant","content":"The"},"done":false}"#;
        let chunk: ChatStreamChunk = serde_json::from_str(json).expect("deserialize");
        assert_eq!(chunk.model, "llama3.1:8b");
        assert_eq!(chunk.message.content, "The");
        assert!(!chunk.done);
        assert!(chunk.total_duration.is_none());

        // Final chunk.
        let json = r#"{"model":"llama3.1:8b","message":{"role":"assistant","content":""},"done":true,"total_duration":5000000000,"prompt_eval_count":26,"eval_count":150}"#;
        let chunk: ChatStreamChunk = serde_json::from_str(json).expect("deserialize");
        assert!(chunk.done);
        assert_eq!(chunk.total_duration, Some(5_000_000_000));
        assert_eq!(chunk.eval_count, Some(150));
    }

    #[test]
    fn test_ollama_chat_request_serialization() {
        let req = OllamaChatRequest {
            model: "llama3.1:8b".to_string(),
            messages: vec![
                ChatMessage::system("You are Alpha."),
                ChatMessage::user("Hello!"),
            ],
            stream: false,
            format: None,
            options: Some(OllamaOptions {
                temperature: Some(0.7),
                num_predict: None,
            }),
            keep_alive: None,
        };
        let json = serde_json::to_string(&req).expect("serialize");
        assert!(json.contains("\"llama3.1:8b\""));
        assert!(json.contains("\"system\""));
        assert!(json.contains("\"user\""));
        assert!(!json.contains("format"));
        assert!(!json.contains("keep_alive"));
        assert!(json.contains("\"temperature\":0.7"));
        assert!(!json.contains("num_predict"));
    }

    #[test]
    fn test_ollama_tags_response_deserialization() {
        let json = r#"{
            "models": [
                {"name": "llama3.1:8b", "size": 4500000000, "digest": "abc123", "modified_at": "2025-01-01T00:00:00Z"},
                {"name": "nomic-embed-text", "size": 274000000, "digest": "def456", "modified_at": "2025-01-01T00:00:00Z"}
            ]
        }"#;
        let resp: OllamaTagsResponse = serde_json::from_str(json).expect("deserialize");
        assert_eq!(resp.models.len(), 2);
        assert_eq!(resp.models[0].name, "llama3.1:8b");
        assert_eq!(resp.models[1].name, "nomic-embed-text");
    }
}
