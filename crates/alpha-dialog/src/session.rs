//! SQLite-backed session manager for dialog history.
//!
//! Manages conversation sessions and their turns with full persistence.
//! Each session tracks its lifecycle (active → closed) and maintains
//! an auto-incrementing turn index for ordering.

use std::path::Path;
use std::sync::Mutex;

use alpha_common::error::AlphaError;
use alpha_common::types::{new_id, now, AlphaId, JsonValue, Timestamp};
use rusqlite::Connection;
use tracing::{debug, info};

use crate::types::{DialogSession, DialogTurn, SessionStatus, TurnRole};

// ── DDL ──

const DDL: &str = "
CREATE TABLE IF NOT EXISTS sessions (
    id              TEXT PRIMARY KEY,
    title           TEXT NOT NULL DEFAULT '',
    status          TEXT NOT NULL DEFAULT 'active'
                    CHECK(status IN ('active', 'closed')),
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    turn_count      INTEGER NOT NULL DEFAULT 0,
    metadata        TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS turns (
    id              TEXT PRIMARY KEY,
    session_id      TEXT NOT NULL REFERENCES sessions(id),
    role            TEXT NOT NULL CHECK(role IN ('user', 'alpha')),
    content         TEXT NOT NULL,
    turn_index      INTEGER NOT NULL,
    tokens_used     INTEGER NOT NULL DEFAULT 0,
    model_used      TEXT NOT NULL DEFAULT '',
    duration_ms     INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL,
    metadata        TEXT NOT NULL DEFAULT '{}'
);

CREATE INDEX IF NOT EXISTS idx_turns_session
    ON turns(session_id, turn_index);
CREATE INDEX IF NOT EXISTS idx_sessions_status
    ON sessions(status);
CREATE INDEX IF NOT EXISTS idx_sessions_updated
    ON sessions(updated_at);
";

/// SQLite-backed session manager.
///
/// Manages conversation sessions and multi-turn dialog history.
pub struct SessionManager {
    conn: Mutex<Connection>,
}

impl SessionManager {
    /// Open or create the dialog database.
    pub fn open(db_path: &Path) -> Result<Self, AlphaError> {
        let conn = Connection::open(db_path).map_err(|e| {
            AlphaError::Database(format!("Failed to open dialog DB: {e}"))
        })?;

        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA busy_timeout=5000;",
        )
        .map_err(|e| AlphaError::Database(format!("Failed to set pragmas: {e}")))?;

        conn.execute_batch(DDL)
            .map_err(|e| AlphaError::Database(format!("Failed to create tables: {e}")))?;

        info!(path = %db_path.display(), "Dialog session manager opened");

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Create a new conversation session. Returns the session ID.
    pub fn create_session(&self) -> Result<AlphaId, AlphaError> {
        let conn = self.lock()?;
        let id = new_id();
        let ts = now().to_rfc3339();

        conn.execute(
            "INSERT INTO sessions (id, title, status, created_at, updated_at, turn_count, metadata)
             VALUES (?1, '', 'active', ?2, ?2, 0, '{}')",
            rusqlite::params![id.to_string(), ts],
        )
        .map_err(|e| AlphaError::Database(format!("Failed to create session: {e}")))?;

        debug!(session_id = %id, "Session created");
        Ok(id)
    }

    /// Get a session by ID.
    pub fn get_session(&self, id: &AlphaId) -> Result<Option<DialogSession>, AlphaError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, title, status, created_at, updated_at, turn_count, metadata
                 FROM sessions WHERE id = ?1",
            )
            .map_err(|e| AlphaError::Database(format!("Failed to prepare query: {e}")))?;

        let result = stmt
            .query_row(rusqlite::params![id.to_string()], |row| {
                Ok(row_to_session(row))
            })
            .optional()
            .map_err(|e| AlphaError::Database(format!("Failed to get session: {e}")))?;

        match result {
            Some(session) => Ok(Some(session?)),
            None => Ok(None),
        }
    }

    /// Close a session (sets status to 'closed').
    pub fn close_session(&self, id: &AlphaId) -> Result<(), AlphaError> {
        let conn = self.lock()?;
        let ts = now().to_rfc3339();

        let rows = conn
            .execute(
                "UPDATE sessions SET status = 'closed', updated_at = ?1 WHERE id = ?2",
                rusqlite::params![ts, id.to_string()],
            )
            .map_err(|e| AlphaError::Database(format!("Failed to close session: {e}")))?;

        if rows == 0 {
            return Err(AlphaError::NotFound {
                entity: "DialogSession".to_string(),
                id: id.to_string(),
            });
        }

        debug!(session_id = %id, "Session closed");
        Ok(())
    }

    /// Get the current active session, or create one if none exists.
    pub fn get_or_create_active(&self) -> Result<DialogSession, AlphaError> {
        let conn = self.lock()?;

        // Try to find the most recently updated active session.
        let mut stmt = conn
            .prepare(
                "SELECT id, title, status, created_at, updated_at, turn_count, metadata
                 FROM sessions WHERE status = 'active'
                 ORDER BY updated_at DESC LIMIT 1",
            )
            .map_err(|e| AlphaError::Database(format!("Failed to prepare query: {e}")))?;

        let result = stmt
            .query_row([], |row| Ok(row_to_session(row)))
            .optional()
            .map_err(|e| AlphaError::Database(format!("Failed to query active session: {e}")))?;

        if let Some(session) = result {
            return session;
        }

        // No active session — create one.
        let id = new_id();
        let ts = now().to_rfc3339();

        conn.execute(
            "INSERT INTO sessions (id, title, status, created_at, updated_at, turn_count, metadata)
             VALUES (?1, '', 'active', ?2, ?2, 0, '{}')",
            rusqlite::params![id.to_string(), ts],
        )
        .map_err(|e| AlphaError::Database(format!("Failed to create session: {e}")))?;

        debug!(session_id = %id, "Active session created via get_or_create");

        // Fetch and return the newly created session.
        let mut stmt2 = conn
            .prepare(
                "SELECT id, title, status, created_at, updated_at, turn_count, metadata
                 FROM sessions WHERE id = ?1",
            )
            .map_err(|e| AlphaError::Database(format!("Failed to prepare query: {e}")))?;

        stmt2
            .query_row(rusqlite::params![id.to_string()], |row| {
                Ok(row_to_session(row))
            })
            .map_err(|e| AlphaError::Database(format!("Failed to read created session: {e}")))?
    }

    /// Add a user turn to a session.
    pub fn add_user_turn(
        &self,
        session_id: &AlphaId,
        content: &str,
    ) -> Result<DialogTurn, AlphaError> {
        self.add_turn(session_id, TurnRole::User, content, "", 0, 0)
    }

    /// Add an Alpha response turn to a session.
    pub fn add_alpha_turn(
        &self,
        session_id: &AlphaId,
        content: &str,
        model_used: &str,
        tokens_used: u32,
        duration_ms: u64,
    ) -> Result<DialogTurn, AlphaError> {
        self.add_turn(
            session_id,
            TurnRole::Alpha,
            content,
            model_used,
            tokens_used,
            duration_ms,
        )
    }

    /// Get recent turns for a session (most recent first).
    pub fn get_recent_turns(
        &self,
        session_id: &AlphaId,
        limit: u32,
    ) -> Result<Vec<DialogTurn>, AlphaError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, session_id, role, content, turn_index,
                        tokens_used, model_used, duration_ms, created_at, metadata
                 FROM turns WHERE session_id = ?1
                 ORDER BY turn_index DESC LIMIT ?2",
            )
            .map_err(|e| AlphaError::Database(format!("Failed to prepare query: {e}")))?;

        let rows = stmt
            .query_map(
                rusqlite::params![session_id.to_string(), limit],
                |row| Ok(row_to_turn(row)),
            )
            .map_err(|e| AlphaError::Database(format!("Failed to query turns: {e}")))?;

        let mut turns = Vec::new();
        for row_result in rows {
            let turn = row_result
                .map_err(|e| AlphaError::Database(format!("Row error: {e}")))?;
            turns.push(turn?);
        }

        Ok(turns)
    }

    /// Get all turns for a session (chronological order).
    pub fn get_turns(&self, session_id: &AlphaId) -> Result<Vec<DialogTurn>, AlphaError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, session_id, role, content, turn_index,
                        tokens_used, model_used, duration_ms, created_at, metadata
                 FROM turns WHERE session_id = ?1
                 ORDER BY turn_index ASC",
            )
            .map_err(|e| AlphaError::Database(format!("Failed to prepare query: {e}")))?;

        let rows = stmt
            .query_map(rusqlite::params![session_id.to_string()], |row| {
                Ok(row_to_turn(row))
            })
            .map_err(|e| AlphaError::Database(format!("Failed to query turns: {e}")))?;

        let mut turns = Vec::new();
        for row_result in rows {
            let turn = row_result
                .map_err(|e| AlphaError::Database(format!("Row error: {e}")))?;
            turns.push(turn?);
        }

        Ok(turns)
    }

    /// List sessions (most recently updated first).
    pub fn list_sessions(
        &self,
        status: Option<&SessionStatus>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<DialogSession>, AlphaError> {
        let conn = self.lock()?;

        let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
            if let Some(st) = status {
                (
                    "SELECT id, title, status, created_at, updated_at, turn_count, metadata
                     FROM sessions WHERE status = ?1
                     ORDER BY updated_at DESC LIMIT ?2 OFFSET ?3"
                        .to_string(),
                    vec![
                        Box::new(st.as_str().to_string()) as Box<dyn rusqlite::types::ToSql>,
                        Box::new(limit),
                        Box::new(offset),
                    ],
                )
            } else {
                (
                    "SELECT id, title, status, created_at, updated_at, turn_count, metadata
                     FROM sessions
                     ORDER BY updated_at DESC LIMIT ?1 OFFSET ?2"
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
            .query_map(param_refs.as_slice(), |row| Ok(row_to_session(row)))
            .map_err(|e| AlphaError::Database(format!("Failed to list sessions: {e}")))?;

        let mut sessions = Vec::new();
        for row_result in rows {
            let session = row_result
                .map_err(|e| AlphaError::Database(format!("Row error: {e}")))?;
            sessions.push(session?);
        }

        Ok(sessions)
    }

    /// Set session title.
    pub fn set_title(
        &self,
        session_id: &AlphaId,
        title: &str,
    ) -> Result<(), AlphaError> {
        let conn = self.lock()?;
        let ts = now().to_rfc3339();

        let rows = conn
            .execute(
                "UPDATE sessions SET title = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![title, ts, session_id.to_string()],
            )
            .map_err(|e| AlphaError::Database(format!("Failed to set title: {e}")))?;

        if rows == 0 {
            return Err(AlphaError::NotFound {
                entity: "DialogSession".to_string(),
                id: session_id.to_string(),
            });
        }

        debug!(session_id = %session_id, title, "Session title set");
        Ok(())
    }

    /// Total session count, optionally filtered by status.
    pub fn count(&self, status: Option<&SessionStatus>) -> Result<u64, AlphaError> {
        let conn = self.lock()?;

        let (sql, params): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) =
            if let Some(st) = status {
                (
                    "SELECT COUNT(*) FROM sessions WHERE status = ?1",
                    vec![Box::new(st.as_str().to_string()) as Box<dyn rusqlite::types::ToSql>],
                )
            } else {
                ("SELECT COUNT(*) FROM sessions", vec![])
            };

        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();

        let count: u64 = conn
            .query_row(sql, param_refs.as_slice(), |row| row.get(0))
            .map_err(|e| AlphaError::Database(format!("Failed to count sessions: {e}")))?;

        Ok(count)
    }

    /// Check whether WAL mode is enabled (for testing).
    pub fn is_wal_mode(&self) -> Result<bool, AlphaError> {
        let conn = self.lock()?;
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .map_err(|e| AlphaError::Database(format!("Failed to check journal mode: {e}")))?;
        Ok(mode.to_lowercase() == "wal")
    }

    // ── Private Helpers ──

    /// Acquire the connection lock.
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, AlphaError> {
        self.conn.lock().map_err(|e| {
            AlphaError::Database(format!("Failed to acquire lock: {e}"))
        })
    }

    /// Add a turn (shared logic for user and alpha turns).
    fn add_turn(
        &self,
        session_id: &AlphaId,
        role: TurnRole,
        content: &str,
        model_used: &str,
        tokens_used: u32,
        duration_ms: u64,
    ) -> Result<DialogTurn, AlphaError> {
        let conn = self.lock()?;
        let turn_id = new_id();
        let created_at = now();
        let created_at_str = created_at.to_rfc3339();
        let session_id_str = session_id.to_string();

        // Get current turn_count for this session to determine the next turn_index.
        let turn_count: u32 = conn
            .query_row(
                "SELECT turn_count FROM sessions WHERE id = ?1",
                rusqlite::params![session_id_str],
                |row| row.get(0),
            )
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("no rows") {
                    AlphaError::NotFound {
                        entity: "DialogSession".to_string(),
                        id: session_id_str.clone(),
                    }
                } else {
                    AlphaError::Database(format!("Failed to get turn count: {e}"))
                }
            })?;

        let turn_index = turn_count;

        // Insert the turn.
        conn.execute(
            "INSERT INTO turns (id, session_id, role, content, turn_index,
                                tokens_used, model_used, duration_ms, created_at, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, '{}')",
            rusqlite::params![
                turn_id.to_string(),
                session_id_str,
                role.as_str(),
                content,
                turn_index,
                tokens_used,
                model_used,
                duration_ms as i64,
                created_at_str,
            ],
        )
        .map_err(|e| AlphaError::Database(format!("Failed to insert turn: {e}")))?;

        // Update session: increment turn_count and update updated_at.
        conn.execute(
            "UPDATE sessions SET turn_count = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![turn_index + 1, created_at_str, session_id_str],
        )
        .map_err(|e| AlphaError::Database(format!("Failed to update session: {e}")))?;

        debug!(
            turn_id = %turn_id,
            session_id = %session_id,
            role = role.as_str(),
            turn_index,
            "Turn added"
        );

        Ok(DialogTurn {
            id: turn_id,
            session_id: *session_id,
            role,
            content: content.to_string(),
            turn_index,
            tokens_used,
            model_used: model_used.to_string(),
            duration_ms,
            created_at,
            metadata: JsonValue::Null,
        })
    }
}

