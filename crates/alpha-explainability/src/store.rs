//! SQLite persistence layer for explainability records.

use std::path::Path;
use std::sync::Mutex;

use alpha_common::{AlphaError, AlphaId, ExplainabilityRecord};
use rusqlite::Connection;
use tracing::{debug, info};

/// SQLite-backed explainability record store.
///
/// Sprint 2: minimal persistence. Store, retrieve, and count only.
/// No explanation generation, no advanced querying.
pub struct ExplainabilityStore {
    conn: Mutex<Connection>,
}

impl ExplainabilityStore {
    /// Open (or create) the explainability database at the given path.
    ///
    /// Creates the `explanations` table if it does not exist.
    /// Enables WAL mode and sets pragmas for performance.
    pub fn open(db_path: &Path) -> Result<Self, AlphaError> {
        let conn = Connection::open(db_path).map_err(|e| {
            AlphaError::Database(format!("Failed to open explainability DB: {e}"))
        })?;

        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA busy_timeout=5000;",
        )
        .map_err(|e| AlphaError::Database(format!("Failed to set pragmas: {e}")))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS explanations (
                id                  TEXT PRIMARY KEY,
                explanation_type    TEXT NOT NULL,
                subject_id          TEXT NOT NULL,
                summary             TEXT NOT NULL,
                reasoning           TEXT NOT NULL DEFAULT '[]',
                factors             TEXT NOT NULL DEFAULT '[]',
                alternatives        TEXT NOT NULL DEFAULT '[]',
                evidence            TEXT NOT NULL DEFAULT '[]',
                confidence          REAL NOT NULL DEFAULT 1.0,
                trace_id            TEXT NOT NULL,
                created_at          TEXT NOT NULL,
                metadata            TEXT NOT NULL DEFAULT '{}'
            );

