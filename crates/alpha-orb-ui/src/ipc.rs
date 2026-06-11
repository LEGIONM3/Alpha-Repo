//! IPC protocol for Rust ↔ WebView communication.
//!
//! ## Protocol
//!
//! ### JS → Rust (`IpcRequest`)
//!
//! ```json
//! { "type": "send_message", "payload": { "message": "Hello" } }
//! ```
//!
//! ### Rust → JS (Streaming Events)
//!
//! ```json
//! { "type": "stream_start", "payload": { "session_id": "..." } }
//! { "type": "stream_token", "payload": { "token": "Hello" } }
//! { "type": "stream_done",  "payload": { "response": "..." } }
//! { "type": "stream_error", "payload": { "error": "..." } }
//! ```
//!
//! ## Handler
//!
//! `ConversationBridge` wraps `ConversationService` and translates between
//! IPC messages and the streaming conversation pipeline.

use std::sync::Arc;

use alpha_conversation::types::{ConversationRequest, ModelProvider, StreamEvent};
use alpha_conversation::ConversationService;
use serde::{Deserialize, Serialize};
use tao::event_loop::EventLoopProxy;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::IpcEvent;

// ── JS → Rust Messages ──

/// Top-level IPC request envelope from JavaScript.
#[derive(Debug, Clone, Deserialize)]
pub struct IpcRequest {
    /// The request type (e.g. `"send_message"`).
    #[serde(rename = "type")]
    pub msg_type: String,
    /// The request payload.
    pub payload: serde_json::Value,
}

/// Payload for a `send_message` request.
#[derive(Debug, Clone, Deserialize)]
pub struct SendMessagePayload {
    /// The user's message text.
    pub message: String,
}

// ── Rust → JS Messages ──

/// Top-level IPC response envelope to JavaScript.
#[derive(Debug, Clone, Serialize)]
pub struct IpcResponse {
    /// The response type (e.g. `"stream_token"`).
    #[serde(rename = "type")]
    pub msg_type: String,
    /// The response payload.
    pub payload: serde_json::Value,
}

impl IpcResponse {
    /// Create a `stream_start` event.
    pub fn stream_start(session_id: &str) -> Self {
        Self {
            msg_type: "stream_start".to_string(),
            payload: serde_json::json!({ "session_id": session_id }),
        }
    }

    /// Create a `stream_token` event.
    pub fn stream_token(token: &str) -> Self {
        Self {
            msg_type: "stream_token".to_string(),
            payload: serde_json::json!({ "token": token }),
        }
    }

    /// Create a `stream_done` event.
    pub fn stream_done(response: &str) -> Self {
        Self {
            msg_type: "stream_done".to_string(),
            payload: serde_json::json!({ "response": response }),
        }
    }

    /// Create a `stream_error` event.
    pub fn stream_error(error: &str) -> Self {
        Self {
            msg_type: "stream_error".to_string(),
            payload: serde_json::json!({ "error": error }),
        }
    }

    /// Serialize to a JSON string for injection into JavaScript.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("serialize IpcResponse")
    }
}

// ── Parsing ──

/// Parse a raw IPC string from JavaScript into a typed request.
///
/// Returns `None` if the string is not valid JSON or has an unknown type.
pub fn parse_request(raw: &str) -> Option<IpcRequest> {
    let request: IpcRequest = match serde_json::from_str(raw) {
        Ok(r) => r,
        Err(e) => {
            warn!(error = %e, "Failed to parse IPC request");
            return None;
        }
    };

    // Validate known message types.
    match request.msg_type.as_str() {
        "send_message" => {
            // Validate the payload can be deserialized.
            if serde_json::from_value::<SendMessagePayload>(request.payload.clone()).is_err() {
                warn!("Invalid send_message payload");
                return None;
            }
            Some(request)
        }
        unknown => {
            warn!(msg_type = unknown, "Unknown IPC message type");
            None
        }
    }
}

