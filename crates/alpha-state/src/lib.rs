//! # alpha-state
//!
//! Persistent key-value and document store for Project Alpha.
//!
//! This crate provides the [`StateStore`] — a SQLite-backed storage engine
//! that every Alpha service uses for persisting state across restarts.
//!
//! ## Design
//!
//! - **One SQLite file** (`state.db`) for service state.
//! - **WAL mode** for concurrent read/write performance.
//! - **Two storage models**:
//!   - **Key-Value**: namespaced string key → string value pairs.
//!   - **Document**: collection-scoped UUID → JSON document storage.
//! - **Migration framework**: versioned SQL migrations per component.
//! - **Synchronous API**: SQLite is fast enough; wrap in `spawn_blocking` if needed.
//!
//! ## Usage
//!
//! ```no_run
//! use alpha_state::StateStore;
//! use std::path::Path;
//!
//! let store = StateStore::open(Path::new("data/state.db")).unwrap();
//! store.set("my-service", "last_run", "2026-06-09T00:00:00Z").unwrap();
//! let val = store.get("my-service", "last_run").unwrap();
//! assert_eq!(val, Some("2026-06-09T00:00:00Z".to_string()));
//! ```

mod kv;
mod document;
mod migration;

#[cfg(test)]
mod tests;

use std::path::Path;

use rusqlite::Connection;
use tracing::{debug, info};

use alpha_common::error::AlphaError;
use alpha_common::types::{AlphaId, JsonValue};

pub use migration::Migration;

/// Persistent key-value and document store backed by SQLite.
///
/// All operations are synchronous. The database uses WAL mode for
/// concurrent read performance and `synchronous=NORMAL` for a good
/// balance between durability and write speed.
pub struct StateStore {
    conn: Connection,
}

impl StateStore {
    /// Open or create the state store backed by a SQLite file.
    ///
    /// On first open:
    /// - Creates the database file and parent directories.
    /// - Enables WAL journal mode.
    /// - Sets `synchronous=NORMAL`.
    /// - Creates the `kv`, `documents`, and `migrations` tables.
    pub fn open(db_path: &Path) -> Result<Self, AlphaError> {
        // Ensure parent directory exists.
        if let Some(parent) = db_path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    AlphaError::Config(format!(
                        "Failed to create state store directory '{}': {}",
                        parent.display(),
                        e
                    ))
                })?;
            }
        }

        let conn = Connection::open(db_path).map_err(|e| {
            AlphaError::Database(format!(
                "Failed to open state store at '{}': {}",
                db_path.display(),
                e
            ))
        })?;

        // Enable WAL mode for concurrent read/write.
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA foreign_keys=ON;"
        )
        .map_err(|e| AlphaError::Database(format!("Failed to set pragmas: {}", e)))?;

        debug!("State store pragmas set: WAL mode, synchronous=NORMAL");

        // Create core tables.
        conn.execute_batch(Self::DDL)
            .map_err(|e| AlphaError::Database(format!("Failed to create tables: {}", e)))?;

        info!(path = %db_path.display(), "State store opened");

        Ok(Self { conn })
    }

    /// SQL DDL for all core tables.
    const DDL: &'static str = "
        -- Key-Value Store
        CREATE TABLE IF NOT EXISTS kv (
            namespace   TEXT NOT NULL,
            key         TEXT NOT NULL,
            value       TEXT NOT NULL,
            updated_at  TEXT NOT NULL,
            PRIMARY KEY (namespace, key)
        );

        -- Document Store
        CREATE TABLE IF NOT EXISTS documents (
            collection  TEXT NOT NULL,
            id          TEXT NOT NULL,
            doc         TEXT NOT NULL,
            created_at  TEXT NOT NULL,
            updated_at  TEXT NOT NULL,
            PRIMARY KEY (collection, id)
        );

        -- Migration Tracking
        CREATE TABLE IF NOT EXISTS migrations (
            component   TEXT NOT NULL,
            version     INTEGER NOT NULL,
            description TEXT NOT NULL,
            applied_at  TEXT NOT NULL,
            PRIMARY KEY (component, version)
        );
    ";

    // ── Key-Value Operations ──
    // Delegated to the `kv` module for code organization,
    // but exposed as methods on StateStore.

    /// Set a key-value pair. Overwrites if the key already exists.
    pub fn set(&self, namespace: &str, key: &str, value: &str) -> Result<(), AlphaError> {
        kv::set(&self.conn, namespace, key, value)
    }

    /// Get a value by key. Returns `None` if the key does not exist.
    pub fn get(&self, namespace: &str, key: &str) -> Result<Option<String>, AlphaError> {
        kv::get(&self.conn, namespace, key)
    }

    /// Delete a key. Returns `true` if the key existed and was deleted.
    pub fn delete(&self, namespace: &str, key: &str) -> Result<bool, AlphaError> {
        kv::delete(&self.conn, namespace, key)
    }

    /// List all keys in a namespace.
    pub fn list_keys(&self, namespace: &str) -> Result<Vec<String>, AlphaError> {
        kv::list_keys(&self.conn, namespace)
    }

    // ── Document Operations ──

    /// Store a JSON document in a collection. Overwrites if the ID already exists.
    pub fn store_doc(
        &self,
        collection: &str,
        id: &AlphaId,
        doc: &JsonValue,
    ) -> Result<(), AlphaError> {
        document::store_doc(&self.conn, collection, id, doc)
    }

    /// Get a document by ID from a collection.
    pub fn get_doc(
        &self,
        collection: &str,
        id: &AlphaId,
    ) -> Result<Option<JsonValue>, AlphaError> {
        document::get_doc(&self.conn, collection, id)
    }

    /// Delete a document by ID. Returns `true` if the document existed.
    pub fn delete_doc(&self, collection: &str, id: &AlphaId) -> Result<bool, AlphaError> {
        document::delete_doc(&self.conn, collection, id)
    }

    /// List all document IDs in a collection.
    pub fn list_docs(&self, collection: &str) -> Result<Vec<AlphaId>, AlphaError> {
        document::list_docs(&self.conn, collection)
    }

    // ── Schema Migration ──

    /// Run pending migrations for a named component.
    ///
    /// Migrations are applied in version order. Already-applied migrations
    /// (tracked in the `migrations` table) are skipped. Each migration's SQL
    /// is executed in a transaction.
    pub fn migrate(
        &self,
        component: &str,
        migrations: &[Migration],
    ) -> Result<(), AlphaError> {
        migration::run_migrations(&self.conn, component, migrations)
    }
}

impl alpha_common::traits::Service for StateStore {
    fn name(&self) -> &str {
        "state-store"
    }

    fn init(&mut self) -> Result<(), AlphaError> {
        // Already initialized in open(). Nothing additional needed.
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), AlphaError> {
        // SQLite handles cleanup on drop. Checkpoint WAL for clean shutdown.
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|e| {
                AlphaError::Database(format!("Failed to checkpoint WAL on shutdown: {}", e))
            })?;
        info!("State store shutdown: WAL checkpointed");
        Ok(())
    }
}
