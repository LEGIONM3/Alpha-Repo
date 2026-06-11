//! SQLite persistence layer for the Relationship Core.
//!
//! The relationship core is the protected substrate of the user-Alpha
//! relationship. Unlike regular memories, these records:
//! - Never decay (`decay_rate = 0.0`)
//! - Cannot have protection removed (`protected = true`)
//! - Cannot transition governance state (`governance_state = 'active'`)
//! - Have a minimum importance floor (`importance >= 0.5`)
//!
//! Invariants are enforced at two layers:
//! 1. **Rust**: `record.validate_invariants()` before insert
//! 2. **SQL**: `CHECK` constraints on the table

use std::path::Path;
use std::sync::Mutex;

use alpha_common::error::AlphaError;
use alpha_common::schemas::relationship::{
    RelationshipCategory, RelationshipCoreRecord, RelationshipSource,
};
use alpha_common::types::{AlphaId, JsonValue, Timestamp};
use rusqlite::Connection;
use tracing::{debug, info};

// ── Embedding Serialization Helpers ──
// Same packed f32 LE format as alpha-memory.

/// Serialize an embedding vector to a compact byte representation.
pub fn embedding_to_bytes(embedding: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(embedding.len() * 4);
    for &val in embedding {
        bytes.extend_from_slice(&val.to_le_bytes());
    }
    bytes
}

/// Deserialize an embedding vector from its compact byte representation.
pub fn bytes_to_embedding(bytes: &[u8]) -> Result<Vec<f32>, AlphaError> {
    if !bytes.len().is_multiple_of(4) {
        return Err(AlphaError::Invariant(format!(
            "Embedding byte length {} is not a multiple of 4",
            bytes.len()
        )));
    }
    let mut embedding = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        let val = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        embedding.push(val);
    }
    Ok(embedding)
}

// ── DDL ──

const DDL: &str = "
CREATE TABLE IF NOT EXISTS relationship_core (
    id                TEXT PRIMARY KEY,
    category          TEXT NOT NULL,
    content           TEXT NOT NULL,
    embedding         BLOB NOT NULL DEFAULT x'',
    importance        REAL NOT NULL DEFAULT 0.5 CHECK(importance >= 0.5),
    protected         INTEGER NOT NULL DEFAULT 1 CHECK(protected = 1),
    decay_rate        REAL NOT NULL DEFAULT 0.0 CHECK(abs(decay_rate) < 0.000001),
    governance_state  TEXT NOT NULL DEFAULT 'active' CHECK(governance_state = 'active'),
    source            TEXT NOT NULL,
    confidence        REAL NOT NULL DEFAULT 1.0,
    confirmed_by_user INTEGER NOT NULL DEFAULT 0,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    supersedes        TEXT,
    related_memories  TEXT NOT NULL DEFAULT '[]',
    metadata          TEXT NOT NULL DEFAULT '{}'
);