/// Extract a `SendMessagePayload` from a validated `IpcRequest`.
///
/// # Panics
///
/// Panics if the request type is not `send_message` or payload is invalid.
/// Only call after `parse_request` has validated the request.
pub fn extract_send_message(request: &IpcRequest) -> SendMessagePayload {
    serde_json::from_value(request.payload.clone()).expect("validated payload")
}

// ── StreamEvent → IPC Translation ──

/// Translate a `StreamEvent` from `ConversationService` into an `IpcResponse`.
pub fn translate_stream_event(event: &StreamEvent) -> IpcResponse {
    match event {
        StreamEvent::Started { session_id } => {
            IpcResponse::stream_start(&session_id.to_string())
        }
        StreamEvent::Token(token) => IpcResponse::stream_token(token),
        StreamEvent::Done(response) => IpcResponse::stream_done(&response.response),
        StreamEvent::Error(error) => IpcResponse::stream_error(error),
    }
}

/// Convert a `SendMessagePayload` into a `ConversationRequest`.
pub fn to_conversation_request(payload: &SendMessagePayload) -> ConversationRequest {
    ConversationRequest {
        message: payload.message.clone(),
        session_id: None,
    }
}

// ── ConversationBridge ──

/// Bridges IPC messages to the `ConversationService` streaming pipeline.
///
/// Owns an `Arc<ConversationService<M>>` and a tokio `Handle` to spawn
/// async work without blocking the tao event loop.
pub struct ConversationBridge<M: ModelProvider + 'static> {
    service: Arc<ConversationService<M>>,
    runtime_handle: tokio::runtime::Handle,
}

impl<M: ModelProvider + 'static> ConversationBridge<M> {
    /// Create a new bridge.
    pub fn new(
        service: Arc<ConversationService<M>>,
        runtime_handle: tokio::runtime::Handle,
    ) -> Self {
        Self {
            service,
            runtime_handle,
        }
    }

    /// Handle a raw IPC message from JavaScript.
    ///
    /// Parses the request, dispatches to `ConversationService::send_message_stream()`,
    /// and forwards streaming events back to the UI through the event loop proxy.
    pub fn handle_message(&self, raw: &str, proxy: &EventLoopProxy<IpcEvent>) {
        let request = match parse_request(raw) {
            Some(r) => r,
            None => return,
        };

        if request.msg_type == "send_message" {
            let payload = extract_send_message(&request);
            debug!(message = %payload.message, "IPC: send_message → ConversationService");

            let conv_request = to_conversation_request(&payload);
            let service = Arc::clone(&self.service);
            let proxy = proxy.clone();

            self.runtime_handle.spawn(async move {
                Self::stream_conversation(service, conv_request, proxy).await;
            });
        }
    }

    /// Run the streaming conversation pipeline and forward events to the UI.
    async fn stream_conversation(
        service: Arc<ConversationService<M>>,
        request: ConversationRequest,
        proxy: EventLoopProxy<IpcEvent>,
    ) {
        // Start the stream.
        let mut rx: mpsc::UnboundedReceiver<StreamEvent> =
            match service.send_message_stream(&request).await {
                Ok(rx) => rx,
                Err(e) => {
                    warn!(error = %e, "ConversationService stream failed to start");
                    let error_resp = IpcResponse::stream_error(&format!("{e}"));
                    Self::send_to_ui(&proxy, &error_resp);
                    return;
                }
            };

        // Forward each StreamEvent to the UI.
        while let Some(event) = rx.recv().await {
            let is_terminal = matches!(event, StreamEvent::Done(_) | StreamEvent::Error(_));
            let ipc_response = translate_stream_event(&event);

            if let StreamEvent::Done(_) = &event {
                info!("Streaming conversation complete");
            }

            Self::send_to_ui(&proxy, &ipc_response);

            if is_terminal {
                break;
            }
        }
    }

    /// Send an IPC response to the UI via the event loop proxy.
    fn send_to_ui(proxy: &EventLoopProxy<IpcEvent>, response: &IpcResponse) {
        let js = format!("window.__alpha_receive({})", response.to_json());
        if let Err(e) = proxy.send_event(IpcEvent::EvaluateScript(js)) {
            warn!(error = ?e, "Failed to send IPC event to event loop");
        }
    }
}
