//! Low-level Ollama HTTP client.
//!
//! This module contains the HTTP request/response logic for communicating
//! with an Ollama server. It is consumed by [`crate::ModelRouter`] and
//! not exposed in the public API.
//!
//! ## Endpoints
//!
//! | Method | Path          | Purpose                     |
//! |--------|---------------|-----------------------------|
//! | GET    | `/api/tags`   | List local models           |
//! | POST   | `/api/chat`   | Non-streaming / streaming   |
//! | POST   | `/api/embed`  | Generate embeddings         |

use std::time::Duration;

use alpha_common::AlphaError;
use tracing::{debug, info, warn};

use crate::streaming::parse_chat_stream;
use crate::types::{
    ChatMessage, ChatOptions, ChatResponse, ChatStream, EmbedResponse, ModelInfo,
    OllamaChatRequest, OllamaChatResponse, OllamaEmbedRequest, OllamaOptions,
    OllamaTagsResponse,
};

/// Default connect timeout (Sprint 2 Amendment §1).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Default request timeout (Sprint 2 Amendment §1).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// Low-level HTTP client for the Ollama REST API.
///
/// Handles request construction, serialization, HTTP calls, response
/// deserialization, and error mapping. All methods return `AlphaError`
/// on failure.
pub(crate) struct OllamaClient {
    client: reqwest::Client,
    base_url: String,
}

impl OllamaClient {
    /// Create a new client for the given Ollama endpoint.
    ///
    /// Timeouts per Sprint 2 Amendment §1:
    /// - `connect_timeout`: 5 seconds
    /// - `timeout`: 120 seconds
    pub fn new(base_url: &str) -> Result<Self, AlphaError> {
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|e| AlphaError::Other(format!("Failed to build HTTP client: {e}")))?;

        let base_url = base_url.trim_end_matches('/').to_string();
        info!(url = %base_url, "OllamaClient created");