CREATE INDEX IF NOT EXISTS idx_rc_category ON relationship_core(category);
CREATE INDEX IF NOT EXISTS idx_rc_source ON relationship_core(source);
CREATE INDEX IF NOT EXISTS idx_rc_importance ON relationship_core(importance DESC);
CREATE INDEX IF NOT EXISTS idx_rc_created ON relationship_core(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_rc_confirmed ON relationship_core(confirmed_by_user);
";

/// SQLite-backed relationship core store.
///
/// Enforces invariants at both Rust and SQL layers.
pub struct RelationshipStore {
    conn: Mutex<Connection>,
}

impl RelationshipStore {
    /// Open (or create) the relationship database.
    pub fn open(db_path: &Path) -> Result<Self, AlphaError> {
        let conn = Connection::open(db_path).map_err(|e| {
            AlphaError::Database(format!("Failed to open relationship DB: {e}"))
        })?;

        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA busy_timeout=5000;",
        )
        .map_err(|e| AlphaError::Database(format!("Failed to set pragmas: {e}")))?;

        conn.execute_batch(DDL)
            .map_err(|e| AlphaError::Database(format!("Failed to create tables: {e}")))?;

        info!(path = %db_path.display(), "Relationship store opened");

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Store a relationship core record with its embedding.
    ///
    /// **Two-layer invariant enforcement:**
    /// 1. `record.validate_invariants()` — Rust layer
    /// 2. SQL `CHECK` constraints — database layer
    pub fn store_with_embedding(
        &self,
        record: &RelationshipCoreRecord,
    ) -> Result<AlphaId, AlphaError> {
        // Layer 1: Rust invariant validation.
        record.validate_invariants()?;

        let conn = self.conn.lock().map_err(|e| {
            AlphaError::Database(format!("Failed to acquire lock: {e}"))
        })?;

        let category = serde_json::to_string(&record.category)?;
        let category = category.trim_matches('"');
        let embedding_bytes = embedding_to_bytes(&record.embedding);
        let source = serde_json::to_string(&record.source)?;
        let source = source.trim_matches('"');
        let created_at = record.created_at.to_rfc3339();
        let updated_at = record.updated_at.to_rfc3339();
        let supersedes = record.supersedes.map(|id| id.to_string());
        let related_memories_json = serde_json::to_string(&record.related_memories)?;
        let metadata_json = serde_json::to_string(&record.metadata)?;

        // Layer 2: SQL CHECK constraints enforce invariants at the DB level.
        conn.execute(
            "INSERT INTO relationship_core
                (id, category, content, embedding, importance, protected,
                 decay_rate, governance_state, source, confidence, confirmed_by_user,
                 created_at, updated_at, supersedes, related_memories, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            rusqlite::params![
                record.id.to_string(),
                category,
                record.content,
                embedding_bytes,
                record.importance,
                record.protected as i32,
                record.decay_rate,
                record.governance_state,
                source,
                record.confidence,
                record.confirmed_by_user as i32,
                created_at,
                updated_at,
                supersedes,
                related_memories_json,
                metadata_json,
            ],
        )
        .map_err(|e| {
            // Convert CHECK constraint violations to Invariant errors.
            let msg = e.to_string();
            if msg.contains("CHECK") {
                AlphaError::Invariant(format!("SQL invariant violation: {msg}"))
            } else {
                AlphaError::Database(format!("Failed to store relationship record: {e}"))
            }
        })?;

        debug!(id = %record.id, category = %category, "Relationship record stored");
        Ok(record.id)
    }

    /// Retrieve a relationship record by ID.
    pub fn get(&self, id: &AlphaId) -> Result<Option<RelationshipCoreRecord>, AlphaError> {
        let conn = self.conn.lock().map_err(|e| {
            AlphaError::Database(format!("Failed to acquire lock: {e}"))
        })?;

        let mut stmt = conn
            .prepare(
                "SELECT id, category, content, embedding, importance, protected,
                        decay_rate, governance_state, source, confidence, confirmed_by_user,
                        created_at, updated_at, supersedes, related_memories, metadata
                 FROM relationship_core WHERE id = ?1",
            )
            .map_err(|e| AlphaError::Database(format!("Failed to prepare query: {e}")))?;

        let result = stmt
            .query_row(rusqlite::params![id.to_string()], |row| {
                Ok(row_to_record(row))
            })
            .optional()
            .map_err(|e| AlphaError::Database(format!("Failed to get record: {e}")))?;

        match result {
            Some(record) => Ok(Some(record?)),
            None => Ok(None),
        }
    }

    /// List relationship records with optional category filter.
    pub fn list(
        &self,
        category: Option<&RelationshipCategory>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<RelationshipCoreRecord>, AlphaError> {
        let conn = self.conn.lock().map_err(|e| {
            AlphaError::Database(format!("Failed to acquire lock: {e}"))
        })?;

        let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(cat) =
            category
        {
            let cat_str = serde_json::to_string(cat)
                .unwrap_or_default()
                .trim_matches('"')
                .to_string();
            (
                "SELECT id, category, content, embedding, importance, protected,
                        decay_rate, governance_state, source, confidence, confirmed_by_user,
                        created_at, updated_at, supersedes, related_memories, metadata
                 FROM relationship_core
                 WHERE category = ?1
                 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3"
                    .to_string(),
                vec![
                    Box::new(cat_str) as Box<dyn rusqlite::types::ToSql>,
                    Box::new(limit),
                    Box::new(offset),
                ],
            )
        } else {
            (
                "SELECT id, category, content, embedding, importance, protected,
                        decay_rate, governance_state, source, confidence, confirmed_by_user,
                        created_at, updated_at, supersedes, related_memories, metadata
                 FROM relationship_core
                 ORDER BY created_at DESC LIMIT ?1 OFFSET ?2"
                    .to_string(),
                vec![
                    Box::new(limit) as Box<dyn rusqlite::types::ToSql>,
                    Box::new(offset),
                ],
            )
        };

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AlphaError::Database(format!("Failed to prepare list query: {e}")))?;

        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();

        let rows = stmt
            .query_map(param_refs.as_slice(), |row| Ok(row_to_record(row)))
            .map_err(|e| AlphaError::Database(format!("Failed to list records: {e}")))?;

        let mut records = Vec::new();
        for row_result in rows {
            let record = row_result
                .map_err(|e| AlphaError::Database(format!("Row error: {e}")))?;
            records.push(record?);
        }

        Ok(records)
    }

    /// Retrieve communication preferences.
    ///
    /// Returns all records with `category = 'communication_pref'`,
    /// ordered by importance descending.
    pub fn get_communication_prefs(&self) -> Result<Vec<RelationshipCoreRecord>, AlphaError> {
        let conn = self.conn.lock().map_err(|e| {
            AlphaError::Database(format!("Failed to acquire lock: {e}"))
        })?;

        let mut stmt = conn
            .prepare(
                "SELECT id, category, content, embedding, importance, protected,
                        decay_rate, governance_state, source, confidence, confirmed_by_user,
                        created_at, updated_at, supersedes, related_memories, metadata
                 FROM relationship_core
                 WHERE category = 'communication_pref'
                 ORDER BY importance DESC",
            )
            .map_err(|e| AlphaError::Database(format!("Failed to prepare query: {e}")))?;

        let rows = stmt
            .query_map([], |row| Ok(row_to_record(row)))
            .map_err(|e| AlphaError::Database(format!("Failed to query prefs: {e}")))?;

        let mut records = Vec::new();
        for row_result in rows {
            let record = row_result
                .map_err(|e| AlphaError::Database(format!("Row error: {e}")))?;
            records.push(record?);
        }

        Ok(records)
    }

    /// Mark a record as confirmed by the user.
    pub fn confirm(&self, id: &AlphaId) -> Result<(), AlphaError> {
        let conn = self.conn.lock().map_err(|e| {
            AlphaError::Database(format!("Failed to acquire lock: {e}"))
        })?;

        let now = alpha_common::types::now().to_rfc3339();
        let rows = conn
            .execute(
                "UPDATE relationship_core SET confirmed_by_user = 1, updated_at = ?1 WHERE id = ?2",
                rusqlite::params![now, id.to_string()],
            )
            .map_err(|e| AlphaError::Database(format!("Failed to confirm record: {e}")))?;

        if rows == 0 {
            return Err(AlphaError::NotFound {
                entity: "RelationshipCoreRecord".to_string(),
                id: id.to_string(),
            });
        }

        debug!(id = %id, "Record confirmed by user");
        Ok(())
    }

    /// Count relationship records, optionally filtered by category.
    pub fn count(
        &self,
        category: Option<&RelationshipCategory>,
    ) -> Result<u64, AlphaError> {
        let conn = self.conn.lock().map_err(|e| {
            AlphaError::Database(format!("Failed to acquire lock: {e}"))
        })?;

        let (sql, params): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) =
            if let Some(cat) = category {
                let cat_str = serde_json::to_string(cat)
                    .unwrap_or_default()
                    .trim_matches('"')
                    .to_string();
                (
                    "SELECT COUNT(*) FROM relationship_core WHERE category = ?1",
                    vec![Box::new(cat_str) as Box<dyn rusqlite::types::ToSql>],
                )
            } else {
                (
                    "SELECT COUNT(*) FROM relationship_core",
                    vec![],
                )
            };

        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();

        let count: u64 = conn
            .query_row(sql, param_refs.as_slice(), |row| row.get(0))
            .map_err(|e| AlphaError::Database(format!("Failed to count: {e}")))?;

        Ok(count)
    }

    /// Supersede one record with another.
    ///
    /// **Deferred**: Not implemented yet.
    pub fn supersede(
        &self,
        _old_id: &AlphaId,
        _new_record: &RelationshipCoreRecord,
    ) -> Result<AlphaId, AlphaError> {
        Err(AlphaError::Other(
            "Not implemented in Sprint 2".into(),
        ))
    }

    /// Semantic search by embedding vector.
    ///
    /// **Deferred**: Not implemented yet.
    pub fn search(
        &self,
        _query_embedding: &[f32],
        _limit: u32,
    ) -> Result<Vec<RelationshipCoreRecord>, AlphaError> {
        Err(AlphaError::Other(
            "Not implemented in Sprint 2".into(),
        ))
    }

    /// Check whether WAL mode is enabled (for testing).
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

// ── Row Deserialization ──

fn row_to_record(row: &rusqlite::Row<'_>) -> Result<RelationshipCoreRecord, AlphaError> {
    let id_str: String = row.get(0).map_err(|e| AlphaError::Database(e.to_string()))?;
    let id: AlphaId = id_str
        .parse()
        .map_err(|e| AlphaError::Database(format!("Invalid UUID: {e}")))?;

    let cat_str: String = row.get(1).map_err(|e| AlphaError::Database(e.to_string()))?;
    let category: RelationshipCategory = serde_json::from_str(&format!("\"{cat_str}\""))?;

    let content: String = row.get(2).map_err(|e| AlphaError::Database(e.to_string()))?;

    let embedding_bytes: Vec<u8> = row.get(3).map_err(|e| AlphaError::Database(e.to_string()))?;
    let embedding = bytes_to_embedding(&embedding_bytes)?;

    let importance: f32 = row.get(4).map_err(|e| AlphaError::Database(e.to_string()))?;

    let protected_int: i32 = row.get(5).map_err(|e| AlphaError::Database(e.to_string()))?;
    let protected = protected_int != 0;

    let decay_rate: f32 = row.get(6).map_err(|e| AlphaError::Database(e.to_string()))?;
    let governance_state: String = row.get(7).map_err(|e| AlphaError::Database(e.to_string()))?;

    let source_str: String = row.get(8).map_err(|e| AlphaError::Database(e.to_string()))?;
    let source: RelationshipSource = serde_json::from_str(&format!("\"{source_str}\""))?;

    let confidence: f32 = row.get(9).map_err(|e| AlphaError::Database(e.to_string()))?;

    let confirmed_int: i32 = row.get(10).map_err(|e| AlphaError::Database(e.to_string()))?;
    let confirmed_by_user = confirmed_int != 0;

    let created_at_str: String = row.get(11).map_err(|e| AlphaError::Database(e.to_string()))?;
    let created_at: Timestamp = chrono::DateTime::parse_from_rfc3339(&created_at_str)
        .map_err(|e| AlphaError::Database(format!("Invalid timestamp: {e}")))?
        .with_timezone(&chrono::Utc);

    let updated_at_str: String = row.get(12).map_err(|e| AlphaError::Database(e.to_string()))?;
    let updated_at: Timestamp = chrono::DateTime::parse_from_rfc3339(&updated_at_str)
        .map_err(|e| AlphaError::Database(format!("Invalid timestamp: {e}")))?
        .with_timezone(&chrono::Utc);

    let supersedes_str: Option<String> =
        row.get(13).map_err(|e| AlphaError::Database(e.to_string()))?;
    let supersedes: Option<AlphaId> = supersedes_str
        .map(|s| s.parse())
        .transpose()
        .map_err(|e| AlphaError::Database(format!("Invalid supersedes UUID: {e}")))?;

    let related_str: String = row.get(14).map_err(|e| AlphaError::Database(e.to_string()))?;
    let related_memories: Vec<AlphaId> = serde_json::from_str(&related_str)?;

    let metadata_str: String = row.get(15).map_err(|e| AlphaError::Database(e.to_string()))?;
    let metadata: JsonValue = serde_json::from_str(&metadata_str)?;

    Ok(RelationshipCoreRecord {
        id,
        category,
        content,
        embedding,
        importance,
        protected,
        decay_rate,
        governance_state,
        source,
        confidence,
        confirmed_by_user,
        created_at,
        updated_at,
        supersedes,
        related_memories,
        metadata,
    })
}

/// Extension trait for optional query results.
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

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