// ── Row Deserialization ──

/// Deserialize a session row.
fn row_to_session(row: &rusqlite::Row<'_>) -> Result<DialogSession, AlphaError> {
    let id_str: String = row.get(0).map_err(|e| AlphaError::Database(e.to_string()))?;
    let id: AlphaId = id_str
        .parse()
        .map_err(|e| AlphaError::Database(format!("Invalid UUID: {e}")))?;

    let title: String = row.get(1).map_err(|e| AlphaError::Database(e.to_string()))?;

    let status_str: String = row.get(2).map_err(|e| AlphaError::Database(e.to_string()))?;
    let status = SessionStatus::parse_str(&status_str).ok_or_else(|| {
        AlphaError::Database(format!("Invalid session status: {status_str}"))
    })?;

    let created_at = parse_timestamp(row, 3)?;
    let updated_at = parse_timestamp(row, 4)?;

    let turn_count: u32 = row.get(5).map_err(|e| AlphaError::Database(e.to_string()))?;

    let metadata_str: String = row.get(6).map_err(|e| AlphaError::Database(e.to_string()))?;
    let metadata: JsonValue = serde_json::from_str(&metadata_str)?;

    Ok(DialogSession {
        id,
        title,
        status,
        created_at,
        updated_at,
        turn_count,
        metadata,
    })
}

