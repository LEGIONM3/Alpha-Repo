//! SQLite persistence layer for Alpha's memory system.
//!
//! Sprint 2 Phase 1: CRUD operations, embedding blob storage, governance
//! transitions. No semantic search, no cosine similarity, no scoring.

use std::path::Path;
use std::sync::Mutex;

use alpha_common::error::AlphaError;
use alpha_common::schemas::memory::{GovernanceState, MemoryRecord, MemoryType};
use alpha_common::types::{AlphaId, JsonValue, Timestamp};
use rusqlite::Connection;
use tracing::{debug, info};

// ── Embedding Serialization Helpers ──

/// Serialize an embedding vector to a compact byte representation.
///
/// Each `f32` is stored as 4 bytes in little-endian order. This is the
/// format stored in the `embedding` BLOB column.
pub fn embedding_to_bytes(embedding: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(embedding.len() * 4);
    for &val in embedding {
        bytes.extend_from_slice(&val.to_le_bytes());
    }
    bytes
}

/// Deserialize an embedding vector from its compact byte representation.
///
/// Reverses [`embedding_to_bytes`]. Returns an error if the byte length
/// is not a multiple of 4.
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

/// Validate embedding dimension against expected size.
///
/// Sprint 2 Amendment §2: reject mismatched embeddings before storage.
pub fn validate_embedding_dimension(
    embedding: &[f32],
    expected_dim: usize,
) -> Result<(), AlphaError> {
    if !embedding.is_empty() && embedding.len() != expected_dim {
        return Err(AlphaError::Invariant(format!(
            "Embedding dimension mismatch: got {}, expected {}",
            embedding.len(),
            expected_dim
        )));
    }
    Ok(())
}

// ── DDL ──

const DDL: &str = "
CREATE TABLE IF NOT EXISTS memories (
    id                TEXT PRIMARY KEY,
    memory_type       TEXT NOT NULL,
    content           TEXT NOT NULL,
    embedding         BLOB NOT NULL DEFAULT x'',
    importance        REAL NOT NULL DEFAULT 0.5,
    access_count      INTEGER NOT NULL DEFAULT 0,
    last_accessed     TEXT NOT NULL,
    created_at        TEXT NOT NULL,
    source            TEXT NOT NULL,
    associations      TEXT NOT NULL DEFAULT '[]',
    tags              TEXT NOT NULL DEFAULT '[]',
    confidence        REAL NOT NULL DEFAULT 1.0,
    decay_rate        REAL NOT NULL DEFAULT 0.01,
    governance_state  TEXT NOT NULL DEFAULT 'active',
    metadata          TEXT NOT NULL DEFAULT '{}'
);

CREATE INDEX IF NOT EXISTS idx_memories_type ON memories(memory_type);
CREATE INDEX IF NOT EXISTS idx_memories_governance ON memories(governance_state);
CREATE INDEX IF NOT EXISTS idx_memories_source ON memories(source);
CREATE INDEX IF NOT EXISTS idx_memories_importance ON memories(importance DESC);
CREATE INDEX IF NOT EXISTS idx_memories_created ON memories(created_at DESC);
";

/// SQLite-backed memory store.
///
/// Provides CRUD operations, embedding blob storage, governance
/// transitions, and semantic search by embedding similarity.
pub struct MemoryStore {
    pub(crate) conn: Mutex<Connection>,
    /// Expected embedding dimension (from config, e.g. 768).
    /// `0` means no validation (dimension unknown).
    pub(crate) expected_dimension: usize,
}

impl MemoryStore {
    /// Open (or create) the memory database at the given path.
    ///
    /// `expected_dimension`: the embedding size to validate against.
    /// Pass `0` to skip dimension validation.
    pub fn open(db_path: &Path, expected_dimension: usize) -> Result<Self, AlphaError> {
        let conn = Connection::open(db_path).map_err(|e| {
            AlphaError::Database(format!("Failed to open memory DB: {e}"))
        })?;

        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA busy_timeout=5000;",
        )
        .map_err(|e| AlphaError::Database(format!("Failed to set pragmas: {e}")))?;

