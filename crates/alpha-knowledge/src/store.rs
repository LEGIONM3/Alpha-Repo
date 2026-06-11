//! SQLite persistence layer for knowledge entities.

use std::path::Path;
use std::sync::Mutex;

use alpha_common::{AlphaError, AlphaId};
use rusqlite::Connection;
use tracing::{debug, info};

use crate::types::KnowledgeEntity;

/// SQLite-backed knowledge entity store.
///
/// Sprint 2: flat CRUD persistence only. No graph traversal,
/// no semantic search, no entity relationships.
pub struct KnowledgeStore {
    conn: Mutex<Connection>,
}

impl KnowledgeStore {
    /// Open (or create) the knowledge database at the given path.
    ///
    /// Creates the `entities` table if it does not exist.
    /// Enables WAL mode and sets pragmas for performance.
    pub fn open(db_path: &Path) -> Result<Self, AlphaError> {
        let conn = Connection::open(db_path)
            .map_err(|e| AlphaError::Database(format!("Failed to open knowledge DB: {e}")))?;

        // Performance pragmas.
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA busy_timeout=5000;",
        )
        .map_err(|e| AlphaError::Database(format!("Failed to set pragmas: {e}")))?;

        // Create tables.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS entities (
                id              TEXT PRIMARY KEY,
                entity_type     TEXT NOT NULL,
                name            TEXT NOT NULL,
                description     TEXT NOT NULL DEFAULT '',
                properties      TEXT NOT NULL DEFAULT '{}',
                source          TEXT NOT NULL,
                confidence      REAL NOT NULL DEFAULT 1.0,
                created_at      TEXT NOT NULL,
                updated_at      TEXT NOT NULL,
                metadata        TEXT NOT NULL DEFAULT '{}'
            );

            CREATE INDEX IF NOT EXISTS idx_entities_type ON entities(entity_type);
            CREATE INDEX IF NOT EXISTS idx_entities_name ON entities(name);",
        )
        .map_err(|e| AlphaError::Database(format!("Failed to create tables: {e}")))?;

        info!(path = %db_path.display(), "Knowledge store opened");
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Store a new knowledge entity.
    ///
    /// Returns the entity's ID on success.
    pub fn store(&self, entity: &KnowledgeEntity) -> Result<AlphaId, AlphaError> {
        let conn = self.conn.lock().map_err(|e| {
            AlphaError::Database(format!("Failed to acquire lock: {e}"))
        })?;

        let properties_json = serde_json::to_string(&entity.properties)?;
        let metadata_json = serde_json::to_string(&entity.metadata)?;
        let created_at = entity.created_at.to_rfc3339();
        let updated_at = entity.updated_at.to_rfc3339();

        conn.execute(
            "INSERT INTO entities (id, entity_type, name, description, properties,
                                   source, confidence, created_at, updated_at, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                entity.id.to_string(),
                entity.entity_type,
                entity.name,
                entity.description,
                properties_json,
                entity.source,
                entity.confidence,
                created_at,
                updated_at,
                metadata_json,
            ],
        )
        .map_err(|e| AlphaError::Database(format!("Failed to store entity: {e}")))?;

        debug!(id = %entity.id, name = %entity.name, "Entity stored");
        Ok(entity.id)
    }

    /// Retrieve an entity by its ID.
    ///
    /// Returns `None` if the entity does not exist.
    pub fn get(&self, id: &AlphaId) -> Result<Option<KnowledgeEntity>, AlphaError> {
        let conn = self.conn.lock().map_err(|e| {
            AlphaError::Database(format!("Failed to acquire lock: {e}"))
        })?;

        let mut stmt = conn
            .prepare(
                "SELECT id, entity_type, name, description, properties,
                        source, confidence, created_at, updated_at, metadata
                 FROM entities WHERE id = ?1",
            )
            .map_err(|e| AlphaError::Database(format!("Failed to prepare query: {e}")))?;

        let result = stmt
            .query_row(rusqlite::params![id.to_string()], |row| {
                Ok(row_to_entity(row))
            })
            .optional()
            .map_err(|e| AlphaError::Database(format!("Failed to get entity: {e}")))?;

        match result {
            Some(entity) => Ok(Some(entity?)),
            None => Ok(None),
        }
    }

    /// Update an existing entity.
    ///
    /// All fields except `id` and `created_at` are overwritten.
    /// The `updated_at` timestamp is set to the entity's current value.
    pub fn update(&self, entity: &KnowledgeEntity) -> Result<(), AlphaError> {
        let conn = self.conn.lock().map_err(|e| {
            AlphaError::Database(format!("Failed to acquire lock: {e}"))
        })?;

        let properties_json = serde_json::to_string(&entity.properties)?;
        let metadata_json = serde_json::to_string(&entity.metadata)?;
        let updated_at = entity.updated_at.to_rfc3339();

        let rows = conn
            .execute(
                "UPDATE entities
                 SET entity_type = ?1, name = ?2, description = ?3,
                     properties = ?4, source = ?5, confidence = ?6,
                     updated_at = ?7, metadata = ?8
                 WHERE id = ?9",
                rusqlite::params![
                    entity.entity_type,
                    entity.name,
                    entity.description,
                    properties_json,
                    entity.source,
                    entity.confidence,
                    updated_at,
                    metadata_json,
                    entity.id.to_string(),
                ],
            )
            .map_err(|e| AlphaError::Database(format!("Failed to update entity: {e}")))?;

        if rows == 0 {
            return Err(AlphaError::NotFound {
                entity: "KnowledgeEntity".to_string(),
                id: entity.id.to_string(),
            });
        }

        debug!(id = %entity.id, "Entity updated");
        Ok(())
    }

    /// Delete an entity by its ID.
    ///
    /// Returns `true` if the entity existed and was deleted, `false` if not found.
    pub fn delete(&self, id: &AlphaId) -> Result<bool, AlphaError> {
        let conn = self.conn.lock().map_err(|e| {
            AlphaError::Database(format!("Failed to acquire lock: {e}"))
        })?;

        let rows = conn
            .execute(
                "DELETE FROM entities WHERE id = ?1",
                rusqlite::params![id.to_string()],
            )
            .map_err(|e| AlphaError::Database(format!("Failed to delete entity: {e}")))?;

        if rows > 0 {
            debug!(id = %id, "Entity deleted");
        }
        Ok(rows > 0)
    }

    /// List entities with optional type filter and pagination.
    ///
    /// - `entity_type`: if `Some`, only return entities of this type.
    /// - `limit`: maximum number of entities to return.
    /// - `offset`: number of entities to skip (for pagination).
    pub fn list(
        &self,
        entity_type: Option<&str>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<KnowledgeEntity>, AlphaError> {
        let conn = self.conn.lock().map_err(|e| {
            AlphaError::Database(format!("Failed to acquire lock: {e}"))
        })?;

        let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match entity_type {
            Some(et) => (
                "SELECT id, entity_type, name, description, properties,
                        source, confidence, created_at, updated_at, metadata
                 FROM entities
                 WHERE entity_type = ?1
                 ORDER BY created_at DESC
                 LIMIT ?2 OFFSET ?3"
                    .to_string(),
                vec![
                    Box::new(et.to_string()),
                    Box::new(limit),
                    Box::new(offset),
                ],
            ),
            None => (
                "SELECT id, entity_type, name, description, properties,
                        source, confidence, created_at, updated_at, metadata
                 FROM entities
                 ORDER BY created_at DESC
                 LIMIT ?1 OFFSET ?2"
                    .to_string(),
                vec![Box::new(limit), Box::new(offset)],
            ),
        };

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AlphaError::Database(format!("Failed to prepare list query: {e}")))?;

        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();

        let rows = stmt
            .query_map(param_refs.as_slice(), |row| Ok(row_to_entity(row)))
            .map_err(|e| AlphaError::Database(format!("Failed to list entities: {e}")))?;

        let mut entities = Vec::new();
        for row_result in rows {
            let entity = row_result
                .map_err(|e| AlphaError::Database(format!("Row error: {e}")))?;
            entities.push(entity?);
        }

        Ok(entities)
    }

    /// Count entities, optionally filtered by type.
    pub fn count(&self, entity_type: Option<&str>) -> Result<u64, AlphaError> {
        let conn = self.conn.lock().map_err(|e| {
            AlphaError::Database(format!("Failed to acquire lock: {e}"))
        })?;

        let count: u64 = match entity_type {
            Some(et) => conn
                .query_row(
                    "SELECT COUNT(*) FROM entities WHERE entity_type = ?1",
                    rusqlite::params![et],
                    |row| row.get(0),
                )
                .map_err(|e| AlphaError::Database(format!("Failed to count: {e}")))?,
            None => conn
                .query_row("SELECT COUNT(*) FROM entities", [], |row| row.get(0))
                .map_err(|e| AlphaError::Database(format!("Failed to count: {e}")))?,
        };

        Ok(count)
    }

    /// Check whether WAL mode is enabled (used for testing).
    pub fn is_wal_mode(&self) -> Result<bool, AlphaError> {
        let conn = self.conn.lock().map_err(|e| {
            AlphaError::Database(format!("Failed to acquire lock: {e}"))
        })?;

        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .map_err(|e| AlphaError::Database(format!("Failed to check journal mode: {e}")))?;

        Ok(mode.to_lowercase() == "wal")
    }
}