        Ok(Self { client, base_url })
    }

    /// Create a client with a pre-built `reqwest::Client` (for testing).
    #[cfg(test)]
    pub fn with_client(client: reqwest::Client, base_url: &str) -> Self {
        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// Ping the Ollama server to check reachability.
    ///
    /// Uses `GET /api/tags` as the health probe. Returns `Ok(())` if the
    /// server responds with a 2xx status, or an `AlphaError` otherwise.
    pub async fn ping(&self) -> Result<(), AlphaError> {
        let url = format!("{}/api/tags", self.base_url);
        debug!(url = %url, "Pinging Ollama server");

        let resp = self.client.get(&url).send().await.map_err(|e| {
            warn!(error = %e, "Ollama server unreachable");
            AlphaError::Other(format!("Ollama unreachable: {e}"))
        })?;

        if resp.status().is_success() {
            debug!("Ollama server is reachable");
            Ok(())
        } else {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            warn!(status = %status, body = %body, "Ollama ping failed");
            Err(AlphaError::Other(format!(
                "Ollama ping returned HTTP {status}: {body}"
            )))
        }
    }

    /// List locally available models.
    ///
    /// `GET /api/tags` → `OllamaTagsResponse`
    pub async fn list_models(&self) -> Result<Vec<ModelInfo>, AlphaError> {
        let url = format!("{}/api/tags", self.base_url);
        debug!(url = %url, "Listing Ollama models");

        let resp = self.client.get(&url).send().await.map_err(|e| {
            AlphaError::Other(format!("Failed to list models: {e}"))
        })?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AlphaError::Other(format!(
                "list_models returned HTTP {status}: {body}"
            )));
        }

        let tags: OllamaTagsResponse = resp.json().await.map_err(|e| {
            AlphaError::Other(format!("Failed to parse tags response: {e}"))
        })?;

        info!(count = tags.models.len(), "Models listed");
        Ok(tags.models)
    }

    /// Send a non-streaming chat completion request.
    ///
    /// `POST /api/chat` with `stream: false`
    ///
    /// Builds the request from `ChatMessage` slice and `ChatOptions`,
    /// sends it, and returns a typed `ChatResponse`.
    pub async fn chat(
        &self,
        model: &str,
        messages: &[ChatMessage],
        options: &ChatOptions,
    ) -> Result<ChatResponse, AlphaError> {
        let url = format!("{}/api/chat", self.base_url);

        // Build Ollama-native options if any are set.
        let ollama_options = if options.temperature.is_some() || options.num_predict.is_some() {
            Some(OllamaOptions {
                temperature: options.temperature,
                num_predict: options.num_predict,
            })
        } else {
            None
        };

        // Prepend system message if provided in options.
        let mut all_messages = Vec::with_capacity(messages.len() + 1);
        if let Some(ref system) = options.system {
            all_messages.push(ChatMessage::system(system.as_str()));
        }
        all_messages.extend_from_slice(messages);

        let request = OllamaChatRequest {
            model: model.to_string(),
            messages: all_messages,
            stream: false,
            format: options.format.clone(),
            options: ollama_options,
            keep_alive: options.keep_alive.clone(),
        };

        debug!(
            model = %request.model,
            message_count = request.messages.len(),
            "Sending chat request"
        );

        let resp = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| AlphaError::Other(format!("Chat request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AlphaError::Other(format!(
                "Chat returned HTTP {status}: {body}"
            )));
        }

        let ollama_resp: OllamaChatResponse = resp.json().await.map_err(|e| {
            AlphaError::Other(format!("Failed to parse chat response: {e}"))
        })?;

        debug!(
            model = %ollama_resp.model,
            done = ollama_resp.done,
            eval_count = ?ollama_resp.eval_count,
            "Chat response received"
        );

        Ok(ChatResponse {
            model: ollama_resp.model,
            message: ollama_resp.message,
            done: ollama_resp.done,
            total_duration_ns: ollama_resp.total_duration,
            prompt_eval_count: ollama_resp.prompt_eval_count,
            eval_count: ollama_resp.eval_count,
        })
    }

    /// Generate embeddings for one or more texts.
    ///
    /// `POST /api/embed`
    ///
    /// Returns one embedding vector per input text.
    pub async fn embed(
        &self,
        model: &str,
        texts: &[String],
    ) -> Result<Vec<Vec<f32>>, AlphaError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let url = format!("{}/api/embed", self.base_url);

        let request = OllamaEmbedRequest {
            model: model.to_string(),
            input: texts.to_vec(),
        };

        debug!(
            model = %request.model,
            input_count = request.input.len(),
            "Sending embed request"
        );

        let resp = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| AlphaError::Other(format!("Embed request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AlphaError::Other(format!(
                "Embed returned HTTP {status}: {body}"
            )));
        }

        let embed_resp: EmbedResponse = resp.json().await.map_err(|e| {
            AlphaError::Other(format!("Failed to parse embed response: {e}"))
        })?;

        info!(
            model = %embed_resp.model,
            vectors = embed_resp.embeddings.len(),
            dimension = embed_resp.embeddings.first().map(|v| v.len()).unwrap_or(0),
            "Embeddings generated"
        );

        Ok(embed_resp.embeddings)
    }

    /// Send a streaming chat completion request.
    ///
    /// `POST /api/chat` with `stream: true`
    ///
    /// Returns a `ChatStream` that yields `ChatStreamChunk` items parsed
    /// from the newline-delimited JSON response body.
    pub async fn chat_stream(
        &self,
        model: &str,
        messages: &[ChatMessage],
        options: &ChatOptions,
    ) -> Result<ChatStream, AlphaError> {
        let url = format!("{}/api/chat", self.base_url);

        let ollama_options = if options.temperature.is_some() || options.num_predict.is_some() {
            Some(OllamaOptions {
                temperature: options.temperature,
                num_predict: options.num_predict,
            })
        } else {
            None
        };

        let mut all_messages = Vec::with_capacity(messages.len() + 1);
        if let Some(ref system) = options.system {
            all_messages.push(ChatMessage::system(system.as_str()));
        }
        all_messages.extend_from_slice(messages);

        let request = OllamaChatRequest {
            model: model.to_string(),
            messages: all_messages,
            stream: true,
            format: options.format.clone(),
            options: ollama_options,
            keep_alive: options.keep_alive.clone(),
        };

        debug!(
            model = %request.model,
            message_count = request.messages.len(),
            "Sending streaming chat request"
        );

        let resp = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| AlphaError::Other(format!("Streaming chat request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(AlphaError::Other(format!(
                "Streaming chat returned HTTP {status}: {body}"
            )));
        }

        let byte_stream = resp.bytes_stream();
        Ok(parse_chat_stream(byte_stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── Request Construction ──

    #[test]
    fn test_chat_request_construction() {
        let messages = vec![
            ChatMessage::system("You are Alpha."),
            ChatMessage::user("Hello!"),
        ];

        let options = ChatOptions {
            temperature: Some(0.7),
            num_predict: Some(256),
            format: Some("json".to_string()),
            keep_alive: Some("10m".to_string()),
            ..Default::default()
        };

        let ollama_options = OllamaOptions {
            temperature: options.temperature,
            num_predict: options.num_predict,
        };

        let request = OllamaChatRequest {
            model: "llama3.1:8b".to_string(),
            messages: messages.clone(),
            stream: false,
            format: options.format.clone(),
            options: Some(ollama_options),
            keep_alive: options.keep_alive.clone(),
        };

        let json = serde_json::to_value(&request).expect("serialize");

        assert_eq!(json["model"], "llama3.1:8b");
        assert_eq!(json["stream"], false);
        assert_eq!(json["messages"].as_array().unwrap().len(), 2);
        assert_eq!(json["messages"][0]["role"], "system");
        assert_eq!(json["messages"][1]["role"], "user");
        assert_eq!(json["format"], "json");
        let temp = json["options"]["temperature"].as_f64().unwrap();
        assert!((temp - 0.7).abs() < 0.001, "temperature mismatch: {temp}");
        assert_eq!(json["options"]["num_predict"], 256);
        assert_eq!(json["keep_alive"], "10m");
    }

    #[test]
    fn test_chat_request_minimal() {
        let request = OllamaChatRequest {
            model: "llama3.1:8b".to_string(),
            messages: vec![ChatMessage::user("Hi")],
            stream: false,
            format: None,
            options: None,
            keep_alive: None,
        };

        let json = serde_json::to_value(&request).expect("serialize");
        assert_eq!(json["model"], "llama3.1:8b");
        assert_eq!(json["stream"], false);
        // None fields should be skipped.
        assert!(json.get("format").is_none());
        assert!(json.get("options").is_none());
        assert!(json.get("keep_alive").is_none());
    }

    #[test]
    fn test_embed_request_construction() {
        let request = OllamaEmbedRequest {
            model: "nomic-embed-text".to_string(),
            input: vec!["Hello world".to_string(), "Goodbye world".to_string()],
        };

        let json = serde_json::to_value(&request).expect("serialize");
        assert_eq!(json["model"], "nomic-embed-text");
        assert_eq!(json["input"].as_array().unwrap().len(), 2);
        assert_eq!(json["input"][0], "Hello world");
    }

    // ── Response Parsing ──

    #[test]
    fn test_chat_response_parsing() {
        let json = json!({
            "model": "llama3.1:8b",
            "message": {
                "role": "assistant",
                "content": "Hello! How can I help you today?"
            },
            "done": true,
            "total_duration": 5_000_000_000_u64,
            "prompt_eval_count": 26,
            "eval_count": 150
        });

        let resp: OllamaChatResponse =
            serde_json::from_value(json).expect("deserialize");

        assert_eq!(resp.model, "llama3.1:8b");
        assert_eq!(resp.message.content, "Hello! How can I help you today?");
        assert!(resp.done);
        assert_eq!(resp.total_duration, Some(5_000_000_000));
        assert_eq!(resp.prompt_eval_count, Some(26));
        assert_eq!(resp.eval_count, Some(150));
    }

    #[test]
    fn test_chat_response_minimal_fields() {
        // Ollama sometimes omits optional timing fields.
        let json = json!({
            "model": "llama3.1:8b",
            "message": {
                "role": "assistant",
                "content": "Sure!"
            },
            "done": true
        });

        let resp: OllamaChatResponse =
            serde_json::from_value(json).expect("deserialize");

        assert_eq!(resp.model, "llama3.1:8b");
        assert!(resp.done);
        assert!(resp.total_duration.is_none());
        assert!(resp.prompt_eval_count.is_none());
        assert!(resp.eval_count.is_none());
    }

    // ── Tags Parsing ──

    #[test]
    fn test_tags_response_parsing() {
        let json = json!({
            "models": [
                {
                    "name": "llama3.1:8b",
                    "size": 4_500_000_000_u64,
                    "digest": "sha256:abc123",
                    "modified_at": "2025-01-15T10:30:00Z"
                },
                {
                    "name": "nomic-embed-text",
                    "size": 274_000_000_u64,
                    "digest": "sha256:def456",
                    "modified_at": "2025-01-10T08:00:00Z"
                }
            ]
        });

        let resp: OllamaTagsResponse =
            serde_json::from_value(json).expect("deserialize");

        assert_eq!(resp.models.len(), 2);
        assert_eq!(resp.models[0].name, "llama3.1:8b");
        assert_eq!(resp.models[0].size, 4_500_000_000);
        assert_eq!(resp.models[1].name, "nomic-embed-text");
    }

    #[test]
    fn test_tags_response_empty() {
        let json = json!({ "models": [] });
        let resp: OllamaTagsResponse =
            serde_json::from_value(json).expect("deserialize");
        assert!(resp.models.is_empty());
    }

    // ── Embed Parsing ──

    #[test]
    fn test_embed_response_parsing() {
        let json = json!({
            "model": "nomic-embed-text",
            "embeddings": [
                [0.1, 0.2, 0.3, 0.4],
                [0.5, 0.6, 0.7, 0.8]
            ]
        });

        let resp: EmbedResponse =
            serde_json::from_value(json).expect("deserialize");

        assert_eq!(resp.model, "nomic-embed-text");
        assert_eq!(resp.embeddings.len(), 2);
        assert_eq!(resp.embeddings[0].len(), 4);
        assert!((resp.embeddings[0][0] - 0.1).abs() < 0.001);
        assert!((resp.embeddings[1][3] - 0.8).abs() < f32::EPSILON);
    }

    #[test]
    fn test_embed_response_single_input() {
        let json = json!({
            "model": "nomic-embed-text",
            "embeddings": [[0.1, 0.2, 0.3]]
        });

        let resp: EmbedResponse =
            serde_json::from_value(json).expect("deserialize");

        assert_eq!(resp.embeddings.len(), 1);
        assert_eq!(resp.embeddings[0].len(), 3);
    }

    // ── Ping Failure Handling ──

    #[tokio::test]
    async fn test_ping_failure_unreachable() {
        // Connect to a port that should be closed.
        let client = OllamaClient::with_client(
            reqwest::Client::builder()
                .connect_timeout(Duration::from_millis(100))
                .timeout(Duration::from_millis(500))
                .build()
                .unwrap(),
            "http://127.0.0.1:1",
        );

        let result = client.ping().await;
        assert!(result.is_err(), "ping should fail for unreachable server");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("unreachable") || err_msg.contains("Ollama"),
            "error should mention unreachable: {err_msg}"
        );
    }

    #[tokio::test]
    async fn test_list_models_failure_unreachable() {
        let client = OllamaClient::with_client(
            reqwest::Client::builder()
                .connect_timeout(Duration::from_millis(100))
                .timeout(Duration::from_millis(500))
                .build()
                .unwrap(),
            "http://127.0.0.1:1",
        );

        let result = client.list_models().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_chat_failure_unreachable() {
        let client = OllamaClient::with_client(
            reqwest::Client::builder()
                .connect_timeout(Duration::from_millis(100))
                .timeout(Duration::from_millis(500))
                .build()
                .unwrap(),
            "http://127.0.0.1:1",
        );

        let result = client
            .chat(
                "llama3.1:8b",
                &[ChatMessage::user("Hi")],
                &ChatOptions::default(),
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_embed_empty_input() {
        let client = OllamaClient::with_client(
            reqwest::Client::builder()
                .connect_timeout(Duration::from_millis(100))
                .timeout(Duration::from_millis(500))
                .build()
                .unwrap(),
            "http://127.0.0.1:1",
        );

        // Empty input should return empty vec without making HTTP call.
        let result = client.embed("nomic-embed-text", &[]).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_embed_failure_unreachable() {
        let client = OllamaClient::with_client(
            reqwest::Client::builder()
                .connect_timeout(Duration::from_millis(100))
                .timeout(Duration::from_millis(500))
                .build()
                .unwrap(),
            "http://127.0.0.1:1",
        );

        let result = client
            .embed("nomic-embed-text", &["Hello".to_string()])
            .await;
        assert!(result.is_err());
    }

    // ── OllamaClient Construction ──

    #[test]
    fn test_client_new() {
        let client = OllamaClient::new("http://localhost:11434/");
        assert!(client.is_ok());
        // Trailing slash should be stripped.
        assert_eq!(client.unwrap().base_url, "http://localhost:11434");
    }

    #[test]
    fn test_client_new_strips_trailing_slash() {
        let client = OllamaClient::new("http://example.com:11434///").unwrap();
        assert_eq!(client.base_url, "http://example.com:11434");
    }
}
