//! Comprehensive unit tests for the Event Bus.
//!
//! All tests use a temporary database and the tokio test runtime.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use chrono::{Duration, Utc};
use serde_json::json;

use alpha_common::event::Event;
use crate::EventBus;

/// Helper: create an EventBus backed by a temp file.
fn temp_bus() -> (EventBus, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("test_event_bus.db");
    let bus = EventBus::open(&db_path).expect("open event bus");
    (bus, dir)
}

// ════════════════════════════════════════════════════════════════
// Publish & Receive
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_publish_and_receive() {
    let (bus, _dir) = temp_bus();

    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = Arc::clone(&counter);

    bus.subscribe("alpha.system.started", move |_event| {
        counter_clone.fetch_add(1, Ordering::SeqCst);
    })
    .await
    .unwrap();

    let event = Event::new("alpha.system.started", "test", json!({"version": "0.1.0"}));
    bus.publish(event).await.unwrap();

    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

// ════════════════════════════════════════════════════════════════
// Topic Matching
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_topic_exact_matching() {
    let (bus, _dir) = temp_bus();

    let received = Arc::new(AtomicU32::new(0));
    let received_clone = Arc::clone(&received);

    bus.subscribe("alpha.system.started", move |_| {
        received_clone.fetch_add(1, Ordering::SeqCst);
    })
    .await
    .unwrap();

    // Should match.
    bus.publish(Event::new("alpha.system.started", "test", json!({})))
        .await
        .unwrap();

    // Should NOT match — different topic.
    bus.publish(Event::new("alpha.system.shutdown", "test", json!({})))
        .await
        .unwrap();

    assert_eq!(received.load(Ordering::SeqCst), 1, "Only exact match should fire");
}

#[tokio::test]
async fn test_topic_wildcard_matching() {
    let (bus, _dir) = temp_bus();

    let received = Arc::new(AtomicU32::new(0));
    let received_clone = Arc::clone(&received);

    bus.subscribe("alpha.user.*", move |_| {
        received_clone.fetch_add(1, Ordering::SeqCst);
    })
    .await
    .unwrap();

    // Should match — alpha.user.input has 3 segments, pattern has 3.
    bus.publish(Event::new("alpha.user.input", "test", json!({})))
        .await
        .unwrap();

    // Should match — alpha.user.voice has 3 segments.
    bus.publish(Event::new("alpha.user.voice", "test", json!({})))
        .await
        .unwrap();

    // Should NOT match — alpha.user.input.text has 4 segments.
    bus.publish(Event::new("alpha.user.input.text", "test", json!({})))
        .await
        .unwrap();

    assert_eq!(received.load(Ordering::SeqCst), 2, "Wildcard should match exactly 2 events");
}

#[tokio::test]
async fn test_no_match() {
    let (bus, _dir) = temp_bus();

    let received = Arc::new(AtomicU32::new(0));
    let received_clone = Arc::clone(&received);

    bus.subscribe("alpha.memory.*", move |_| {
        received_clone.fetch_add(1, Ordering::SeqCst);
    })
    .await
    .unwrap();

    // Completely different topic domain.
    bus.publish(Event::new("alpha.system.started", "test", json!({})))
        .await
        .unwrap();

    assert_eq!(received.load(Ordering::SeqCst), 0, "No match should not fire");
}

// ════════════════════════════════════════════════════════════════
// Multiple Subscribers
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_multiple_subscribers() {
    let (bus, _dir) = temp_bus();

    let counter_a = Arc::new(AtomicU32::new(0));
    let counter_b = Arc::new(AtomicU32::new(0));
    let counter_c = Arc::new(AtomicU32::new(0));

    let a = Arc::clone(&counter_a);
    let b = Arc::clone(&counter_b);
    let c = Arc::clone(&counter_c);

    // Three subscribers for the same topic.
    bus.subscribe("alpha.system.started", move |_| {
        a.fetch_add(1, Ordering::SeqCst);
    })
    .await
    .unwrap();

    bus.subscribe("alpha.system.started", move |_| {
        b.fetch_add(1, Ordering::SeqCst);
    })
    .await
    .unwrap();

    // Third subscriber on a wildcard that also matches.
    bus.subscribe("alpha.system.*", move |_| {
        c.fetch_add(1, Ordering::SeqCst);
    })
    .await
    .unwrap();

    bus.publish(Event::new("alpha.system.started", "test", json!({})))
        .await
        .unwrap();

    assert_eq!(counter_a.load(Ordering::SeqCst), 1);
    assert_eq!(counter_b.load(Ordering::SeqCst), 1);
    assert_eq!(counter_c.load(Ordering::SeqCst), 1);
}

// ════════════════════════════════════════════════════════════════
// Unsubscribe
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_unsubscribe() {
    let (bus, _dir) = temp_bus();

    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = Arc::clone(&counter);

    let handle = bus
        .subscribe("alpha.system.started", move |_| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        })
        .await
        .unwrap();

    // First publish — should be received.
    bus.publish(Event::new("alpha.system.started", "test", json!({})))
        .await
        .unwrap();
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    // Unsubscribe.
    bus.unsubscribe(handle).await.unwrap();

    // Second publish — should NOT be received.
    bus.publish(Event::new("alpha.system.started", "test", json!({})))
        .await
        .unwrap();
    assert_eq!(counter.load(Ordering::SeqCst), 1, "Should not receive after unsubscribe");
}