        conn.execute_batch(DDL)
            .map_err(|e| AlphaError::Database(format!("Failed to create tables: {e}")))?;

        info!(
            path = %db_path.display(),
            expected_dimension,
            "Memory store opened"
        );

        Ok(Self {
            conn: Mutex::new(conn),
            expected_dimension,
        })
    }

    /// Store a memory record with its embedding.
    ///
    /// The embedding is serialized as a packed little-endian `f32` BLOB.
    /// If `expected_dimension > 0` and the embedding is non-empty, the
    /// dimension is validated before storage (Sprint 2 Amendment §2).
    pub fn store_with_embedding(&self, record: &MemoryRecord) -> Result<AlphaId, AlphaError> {
        // Validate dimension if configured.
        if self.expected_dimension > 0 {
            validate_embedding_dimension(&record.embedding, self.expected_dimension)?;
        }

        let conn = self.conn.lock().map_err(|e| {
            AlphaError::Database(format!("Failed to acquire lock: {e}"))
        })?;

        let memory_type = serde_json::to_string(&record.memory_type)?;
        let memory_type = memory_type.trim_matches('"');
        let embedding_bytes = embedding_to_bytes(&record.embedding);
        let associations_json = serde_json::to_string(&record.associations)?;
        let tags_json = serde_json::to_string(&record.tags)?;
        let governance = serde_json::to_string(&record.governance_state)?;
        let governance = governance.trim_matches('"');
        let metadata_json = serde_json::to_string(&record.metadata)?;
        let last_accessed = record.last_accessed.to_rfc3339();
        let created_at = record.created_at.to_rfc3339();

        conn.execute(
            "INSERT INTO memories
                (id, memory_type, content, embedding, importance, access_count,
                 last_accessed, created_at, source, associations, tags,
                 confidence, decay_rate, governance_state, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            rusqlite::params![
                record.id.to_string(),
                memory_type,
                record.content,
                embedding_bytes,
                record.importance,
                record.access_count,
                last_accessed,
                created_at,
                record.source,
                associations_json,
                tags_json,
                record.confidence,
                record.decay_rate,
                governance,
                metadata_json,
            ],
        )
        .map_err(|e| AlphaError::Database(format!("Failed to store memory: {e}")))?;

        debug!(id = %record.id, memory_type = %memory_type, "Memory stored");
        Ok(record.id)
    }

    /// Retrieve a memory by ID.
    pub fn get(&self, id: &AlphaId) -> Result<Option<MemoryRecord>, AlphaError> {
        let conn = self.conn.lock().map_err(|e| {
            AlphaError::Database(format!("Failed to acquire lock: {e}"))
        })?;

        let mut stmt = conn
            .prepare(
                "SELECT id, memory_type, content, embedding, importance, access_count,
                        last_accessed, created_at, source, associations, tags,
                        confidence, decay_rate, governance_state, metadata
                 FROM memories WHERE id = ?1",
            )
            .map_err(|e| AlphaError::Database(format!("Failed to prepare query: {e}")))?;

        let result = stmt
            .query_row(rusqlite::params![id.to_string()], |row| {
                Ok(row_to_memory(row))
            })
            .optional()
            .map_err(|e| AlphaError::Database(format!("Failed to get memory: {e}")))?;

        match result {
            Some(record) => Ok(Some(record?)),
            None => Ok(None),
        }
    }

    /// Delete a memory by ID. Returns `true` if deleted, `false` if not found.
    pub fn delete(&self, id: &AlphaId) -> Result<bool, AlphaError> {
        let conn = self.conn.lock().map_err(|e| {
            AlphaError::Database(format!("Failed to acquire lock: {e}"))
        })?;

        let rows = conn
            .execute(
                "DELETE FROM memories WHERE id = ?1",
                rusqlite::params![id.to_string()],
            )
            .map_err(|e| AlphaError::Database(format!("Failed to delete memory: {e}")))?;

        if rows > 0 {
            debug!(id = %id, "Memory deleted");
        }
        Ok(rows > 0)
    }

    /// List memories with optional filters and pagination.
    ///
    /// - `memory_type`: filter by type (episodic/semantic/procedural)
    /// - `governance_state`: filter by governance state
    /// - `limit` / `offset`: pagination
    pub fn list(
        &self,
        memory_type: Option<&MemoryType>,
        governance_state: Option<&GovernanceState>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<MemoryRecord>, AlphaError> {
        let conn = self.conn.lock().map_err(|e| {
            AlphaError::Database(format!("Failed to acquire lock: {e}"))
        })?;

        let mut sql = String::from(
            "SELECT id, memory_type, content, embedding, importance, access_count,
                    last_accessed, created_at, source, associations, tags,
                    confidence, decay_rate, governance_state, metadata
             FROM memories",
        );

        let mut conditions: Vec<String> = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut param_idx = 1;

        if let Some(mt) = memory_type {
            let mt_str = serde_json::to_string(mt)
                .unwrap_or_default()
                .trim_matches('"')
                .to_string();
            conditions.push(format!("memory_type = ?{param_idx}"));
            params.push(Box::new(mt_str));
            param_idx += 1;
        }

        if let Some(gs) = governance_state {
            let gs_str = serde_json::to_string(gs)
                .unwrap_or_default()
                .trim_matches('"')
                .to_string();
            conditions.push(format!("governance_state = ?{param_idx}"));
            params.push(Box::new(gs_str));
            param_idx += 1;
        }

        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }

        sql.push_str(&format!(
            " ORDER BY created_at DESC LIMIT ?{} OFFSET ?{}",
            param_idx,
            param_idx + 1
        ));
        params.push(Box::new(limit));
        params.push(Box::new(offset));

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AlphaError::Database(format!("Failed to prepare list query: {e}")))?;

        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();

        let rows = stmt
            .query_map(param_refs.as_slice(), |row| Ok(row_to_memory(row)))
            .map_err(|e| AlphaError::Database(format!("Failed to list memories: {e}")))?;

        let mut records = Vec::new();
        for row_result in rows {
            let record = row_result
                .map_err(|e| AlphaError::Database(format!("Row error: {e}")))?;
            records.push(record?);
        }

        Ok(records)
    }

    /// Count memories, optionally filtered by type and/or governance state.
    pub fn count(
        &self,
        memory_type: Option<&MemoryType>,
        governance_state: Option<&GovernanceState>,
    ) -> Result<u64, AlphaError> {
        let conn = self.conn.lock().map_err(|e| {
            AlphaError::Database(format!("Failed to acquire lock: {e}"))
        })?;

        let mut sql = String::from("SELECT COUNT(*) FROM memories");
        let mut conditions: Vec<String> = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut param_idx = 1;

        if let Some(mt) = memory_type {
            let mt_str = serde_json::to_string(mt)
                .unwrap_or_default()
                .trim_matches('"')
                .to_string();
            conditions.push(format!("memory_type = ?{param_idx}"));
            params.push(Box::new(mt_str));
            param_idx += 1;
        }

        if let Some(gs) = governance_state {
            let gs_str = serde_json::to_string(gs)
                .unwrap_or_default()
                .trim_matches('"')
                .to_string();
            conditions.push(format!("governance_state = ?{param_idx}"));
            params.push(Box::new(gs_str));
            let _ = param_idx; // suppress unused warning
        }

        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }

        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();

        let count: u64 = conn
            .query_row(&sql, param_refs.as_slice(), |row| row.get(0))
            .map_err(|e| AlphaError::Database(format!("Failed to count: {e}")))?;

        Ok(count)
    }

    /// Transition a memory's governance state.
    ///
    /// Used for lifecycle management: active → reference → archived → deprecated.
    pub fn set_governance(
        &self,
        id: &AlphaId,
        state: &GovernanceState,
    ) -> Result<(), AlphaError> {
        let conn = self.conn.lock().map_err(|e| {
            AlphaError::Database(format!("Failed to acquire lock: {e}"))
        })?;

        let gs_str = serde_json::to_string(state)
            .unwrap_or_default()
            .trim_matches('"')
            .to_string();

        let rows = conn
            .execute(
                "UPDATE memories SET governance_state = ?1 WHERE id = ?2",
                rusqlite::params![gs_str, id.to_string()],
            )
            .map_err(|e| AlphaError::Database(format!("Failed to update governance: {e}")))?;

        if rows == 0 {
            return Err(AlphaError::NotFound {
                entity: "MemoryRecord".to_string(),
                id: id.to_string(),
            });
        }

        debug!(id = %id, state = %gs_str, "Governance state updated");
        Ok(())
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

pub(crate) fn row_to_memory(row: &rusqlite::Row<'_>) -> Result<MemoryRecord, AlphaError> {
    let id_str: String = row.get(0).map_err(|e| AlphaError::Database(e.to_string()))?;
    let id: AlphaId = id_str
        .parse()
        .map_err(|e| AlphaError::Database(format!("Invalid UUID: {e}")))?;

    let type_str: String = row.get(1).map_err(|e| AlphaError::Database(e.to_string()))?;
    let memory_type: MemoryType = serde_json::from_str(&format!("\"{type_str}\""))?;

    let content: String = row.get(2).map_err(|e| AlphaError::Database(e.to_string()))?;

    let embedding_bytes: Vec<u8> = row.get(3).map_err(|e| AlphaError::Database(e.to_string()))?;
    let embedding = bytes_to_embedding(&embedding_bytes)?;

    let importance: f32 = row.get(4).map_err(|e| AlphaError::Database(e.to_string()))?;
    let access_count: u32 = row.get(5).map_err(|e| AlphaError::Database(e.to_string()))?;

    let last_accessed_str: String = row.get(6).map_err(|e| AlphaError::Database(e.to_string()))?;
    let last_accessed: Timestamp = chrono::DateTime::parse_from_rfc3339(&last_accessed_str)
        .map_err(|e| AlphaError::Database(format!("Invalid timestamp: {e}")))?
        .with_timezone(&chrono::Utc);

    let created_at_str: String = row.get(7).map_err(|e| AlphaError::Database(e.to_string()))?;
    let created_at: Timestamp = chrono::DateTime::parse_from_rfc3339(&created_at_str)
        .map_err(|e| AlphaError::Database(format!("Invalid timestamp: {e}")))?
        .with_timezone(&chrono::Utc);

    let source: String = row.get(8).map_err(|e| AlphaError::Database(e.to_string()))?;

    let associations_str: String = row.get(9).map_err(|e| AlphaError::Database(e.to_string()))?;
    let associations: Vec<AlphaId> = serde_json::from_str(&associations_str)?;

    let tags_str: String = row.get(10).map_err(|e| AlphaError::Database(e.to_string()))?;
    let tags: Vec<String> = serde_json::from_str(&tags_str)?;

    let confidence: f32 = row.get(11).map_err(|e| AlphaError::Database(e.to_string()))?;
    let decay_rate: f32 = row.get(12).map_err(|e| AlphaError::Database(e.to_string()))?;

    let governance_str: String = row.get(13).map_err(|e| AlphaError::Database(e.to_string()))?;
    let governance_state: GovernanceState =
        serde_json::from_str(&format!("\"{governance_str}\""))?;

    let metadata_str: String = row.get(14).map_err(|e| AlphaError::Database(e.to_string()))?;
    let metadata: JsonValue = serde_json::from_str(&metadata_str)?;

    Ok(MemoryRecord {
        id,
        memory_type,
        content,
        embedding,
        importance,
        access_count,
        last_accessed,
        created_at,
        source,
        associations,
        tags,
        confidence,
        decay_rate,
        governance_state,
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
