//! Audit log persistence layer.
//!
//! Every security evaluation is recorded in the `audit_log` SQLite table.
//! This provides a complete, immutable record of all security decisions
//! for debugging, compliance, and future analytics.

use rusqlite::{Connection, OptionalExtension, params};

use alpha_common::error::AlphaError;
use alpha_common::types::{AlphaId, Timestamp};

use crate::types::{ActionRequest, AuditEntry, SecurityDecision};

/// SQL DDL for the audit log table and indexes.
pub(crate) const DDL: &str = "
    CREATE TABLE IF NOT EXISTS audit_log (
        id              TEXT PRIMARY KEY,
        action_type     TEXT NOT NULL,
        agent           TEXT NOT NULL,
        target          TEXT NOT NULL,
        risk_level      TEXT NOT NULL,
        parameters      TEXT NOT NULL,
        decision        TEXT NOT NULL,
        decision_reason TEXT NOT NULL,
        timestamp       TEXT NOT NULL
    );

    CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_log(timestamp);
    CREATE INDEX IF NOT EXISTS idx_audit_action ON audit_log(action_type);
";

/// Write an audit entry to the database.
pub(crate) fn write_entry(
    conn: &Connection,
    entry: &AuditEntry,
) -> Result<(), AlphaError> {
    let id = entry.id.to_string();
    let risk_level = serde_json::to_string(&entry.request.risk_level)
        .unwrap_or_else(|_| "\"low\"".to_string());
    // Strip quotes from serialized enum for cleaner storage.
    let risk_level = risk_level.trim_matches('"');
    let parameters = serde_json::to_string(&entry.request.parameters)?;
    let timestamp = entry.timestamp.to_rfc3339();

    conn.execute(
        "INSERT INTO audit_log
         (id, action_type, agent, target, risk_level, parameters, decision, decision_reason, timestamp)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            id,
            entry.request.action_type,
            entry.request.agent,
            entry.request.target,
            risk_level,
            parameters,
            entry.decision.label(),
            entry.decision.reason(),
            timestamp,
        ],
    )
    .map_err(|e| AlphaError::Database(format!("Failed to write audit entry: {}", e)))?;

    Ok(())
}

/// Retrieve an audit entry by ID.
pub(crate) fn get_entry(
    conn: &Connection,
    id: &AlphaId,
) -> Result<Option<AuditEntry>, AlphaError> {
    let id_str = id.to_string();

    let result = conn
        .query_row(
            "SELECT id, action_type, agent, target, risk_level, parameters,
                    decision, decision_reason, timestamp
             FROM audit_log WHERE id = ?1",
            params![id_str],
            |row| {
                Ok(AuditRow {
                    id: row.get(0)?,
                    action_type: row.get(1)?,
                    agent: row.get(2)?,
                    target: row.get(3)?,
                    risk_level: row.get(4)?,
                    parameters: row.get(5)?,
                    decision: row.get(6)?,
                    decision_reason: row.get(7)?,
                    timestamp: row.get(8)?,
                })
            },
        )
        .optional()
        .map_err(|e| AlphaError::Database(format!("Failed to get audit entry: {}", e)))?;

    match result {
        Some(row) => Ok(Some(row_to_entry(row)?)),
        None => Ok(None),
    }
}

/// Retrieve all audit entries, ordered by timestamp ascending.
pub(crate) fn get_all(conn: &Connection) -> Result<Vec<AuditEntry>, AlphaError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, action_type, agent, target, risk_level, parameters,
                    decision, decision_reason, timestamp
             FROM audit_log ORDER BY timestamp ASC",
        )
        .map_err(|e| AlphaError::Database(format!("Failed to prepare get_all: {}", e)))?;

    let rows = stmt
        .query_map([], |row| {
            Ok(AuditRow {
                id: row.get(0)?,
                action_type: row.get(1)?,
                agent: row.get(2)?,
                target: row.get(3)?,
                risk_level: row.get(4)?,
                parameters: row.get(5)?,
                decision: row.get(6)?,
                decision_reason: row.get(7)?,
                timestamp: row.get(8)?,
            })
        })
        .map_err(|e| AlphaError::Database(format!("Failed to query get_all: {}", e)))?;

    let mut entries = Vec::new();
    for row_result in rows {
        let row = row_result
            .map_err(|e| AlphaError::Database(format!("Failed to read audit row: {}", e)))?;
        entries.push(row_to_entry(row)?);
    }

    Ok(entries)
}

/// Get the count of audit entries.
pub(crate) fn count(conn: &Connection) -> Result<u64, AlphaError> {
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM audit_log", [], |row| row.get(0))
        .map_err(|e| AlphaError::Database(format!("Failed to count audit entries: {}", e)))?;

    Ok(count as u64)
}

// ── Internal helpers ──

/// Raw row from SQLite.
struct AuditRow {
    id: String,
    action_type: String,
    agent: String,
    target: String,
    risk_level: String,
    parameters: String,
    decision: String,
    decision_reason: String,
    timestamp: String,
}

/// Convert a raw row into an AuditEntry.
fn row_to_entry(row: AuditRow) -> Result<AuditEntry, AlphaError> {
    use alpha_common::schemas::goal::RiskLevel;
    use chrono::DateTime;

    let id: AlphaId = row.id.parse().map_err(|e| {
        AlphaError::Database(format!("Failed to parse audit id '{}': {}", row.id, e))
    })?;

    let timestamp: Timestamp = DateTime::parse_from_rfc3339(&row.timestamp)
        .map_err(|e| {
            AlphaError::Database(format!(
                "Failed to parse timestamp '{}': {}",
                row.timestamp, e
            ))
        })?
        .with_timezone(&chrono::Utc);

    // Parse risk level from string.
    let risk_level: RiskLevel = serde_json::from_str(&format!("\"{}\"", row.risk_level))
        .unwrap_or(RiskLevel::Low);

    let parameters: serde_json::Value = serde_json::from_str(&row.parameters)?;

    // Reconstruct the ActionRequest.
    let request = ActionRequest {
        id: alpha_common::types::new_id(), // Original request ID not stored separately
        action_type: row.action_type,
        agent: row.agent,
        target: row.target,
        risk_level,
        parameters,
        timestamp,
    };

    // Reconstruct the SecurityDecision.
    let decision_id = alpha_common::types::new_id();
    let decision = match row.decision.as_str() {
        "approved" => SecurityDecision::Approved {
            id: decision_id,
            reason: row.decision_reason,
        },
        "denied" => SecurityDecision::Denied {
            id: decision_id,
            reason: row.decision_reason,
        },
        "requires_approval" => SecurityDecision::RequiresApproval {
            id: decision_id,
            reason: row.decision_reason,
        },
        other => {
            return Err(AlphaError::Database(format!(
                "Unknown decision type: '{}'",
                other
            )));
        }
    };

    Ok(AuditEntry {
        id,
        request,
        decision,
        timestamp,
    })
}
