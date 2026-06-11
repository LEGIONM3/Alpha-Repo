//! Tests for alpha-orb-ui.
//!
//! Includes:
//! - Compile-time asset verification tests.
//! - IPC protocol tests (parsing, serialization).
//! - StreamEvent → IPC translation tests.
//! - IPC → ConversationRequest translation tests.

use alpha_common::types::new_id;
use alpha_conversation::types::{ConversationResponse, StreamEvent};

use crate::ipc;
use crate::{APP_JS, INDEX_HTML, STYLE_CSS};

// ── Asset Existence Tests ──

#[test]
fn test_html_exists() {
    assert!(
        !INDEX_HTML.is_empty(),
        "index.html should be embedded and non-empty"
    );
    assert!(
        INDEX_HTML.contains("<!DOCTYPE html>"),
        "index.html should contain DOCTYPE"
    );
    assert!(
        INDEX_HTML.contains("<div id=\"app\">"),
        "index.html should contain the app root"
    );
}

#[test]
fn test_css_exists() {
    assert!(
        !STYLE_CSS.is_empty(),
        "style.css should be embedded and non-empty"
    );
    assert!(
        STYLE_CSS.contains("--bg-primary"),
        "style.css should define --bg-primary"
    );
}

#[test]
fn test_js_exists() {
    assert!(!APP_JS.is_empty(), "app.js should be embedded and non-empty");
    assert!(
        APP_JS.contains("function addMessage"),
        "app.js should define addMessage"
    );
    assert!(
        APP_JS.contains("function sendMessage"),
        "app.js should define sendMessage"
    );
}

// ── Content Integrity Tests ──

#[test]
fn test_html_contains_required_elements() {
    assert!(INDEX_HTML.contains("id=\"messages\""));
    assert!(INDEX_HTML.contains("id=\"user-input\""));
    assert!(INDEX_HTML.contains("id=\"send-btn\""));
}

#[test]
fn test_html_contains_streaming_ipc() {
    assert!(
        INDEX_HTML.contains("stream_start"),
        "HTML should handle stream_start events"
    );
    assert!(
        INDEX_HTML.contains("stream_token"),
        "HTML should handle stream_token events"
    );
    assert!(
        INDEX_HTML.contains("stream_done"),
        "HTML should handle stream_done events"
    );
    assert!(
        INDEX_HTML.contains("stream_error"),
        "HTML should handle stream_error events"
    );
    assert!(
        INDEX_HTML.contains("setInputEnabled"),
        "HTML should disable input during streaming"
    );
}

// ── IPC Parsing Tests ──

#[test]
fn test_send_message_parse() {
    let raw = r#"{"type":"send_message","payload":{"message":"Hello"}}"#;
    let request = ipc::parse_request(raw);
    assert!(request.is_some(), "Valid send_message should parse");

    let request = request.unwrap();
    assert_eq!(request.msg_type, "send_message");

    let payload = ipc::extract_send_message(&request);
    assert_eq!(payload.message, "Hello");
}

#[test]
fn test_invalid_message_rejected() {
    assert!(ipc::parse_request("").is_none());
    assert!(ipc::parse_request("hello world").is_none());
    assert!(ipc::parse_request(r#"{"type":"unknown","payload":{}}"#).is_none());
    assert!(ipc::parse_request(r#"{"type":"send_message","payload":{}}"#).is_none());
    assert!(
        ipc::parse_request(r#"{"type":"send_message","payload":{"message":123}}"#).is_none()
    );
    assert!(ipc::parse_request(r#"{"payload":{"message":"hi"}}"#).is_none());
}

// ── IPC → ConversationRequest Translation ──

#[test]
fn test_ipc_to_conversation_request() {
    let payload = ipc::SendMessagePayload {
        message: "Tell me about Rust".to_string(),
    };

    let conv_request = ipc::to_conversation_request(&payload);

    assert_eq!(conv_request.message, "Tell me about Rust");
    assert!(
        conv_request.session_id.is_none(),
        "IPC messages should not specify a session_id"
    );
}

// ── StreamEvent → IPC Translation ──

#[test]
fn test_stream_event_translation() {
    // Started → stream_start
    let session_id = new_id();
    let event = StreamEvent::Started {
        session_id,
    };
    let response = ipc::translate_stream_event(&event);
    assert_eq!(response.msg_type, "stream_start");
    assert_eq!(
        response.payload["session_id"],
        session_id.to_string()
    );

    // Token → stream_token
    let event = StreamEvent::Token("Hello".to_string());
    let response = ipc::translate_stream_event(&event);
    assert_eq!(response.msg_type, "stream_token");
    assert_eq!(response.payload["token"], "Hello");

    // Verify JSON serialization round-trips correctly.
    let json = response.to_json();
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    assert_eq!(parsed["type"], "stream_token");
    assert_eq!(parsed["payload"]["token"], "Hello");
}

#[test]
fn test_stream_done_translation() {
    let session_id = new_id();
    let conv_response = ConversationResponse {
        session_id,
        response: "Alpha's full response text".to_string(),
        model: "llama3.2".to_string(),
        tokens_used: 42,
        duration_ms: 1500,
        memory_ids_used: vec![],
        relationship_ids_used: vec![],
    };

    let event = StreamEvent::Done(conv_response);
    let response = ipc::translate_stream_event(&event);

    assert_eq!(response.msg_type, "stream_done");
    assert_eq!(
        response.payload["response"],
        "Alpha's full response text"
    );

    // Verify JSON.
    let json = response.to_json();
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    assert_eq!(parsed["type"], "stream_done");
    assert_eq!(parsed["payload"]["response"], "Alpha's full response text");
}

#[test]
fn test_stream_error_translation() {
    let event = StreamEvent::Error("Connection timeout".to_string());
    let response = ipc::translate_stream_event(&event);

    assert_eq!(response.msg_type, "stream_error");
    assert_eq!(response.payload["error"], "Connection timeout");

    // Verify JSON.
    let json = response.to_json();
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    assert_eq!(parsed["type"], "stream_error");
    assert_eq!(parsed["payload"]["error"], "Connection timeout");
}

// ── IpcResponse Constructor Tests ──

#[test]
fn test_stream_start_response() {
    let response = ipc::IpcResponse::stream_start("session-123");
    assert_eq!(response.msg_type, "stream_start");
    assert_eq!(response.payload["session_id"], "session-123");
}

#[test]
fn test_stream_token_response() {
    let response = ipc::IpcResponse::stream_token("world");
    assert_eq!(response.msg_type, "stream_token");
    assert_eq!(response.payload["token"], "world");
}

#[test]
fn test_stream_done_response() {
    let response = ipc::IpcResponse::stream_done("Complete answer.");
    assert_eq!(response.msg_type, "stream_done");
    assert_eq!(response.payload["response"], "Complete answer.");
}

#[test]
fn test_stream_error_response() {
    let response = ipc::IpcResponse::stream_error("Model unavailable");
    assert_eq!(response.msg_type, "stream_error");
    assert_eq!(response.payload["error"], "Model unavailable");
}