/// Extract a `KnowledgeEntity` from a SQLite row.
///
/// The row must have columns in this order:
/// `id, entity_type, name, description, properties, source, confidence,
///  created_at, updated_at, metadata`
fn row_to_entity(row: &rusqlite::Row<'_>) -> Result<KnowledgeEntity, AlphaError> {
    let id_str: String = row.get(0).map_err(|e| AlphaError::Database(e.to_string()))?;
    let id: AlphaId = id_str
        .parse()
        .map_err(|e| AlphaError::Database(format!("Invalid UUID: {e}")))?;

    let entity_type: String = row.get(1).map_err(|e| AlphaError::Database(e.to_string()))?;
    let name: String = row.get(2).map_err(|e| AlphaError::Database(e.to_string()))?;
    let description: String = row.get(3).map_err(|e| AlphaError::Database(e.to_string()))?;

    let properties_str: String = row.get(4).map_err(|e| AlphaError::Database(e.to_string()))?;
    let properties = serde_json::from_str(&properties_str)?;

    let source: String = row.get(5).map_err(|e| AlphaError::Database(e.to_string()))?;
    let confidence: f32 = row.get(6).map_err(|e| AlphaError::Database(e.to_string()))?;

    let created_at_str: String = row.get(7).map_err(|e| AlphaError::Database(e.to_string()))?;
    let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
        .map_err(|e| AlphaError::Database(format!("Invalid timestamp: {e}")))?
        .with_timezone(&chrono::Utc);

    let updated_at_str: String = row.get(8).map_err(|e| AlphaError::Database(e.to_string()))?;
    let updated_at = chrono::DateTime::parse_from_rfc3339(&updated_at_str)
        .map_err(|e| AlphaError::Database(format!("Invalid timestamp: {e}")))?
        .with_timezone(&chrono::Utc);

    let metadata_str: String = row.get(9).map_err(|e| AlphaError::Database(e.to_string()))?;
    let metadata = serde_json::from_str(&metadata_str)?;

    Ok(KnowledgeEntity {
        id,
        entity_type,
        name,
        description,
        properties,
        source,
        confidence,
        created_at,
        updated_at,
        metadata,
    })
}

/// Trait extension to make `query_row` return `None` instead of error
/// when no rows are found.
trait OptionalExt<T> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error>;
}

impl<T> OptionalExt<T> for Result<T, rusqlite::Error> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