            CREATE INDEX IF NOT EXISTS idx_explanations_subject
                ON explanations(subject_id);
            CREATE INDEX IF NOT EXISTS idx_explanations_type
                ON explanations(explanation_type);",
        )
        .map_err(|e| AlphaError::Database(format!("Failed to create tables: {e}")))?;

        info!(path = %db_path.display(), "Explainability store opened");
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Store an explainability record.
    ///
    /// Serializes `reasoning`, `factors`, `alternatives`, and `evidence`
    /// as JSON text columns. Returns the record's ID.
    pub fn store(&self, record: &ExplainabilityRecord) -> Result<AlphaId, AlphaError> {
        let conn = self.conn.lock().map_err(|e| {
            AlphaError::Database(format!("Failed to acquire lock: {e}"))
        })?;

        let explanation_type = serde_json::to_string(&record.explanation_type)?;
        // serde_json wraps enum in quotes like `"model"` — strip them for cleaner storage.
        let explanation_type = explanation_type.trim_matches('"');

        let reasoning_json = serde_json::to_string(&record.reasoning)?;
        let factors_json = serde_json::to_string(&record.factors)?;
        let alternatives_json = serde_json::to_string(&record.alternatives)?;
        let evidence_json = serde_json::to_string(&record.evidence)?;
        let created_at = record.timestamp.to_rfc3339();

        conn.execute(
            "INSERT INTO explanations
                (id, explanation_type, subject_id, summary, reasoning,
                 factors, alternatives, evidence, confidence, trace_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                record.id.to_string(),
                explanation_type,
                record.subject_id.to_string(),
                record.summary,
                reasoning_json,
                factors_json,
                alternatives_json,
                evidence_json,
                record.confidence,
                record.trace_id.to_string(),
                created_at,
            ],
        )
        .map_err(|e| AlphaError::Database(format!("Failed to store explanation: {e}")))?;

        debug!(id = %record.id, subject = %record.subject_id, "Explanation stored");
        Ok(record.id)
    }

    /// Retrieve an explainability record by ID.
    ///
    /// Returns `None` if not found. Deserializes all JSON columns back
    /// into their typed fields.
    pub fn get(&self, id: &AlphaId) -> Result<Option<ExplainabilityRecord>, AlphaError> {
        let conn = self.conn.lock().map_err(|e| {
            AlphaError::Database(format!("Failed to acquire lock: {e}"))
        })?;

        let mut stmt = conn
            .prepare(
                "SELECT id, explanation_type, subject_id, summary, reasoning,
                        factors, alternatives, evidence, confidence, trace_id,
                        created_at
                 FROM explanations WHERE id = ?1",
            )
            .map_err(|e| AlphaError::Database(format!("Failed to prepare query: {e}")))?;

        let result = stmt
            .query_row(rusqlite::params![id.to_string()], |row| {
                Ok(row_to_record(row))
            })
            .optional()
            .map_err(|e| AlphaError::Database(format!("Failed to get explanation: {e}")))?;

        match result {
            Some(record) => Ok(Some(record?)),
            None => Ok(None),
        }
    }

    /// Count total explainability records.
    pub fn count(&self) -> Result<u64, AlphaError> {
        let conn = self.conn.lock().map_err(|e| {
            AlphaError::Database(format!("Failed to acquire lock: {e}"))
        })?;

        let count: u64 = conn
            .query_row("SELECT COUNT(*) FROM explanations", [], |row| row.get(0))
            .map_err(|e| AlphaError::Database(format!("Failed to count: {e}")))?;

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

/// Deserialize a SQLite row into an `ExplainabilityRecord`.
///
/// Column order:
/// `id, explanation_type, subject_id, summary, reasoning,
///  factors, alternatives, evidence, confidence, trace_id, created_at`
fn row_to_record(row: &rusqlite::Row<'_>) -> Result<ExplainabilityRecord, AlphaError> {
    let id_str: String = row.get(0).map_err(|e| AlphaError::Database(e.to_string()))?;
    let id: AlphaId = id_str
        .parse()
        .map_err(|e| AlphaError::Database(format!("Invalid UUID for id: {e}")))?;

    let type_str: String = row.get(1).map_err(|e| AlphaError::Database(e.to_string()))?;
    // Re-wrap in quotes for serde deserialization of the rename_all enum.
    let explanation_type = serde_json::from_str(&format!("\"{type_str}\""))?;

    let subject_str: String = row.get(2).map_err(|e| AlphaError::Database(e.to_string()))?;
    let subject_id: AlphaId = subject_str
        .parse()
        .map_err(|e| AlphaError::Database(format!("Invalid UUID for subject_id: {e}")))?;

    let summary: String = row.get(3).map_err(|e| AlphaError::Database(e.to_string()))?;

    let reasoning_str: String = row.get(4).map_err(|e| AlphaError::Database(e.to_string()))?;
    let reasoning: Vec<String> = serde_json::from_str(&reasoning_str)?;

    let factors_str: String = row.get(5).map_err(|e| AlphaError::Database(e.to_string()))?;
    let factors = serde_json::from_str(&factors_str)?;

    let alternatives_str: String = row.get(6).map_err(|e| AlphaError::Database(e.to_string()))?;
    let alternatives = serde_json::from_str(&alternatives_str)?;

    let evidence_str: String = row.get(7).map_err(|e| AlphaError::Database(e.to_string()))?;
    let evidence: Vec<AlphaId> = serde_json::from_str(&evidence_str)?;

    let confidence: f32 = row.get(8).map_err(|e| AlphaError::Database(e.to_string()))?;

    let trace_str: String = row.get(9).map_err(|e| AlphaError::Database(e.to_string()))?;
    let trace_id: AlphaId = trace_str
        .parse()
        .map_err(|e| AlphaError::Database(format!("Invalid UUID for trace_id: {e}")))?;

    let created_at_str: String = row.get(10).map_err(|e| AlphaError::Database(e.to_string()))?;
    let timestamp = chrono::DateTime::parse_from_rfc3339(&created_at_str)
        .map_err(|e| AlphaError::Database(format!("Invalid timestamp: {e}")))?
        .with_timezone(&chrono::Utc);

    Ok(ExplainabilityRecord {
        id,
        explanation_type,
        subject_id,
        summary,
        reasoning,
        factors,
        alternatives,
        evidence,
        confidence,
        trace_id,
        timestamp,
    })
}

/// Extension trait to convert `QueryReturnedNoRows` into `None`.
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
