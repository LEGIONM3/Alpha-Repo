//! SQLite persistence layer for the Event Bus.
//!
//! Every published event is written to the `events` table before dispatch.
//! This enables crash recovery via [`replay`] and historical analytics.

use rusqlite::{Connection, params};

use alpha_common::error::AlphaError;
use alpha_common::event::Event;
use alpha_common::types::Timestamp;

use crate::matcher::matches_topic;

/// SQL DDL for the events table and indexes.
pub(crate) const DDL: &str = "
    CREATE TABLE IF NOT EXISTS events (
        id              TEXT PRIMARY KEY,
        event_type      TEXT NOT NULL,
        source          TEXT NOT NULL,
        timestamp       TEXT NOT NULL,
        correlation_id  TEXT NOT NULL,
        priority        INTEGER NOT NULL DEFAULT 5,
        payload         TEXT NOT NULL,
        ttl_ms          INTEGER,
        retry_count     INTEGER NOT NULL DEFAULT 0,
        trace_id        TEXT NOT NULL
    );

    CREATE INDEX IF NOT EXISTS idx_events_type ON events(event_type);
    CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp);
    CREATE INDEX IF NOT EXISTS idx_events_correlation ON events(correlation_id);
";

/// Persist an event to SQLite.
pub(crate) fn persist_event(conn: &Connection, event: &Event) -> Result<(), AlphaError> {
    let id = event.id.to_string();
    let timestamp = event.timestamp.to_rfc3339();
    let correlation_id = event.correlation_id.to_string();
    let payload = serde_json::to_string(&event.payload)?;
    let trace_id = event.metadata.trace_id.to_string();

    conn.execute(
        "INSERT OR IGNORE INTO events
         (id, event_type, source, timestamp, correlation_id, priority, payload, ttl_ms, retry_count, trace_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            id,
            event.event_type,
            event.source,
            timestamp,
            correlation_id,
            event.priority,
            payload,
            event.metadata.ttl_ms.map(|v| v as i64),
            event.metadata.retry_count,
            trace_id,
        ],
    )
    .map_err(|e| AlphaError::Database(format!("Failed to persist event: {}", e)))?;

    Ok(())
}

/// Replay events matching a topic pattern since a given timestamp.
///
/// Fetches all events from SQLite, then filters by the pattern matcher.
/// This is efficient for Sprint 1 volumes. In future sprints, pattern
/// matching could be pushed into SQL using LIKE or FTS.
pub(crate) fn replay_events(
    conn: &Connection,
    pattern: &str,
    since: &Timestamp,
) -> Result<Vec<Event>, AlphaError> {
    let since_str = since.to_rfc3339();

    let mut stmt = conn
        .prepare(
            "SELECT id, event_type, source, timestamp, correlation_id,
                    priority, payload, ttl_ms, retry_count, trace_id
             FROM events
             WHERE timestamp >= ?1
             ORDER BY timestamp ASC",
        )
        .map_err(|e| AlphaError::Database(format!("replay prepare failed: {}", e)))?;

    let rows = stmt
        .query_map(params![since_str], |row| {
            Ok(EventRow {
                id: row.get(0)?,
                event_type: row.get(1)?,
                source: row.get(2)?,
                timestamp: row.get(3)?,
                correlation_id: row.get(4)?,
                priority: row.get(5)?,
                payload: row.get(6)?,
                ttl_ms: row.get(7)?,
                retry_count: row.get(8)?,
                trace_id: row.get(9)?,
            })
        })
        .map_err(|e| AlphaError::Database(format!("replay query failed: {}", e)))?;

    let mut events = Vec::new();
    for row_result in rows {
        let row = row_result
            .map_err(|e| AlphaError::Database(format!("replay row failed: {}", e)))?;

        // Filter by topic pattern.
        if !matches_topic(&row.event_type, pattern) {
            continue;
        }

        let event = row_to_event(row)?;
        events.push(event);
    }

    Ok(events)
}

/// Get the total count of persisted events.
pub(crate) fn event_count(conn: &Connection) -> Result<u64, AlphaError> {
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
        .map_err(|e| AlphaError::Database(format!("event_count failed: {}", e)))?;

    Ok(count as u64)
}

/// Delete events older than the given timestamp.
/// Returns the number of deleted events.
pub(crate) fn purge_before(conn: &Connection, before: &Timestamp) -> Result<u64, AlphaError> {
    let before_str = before.to_rfc3339();

    let rows_deleted = conn
        .execute(
            "DELETE FROM events WHERE timestamp < ?1",
            params![before_str],
        )
        .map_err(|e| AlphaError::Database(format!("purge_before failed: {}", e)))?;

    Ok(rows_deleted as u64)
}

// ── Internal helpers ──

/// Raw row from SQLite, before parsing into Event.
struct EventRow {
    id: String,
    event_type: String,
    source: String,
    timestamp: String,
    correlation_id: String,
    priority: u8,
    payload: String,
    ttl_ms: Option<i64>,
    retry_count: u8,
    trace_id: String,
}

/// Convert a raw SQLite row into an Event.
fn row_to_event(row: EventRow) -> Result<Event, AlphaError> {
    use alpha_common::event::EventMetadata;
    use chrono::DateTime;

    let id = row.id.parse().map_err(|e| {
        AlphaError::Database(format!("Failed to parse event id '{}': {}", row.id, e))
    })?;
    let timestamp = DateTime::parse_from_rfc3339(&row.timestamp)
        .map_err(|e| {
            AlphaError::Database(format!(
                "Failed to parse timestamp '{}': {}",
                row.timestamp, e
            ))
        })?
        .with_timezone(&chrono::Utc);
    let correlation_id = row.correlation_id.parse().map_err(|e| {
        AlphaError::Database(format!(
            "Failed to parse correlation_id '{}': {}",
            row.correlation_id, e
        ))
    })?;
    let payload: serde_json::Value = serde_json::from_str(&row.payload)?;
    let trace_id = row.trace_id.parse().map_err(|e| {
        AlphaError::Database(format!(
            "Failed to parse trace_id '{}': {}",
            row.trace_id, e
        ))
    })?;

    Ok(Event {
        id,
        event_type: row.event_type,
        source: row.source,
        timestamp,
        correlation_id,
        priority: row.priority,
        payload,
        metadata: EventMetadata {
            ttl_ms: row.ttl_ms.map(|v| v as u64),
            retry_count: row.retry_count,
            trace_id,
        },
    })
}