// ════════════════════════════════════════════════════════════════
// Persistence
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_persistence() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("persist_test.db");

    let event_id;

    // Session 1: publish an event.
    {
        let bus = EventBus::open(&db_path).unwrap();
        let event = Event::new("alpha.system.started", "test", json!({"session": 1}));
        event_id = event.id;
        bus.publish(event).await.unwrap();

        let count = bus.event_count().unwrap();
        assert_eq!(count, 1, "Event should be persisted");
    }

    // Session 2: reopen and verify the event is still there.
    {
        let bus = EventBus::open(&db_path).unwrap();
        let count = bus.event_count().unwrap();
        assert_eq!(count, 1, "Event must survive restart");

        // Replay should find it.
        let events = bus
            .replay(
                "alpha.system.started",
                Utc::now() - Duration::hours(1),
            )
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, event_id);
        assert_eq!(events[0].payload["session"], 1);
    }
}

// ════════════════════════════════════════════════════════════════
// Replay
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_replay_since() {
    let (bus, _dir) = temp_bus();

    // Publish 3 events.
    bus.publish(Event::new("alpha.system.started", "test", json!({"seq": 1})))
        .await
        .unwrap();
    bus.publish(Event::new("alpha.system.health", "test", json!({"seq": 2})))
        .await
        .unwrap();
    bus.publish(Event::new("alpha.system.shutdown", "test", json!({"seq": 3})))
        .await
        .unwrap();

    // Replay all system events from the past hour.
    let events = bus
        .replay("alpha.system.*", Utc::now() - Duration::hours(1))
        .unwrap();
    assert_eq!(events.len(), 3, "All 3 events should match alpha.system.*");

    // Replay only exact match.
    let events = bus
        .replay("alpha.system.started", Utc::now() - Duration::hours(1))
        .unwrap();
    assert_eq!(events.len(), 1, "Only 1 event should match exact topic");
    assert_eq!(events[0].event_type, "alpha.system.started");
}

#[tokio::test]
async fn test_replay_since_filters_by_time() {
    let (bus, _dir) = temp_bus();

    bus.publish(Event::new("alpha.system.started", "test", json!({})))
        .await
        .unwrap();

    // Replay from the future — should find nothing.
    let events = bus
        .replay("alpha.system.*", Utc::now() + Duration::hours(1))
        .unwrap();
    assert_eq!(events.len(), 0, "Future timestamp should find no events");
}

// ════════════════════════════════════════════════════════════════
// Purge
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_purge() {
    let (bus, _dir) = temp_bus();

    // Publish some events.
    bus.publish(Event::new("alpha.test.one", "test", json!({})))
        .await
        .unwrap();
    bus.publish(Event::new("alpha.test.two", "test", json!({})))
        .await
        .unwrap();
    bus.publish(Event::new("alpha.test.three", "test", json!({})))
        .await
        .unwrap();

    assert_eq!(bus.event_count().unwrap(), 3);

    // Purge events older than 1 hour in the future — should purge all.
    let purged = bus.purge_before(Utc::now() + Duration::hours(1)).unwrap();
    assert_eq!(purged, 3, "All 3 events should be purged");

    assert_eq!(bus.event_count().unwrap(), 0, "No events should remain");
}

#[tokio::test]
async fn test_purge_selective() {
    let (bus, _dir) = temp_bus();

    bus.publish(Event::new("alpha.test.one", "test", json!({})))
        .await
        .unwrap();

    // Purge events older than 1 hour ago — should purge nothing (events are new).
    let purged = bus.purge_before(Utc::now() - Duration::hours(1)).unwrap();
    assert_eq!(purged, 0, "No events should be purged (all are recent)");

    assert_eq!(bus.event_count().unwrap(), 1, "Event should still exist");
}

// ════════════════════════════════════════════════════════════════
// Event Count
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_event_count() {
    let (bus, _dir) = temp_bus();

    assert_eq!(bus.event_count().unwrap(), 0, "Empty bus should have 0 events");

    bus.publish(Event::new("alpha.test.one", "test", json!({})))
        .await
        .unwrap();
    assert_eq!(bus.event_count().unwrap(), 1);

    bus.publish(Event::new("alpha.test.two", "test", json!({})))
        .await
        .unwrap();
    assert_eq!(bus.event_count().unwrap(), 2);
}

// ════════════════════════════════════════════════════════════════
// Subscription Count
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_subscription_count() {
    let (bus, _dir) = temp_bus();

    assert_eq!(bus.subscription_count().await, 0);

    let h1 = bus.subscribe("alpha.a.*", |_| {}).await.unwrap();
    assert_eq!(bus.subscription_count().await, 1);

    let _h2 = bus.subscribe("alpha.b.*", |_| {}).await.unwrap();
    assert_eq!(bus.subscription_count().await, 2);

    bus.unsubscribe(h1).await.unwrap();
    assert_eq!(bus.subscription_count().await, 1);
}

// ════════════════════════════════════════════════════════════════
// Event payload integrity
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_event_payload_survives_roundtrip() {
    let (bus, _dir) = temp_bus();

    let payload = json!({
        "complex": {
            "nested": true,
            "array": [1, 2, 3],
            "string": "hello world"
        }
    });

    let event = Event::new("alpha.test.roundtrip", "test", payload.clone());
    let event_id = event.id;

    bus.publish(event).await.unwrap();

    let replayed = bus
        .replay("alpha.test.roundtrip", Utc::now() - Duration::hours(1))
        .unwrap();

    assert_eq!(replayed.len(), 1);
    assert_eq!(replayed[0].id, event_id);
    assert_eq!(replayed[0].payload, payload);
}

// ════════════════════════════════════════════════════════════════
// WAL Mode Verification
// ════════════════════════════════════════════════════════════════

#[test]
fn test_wal_mode_enabled() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("wal_test.db");
    let _bus = EventBus::open(&db_path).unwrap();

    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let mode: String = conn
        .query_row("PRAGMA journal_mode;", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "wal", "Event bus must use WAL mode");
}
