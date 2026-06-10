//! Event schema (Canonical Schema 2) — the universal message format.
//!
//! Every inter-service message in Alpha uses this shape.
//! The Event Bus persists events in SQLite and dispatches to subscribers.

use serde::{Deserialize, Serialize};
use crate::types::{AlphaId, JsonValue, Timestamp, new_id, now};

/// Metadata attached to every event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventMetadata {
    /// Time-to-live in milliseconds. `None` means no expiration.
    pub ttl_ms: Option<u64>,
    /// How many times this event has been retried after a delivery failure.
    pub retry_count: u8,
    /// Distributed trace ID for end-to-end tracing across services.
    pub trace_id: AlphaId,
}

/// The universal event type for all inter-service communication.
///
/// Events are the nervous system of Alpha. Every service communicates
/// by publishing and subscribing to events through the Event Bus.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {
    /// Unique event ID.
    pub id: AlphaId,
    /// Dot-notated topic, e.g., `"alpha.user.input.text"`.
    #[serde(rename = "type")]
    pub event_type: String,
    /// Service ID that emitted this event.
    pub source: String,
    /// When the event was created (UTC).
    pub timestamp: Timestamp,
    /// Groups related events into a causal chain.
    pub correlation_id: AlphaId,
    /// Priority: 1 (lowest) to 10 (highest). Default is 5.
    pub priority: u8,
    /// Event-specific payload. Shape depends on `event_type`.
    pub payload: JsonValue,
    /// Event metadata (TTL, retry count, trace ID).
    pub metadata: EventMetadata,
}

impl Event {
    /// Create a new Event with sensible defaults.
    ///
    /// - Generates a new unique ID
    /// - Sets timestamp to now
    /// - Sets priority to 5 (medium)
    /// - Creates a new trace/correlation ID
    pub fn new(
        event_type: impl Into<String>,
        source: impl Into<String>,
        payload: JsonValue,
    ) -> Self {
        let trace_id = new_id();
        Self {
            id: new_id(),
            event_type: event_type.into(),
            source: source.into(),
            timestamp: now(),
            correlation_id: trace_id,
            priority: 5,
            payload,
            metadata: EventMetadata {
                ttl_ms: None,
                retry_count: 0,
                trace_id,
            },
        }
    }

    /// Create an event that is part of an existing trace/correlation chain.
    ///
    /// Use this when an event is a response or continuation of a previous event.
    pub fn with_correlation(
        event_type: impl Into<String>,
        source: impl Into<String>,
        payload: JsonValue,
        correlation_id: AlphaId,
        trace_id: AlphaId,
    ) -> Self {
        Self {
            id: new_id(),
            event_type: event_type.into(),
            source: source.into(),
            timestamp: now(),
            correlation_id,
            priority: 5,
            payload,
            metadata: EventMetadata {
                ttl_ms: None,
                retry_count: 0,
                trace_id,
            },
        }
    }

    /// Set the priority of this event.
    ///
    /// Priority is clamped to 1-10.
    pub fn with_priority(mut self, priority: u8) -> Self {
        self.priority = priority.clamp(1, 10);
        self
    }

    /// Set the TTL (time-to-live) in milliseconds.
    pub fn with_ttl(mut self, ttl_ms: u64) -> Self {
        self.metadata.ttl_ms = Some(ttl_ms);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_event_creation() {
        let event = Event::new(
            "alpha.system.started",
            "alpha-core",
            json!({"version": "0.1.0"}),
        );

        assert_eq!(event.event_type, "alpha.system.started");
        assert_eq!(event.source, "alpha-core");
        assert_eq!(event.priority, 5);
        assert_eq!(event.metadata.retry_count, 0);
        assert!(event.metadata.ttl_ms.is_none());
        // correlation_id and trace_id should match for a new event
        assert_eq!(event.correlation_id, event.metadata.trace_id);
    }

    #[test]
    fn test_event_with_correlation() {
        let parent_correlation = new_id();
        let parent_trace = new_id();

        let event = Event::with_correlation(
            "alpha.agent.task.result",
            "coordinator",
            json!({"status": "completed"}),
            parent_correlation,
            parent_trace,
        );

        assert_eq!(event.correlation_id, parent_correlation);
        assert_eq!(event.metadata.trace_id, parent_trace);
        assert_ne!(event.id, parent_correlation);
    }

    #[test]
    fn test_event_serialization_roundtrip() {
        let event = Event::new(
            "alpha.test.roundtrip",
            "test",
            json!({"key": "value", "number": 42}),
        );

        let json_str = serde_json::to_string(&event).expect("serialize");
        let deserialized: Event = serde_json::from_str(&json_str).expect("deserialize");

        assert_eq!(event, deserialized);
    }

    #[test]
    fn test_event_with_priority() {
        let event = Event::new("test", "test", json!(null)).with_priority(10);
        assert_eq!(event.priority, 10);
    }

    #[test]
    fn test_event_priority_clamped() {
        let event = Event::new("test", "test", json!(null)).with_priority(255);
        assert_eq!(event.priority, 10);

        let event = Event::new("test", "test", json!(null)).with_priority(0);
        assert_eq!(event.priority, 1);
    }

    #[test]
    fn test_event_with_ttl() {
        let event = Event::new("test", "test", json!(null)).with_ttl(30000);
        assert_eq!(event.metadata.ttl_ms, Some(30000));
    }
}
