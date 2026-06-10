//! # alpha-event-bus
//!
//! The central nervous system of Project Alpha.
//!
//! Every inter-service message is an [`Event`] published through the [`EventBus`].
//! The Event Bus:
//!
//! 1. **Persists** every event to SQLite (WAL mode) before dispatching.
//! 2. **Dispatches** to all subscribers whose topic pattern matches.
//! 3. **Replays** historical events for crash recovery.
//! 4. **Purges** old events to keep the database bounded.
//!
//! ## Topic Matching
//!
//! - Exact: `"alpha.system.started"` matches only that topic.
//! - Wildcard: `"alpha.user.*"` matches one segment (`"alpha.user.input"`)
//!   but NOT deeper (`"alpha.user.input.text"`).
//!
//! ## Usage
//!
//! ```no_run
//! use alpha_event_bus::EventBus;
//! use alpha_common::{Event, now};
//! use serde_json::json;
//! use std::path::Path;
//!
//! # async fn example() {
//! let bus = EventBus::open(Path::new("data/event_bus.db")).unwrap();
//!
//! // Subscribe
//! let handle = bus.subscribe("alpha.system.*", |event| {
//!     println!("Got event: {}", event.event_type);
//! }).await.unwrap();
//!
//! // Publish
//! let event = Event::new("alpha.system.started", "core", json!({}));
//! bus.publish(event).await.unwrap();
//!
//! // Replay
//! let past_events = bus.replay("alpha.system.*", now()).unwrap();
//! # }
//! ```

pub mod matcher;
pub mod store;
pub mod subscriber;

#[cfg(test)]
mod tests;

use std::path::Path;
use std::sync::Arc;

use rusqlite::Connection;
use tokio::sync::RwLock;
use tracing::{debug, info};

use alpha_common::error::AlphaError;
use alpha_common::event::Event;
use alpha_common::types::{Timestamp, new_id};

pub use subscriber::SubscriptionHandle;

use subscriber::{HandlerFn, Subscription};

/// The central event bus. All services communicate through this.
///
/// Thread-safe: uses `Arc<RwLock<_>>` for subscriber list and
/// `tokio::sync::Mutex` for the SQLite connection (single-writer).
pub struct EventBus {
    /// SQLite connection, protected by a Mutex for single-writer safety.
    conn: Arc<tokio::sync::Mutex<Connection>>,
    /// Active subscriptions, protected by an RwLock for concurrent reads.
    subscriptions: Arc<RwLock<Vec<Subscription>>>,
}