/// Deserialize a turn row.
fn row_to_turn(row: &rusqlite::Row<'_>) -> Result<DialogTurn, AlphaError> {
    let id_str: String = row.get(0).map_err(|e| AlphaError::Database(e.to_string()))?;
    let id: AlphaId = id_str
        .parse()
        .map_err(|e| AlphaError::Database(format!("Invalid UUID: {e}")))?;

    let session_id_str: String = row.get(1).map_err(|e| AlphaError::Database(e.to_string()))?;
    let session_id: AlphaId = session_id_str
        .parse()
        .map_err(|e| AlphaError::Database(format!("Invalid session UUID: {e}")))?;

    let role_str: String = row.get(2).map_err(|e| AlphaError::Database(e.to_string()))?;
    let role = TurnRole::parse_str(&role_str).ok_or_else(|| {
        AlphaError::Database(format!("Invalid turn role: {role_str}"))
    })?;

    let content: String = row.get(3).map_err(|e| AlphaError::Database(e.to_string()))?;
    let turn_index: u32 = row.get(4).map_err(|e| AlphaError::Database(e.to_string()))?;
    let tokens_used: u32 = row.get(5).map_err(|e| AlphaError::Database(e.to_string()))?;
    let model_used: String = row.get(6).map_err(|e| AlphaError::Database(e.to_string()))?;
    let duration_ms_i64: i64 = row.get(7).map_err(|e| AlphaError::Database(e.to_string()))?;
    let duration_ms = duration_ms_i64 as u64;

    let created_at = parse_timestamp(row, 8)?;

    let metadata_str: String = row.get(9).map_err(|e| AlphaError::Database(e.to_string()))?;
    let metadata: JsonValue = serde_json::from_str(&metadata_str)?;

    Ok(DialogTurn {
        id,
        session_id,
        role,
        content,
        turn_index,
        tokens_used,
        model_used,
        duration_ms,
        created_at,
        metadata,
    })
}

/// Parse an RFC3339 timestamp from a row column.
fn parse_timestamp(row: &rusqlite::Row<'_>, idx: usize) -> Result<Timestamp, AlphaError> {
    let s: String = row.get(idx).map_err(|e| AlphaError::Database(e.to_string()))?;
    let ts = chrono::DateTime::parse_from_rfc3339(&s)
        .map_err(|e| AlphaError::Database(format!("Invalid timestamp: {e}")))?
        .with_timezone(&chrono::Utc);
    Ok(ts)
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