impl EventBus {
    /// Open or create the event bus backed by a SQLite file.
    ///
    /// On first open:
    /// - Creates the database file and parent directories.
    /// - Enables WAL journal mode.
    /// - Sets `synchronous=NORMAL`.
    /// - Creates the `events` table and indexes.
    pub fn open(db_path: &Path) -> Result<Self, AlphaError> {
        // Ensure parent directory exists.
        if let Some(parent) = db_path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    AlphaError::Config(format!(
                        "Failed to create event bus directory '{}': {}",
                        parent.display(),
                        e
                    ))
                })?;
            }
        }

        let conn = Connection::open(db_path).map_err(|e| {
            AlphaError::Database(format!(
                "Failed to open event bus at '{}': {}",
                db_path.display(),
                e
            ))
        })?;

        // Enable WAL mode.
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;",
        )
        .map_err(|e| AlphaError::Database(format!("Failed to set event bus pragmas: {}", e)))?;

        // Create tables and indexes.
        conn.execute_batch(store::DDL)
            .map_err(|e| AlphaError::Database(format!("Failed to create events table: {}", e)))?;

        info!(path = %db_path.display(), "Event bus opened");

        Ok(Self {
            conn: Arc::new(tokio::sync::Mutex::new(conn)),
            subscriptions: Arc::new(RwLock::new(Vec::new())),
        })
    }

    /// Publish an event. Persists to SQLite, then dispatches to matching subscribers.
    ///
    /// Persistence happens first — if the write fails, subscribers are NOT called.
    /// This ensures no event is dispatched without being recoverable via replay.
    pub async fn publish(&self, event: Event) -> Result<(), AlphaError> {
        // 1. Persist to SQLite.
        {
            let conn = self.conn.lock().await;
            store::persist_event(&conn, &event)?;
        }

        debug!(
            event_type = %event.event_type,
            id = %event.id,
            "Event persisted"
        );

        // 2. Dispatch to matching subscribers.
        let subs = self.subscriptions.read().await;
        for sub in subs.iter() {
            if matcher::matches_topic(&event.event_type, &sub.handle.pattern) {
                let handler = Arc::clone(&sub.handler);
                let event_clone = event.clone();
                // Call handler directly (synchronous in Sprint 1).
                // Future: spawn_blocking or use channels for backpressure.
                handler(event_clone);
            }
        }

        debug!(
            event_type = %event.event_type,
            subscriber_count = subs.len(),
            "Event dispatched"
        );

        Ok(())
    }

    /// Subscribe to events matching a topic pattern.
    ///
    /// Pattern supports single-segment wildcard: `"alpha.user.*"`
    /// Returns a [`SubscriptionHandle`] that can be used to unsubscribe.
    pub async fn subscribe(
        &self,
        pattern: &str,
        handler: impl Fn(Event) + Send + Sync + 'static,
    ) -> Result<SubscriptionHandle, AlphaError> {
        let handle = SubscriptionHandle {
            id: new_id(),
            pattern: pattern.to_string(),
        };

        let subscription = Subscription {
            handle: handle.clone(),
            handler: Arc::new(handler) as HandlerFn,
        };

        let mut subs = self.subscriptions.write().await;
        subs.push(subscription);

        debug!(
            pattern = %handle.pattern,
            id = %handle.id,
            "Subscription added"
        );

        Ok(handle)
    }

    /// Unsubscribe a handler by its handle.
    pub async fn unsubscribe(&self, handle: SubscriptionHandle) -> Result<(), AlphaError> {
        let mut subs = self.subscriptions.write().await;
        let initial_len = subs.len();
        subs.retain(|s| s.handle.id != handle.id);
        let removed = initial_len - subs.len();

        debug!(
            pattern = %handle.pattern,
            id = %handle.id,
            removed,
            "Subscription removed"
        );

        Ok(())
    }

    /// Replay all events matching a topic pattern since a given timestamp.
    ///
    /// Used for crash recovery — a service that restarts can replay
    /// events it missed while it was down.
    pub fn replay(
        &self,
        pattern: &str,
        since: Timestamp,
    ) -> Result<Vec<Event>, AlphaError> {
        // Use try_lock to avoid blocking in sync context.
        // For Sprint 1, this is called during initialization (single-threaded).
        let conn = self.conn.try_lock().map_err(|_| {
            AlphaError::EventBus(
                "Cannot replay: event bus connection is locked".to_string(),
            )
        })?;
        store::replay_events(&conn, pattern, &since)
    }

    /// Get the count of persisted events.
    pub fn event_count(&self) -> Result<u64, AlphaError> {
        let conn = self.conn.try_lock().map_err(|_| {
            AlphaError::EventBus(
                "Cannot count: event bus connection is locked".to_string(),
            )
        })?;
        store::event_count(&conn)
    }

    /// Delete events older than the given timestamp.
    /// Returns the number of deleted events.
    pub fn purge_before(&self, before: Timestamp) -> Result<u64, AlphaError> {
        let conn = self.conn.try_lock().map_err(|_| {
            AlphaError::EventBus(
                "Cannot purge: event bus connection is locked".to_string(),
            )
        })?;
        store::purge_before(&conn, &before)
    }

    /// Get the subscription count (for diagnostics).
    pub async fn subscription_count(&self) -> usize {
        self.subscriptions.read().await.len()
    }
}

impl alpha_common::traits::Service for EventBus {
    fn name(&self) -> &str {
        "event-bus"
    }

    fn init(&mut self) -> Result<(), AlphaError> {
        // Already initialized in open().
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), AlphaError> {
        // Checkpoint WAL for clean shutdown.
        let conn = self.conn.try_lock().map_err(|_| {
            AlphaError::EventBus("Cannot shutdown: connection is locked".to_string())
        })?;
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|e| {
                AlphaError::Database(format!(
                    "Failed to checkpoint event bus WAL on shutdown: {}",
                    e
                ))
            })?;
        info!("Event bus shutdown: WAL checkpointed");
        Ok(())
    }
}
