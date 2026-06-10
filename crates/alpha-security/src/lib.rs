//! # alpha-security
//!
//! Security Gate for Project Alpha.
//!
//! The Security Gate is the policy enforcement layer. Every action that
//! could affect the system, user data, or external resources must be
//! evaluated by the Security Gate before execution.
//!
//! ## Sprint 1: AllowAllGate
//!
//! Sprint 1 provides an [`AllowAllGate`] stub that:
//! - Approves every request.
//! - Logs every evaluation to the `audit_log` SQLite table.
//! - Implements the [`SecurityGate`] trait.
//!
//! Future sprints will implement policy-based evaluation, trust levels,
//! and user approval workflows.

pub mod audit;
pub mod types;

#[cfg(test)]
mod tests;

use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;
use tracing::{debug, info};

use alpha_common::error::AlphaError;
use alpha_common::types::new_id;

pub use types::{ActionRequest, AuditEntry, SecurityDecision};

// ── SecurityGate Trait ──

/// Trait that all Security Gate implementations must satisfy.
///
/// Every service that performs actions must submit an [`ActionRequest`]
/// to the Security Gate and respect the resulting [`SecurityDecision`].
pub trait SecurityGate: Send + Sync {
    /// Evaluate whether an action should be allowed.
    ///
    /// Returns a [`SecurityDecision`] indicating approval, denial,
    /// or a request for user approval.
    fn evaluate(&self, request: &ActionRequest) -> SecurityDecision;
}

// ── AllowAllGate (Sprint 1 Stub) ──

/// Sprint 1 stub Security Gate: approves everything, logs everything.
///
/// This gate always returns [`SecurityDecision::Approved`] but faithfully
/// records every evaluation in the audit log for future analysis.
///
/// Thread-safe: the internal SQLite connection is protected by a `Mutex`.
pub struct AllowAllGate {
    conn: Mutex<Connection>,
}

impl AllowAllGate {
    /// Open or create the audit database.
    ///
    /// - Creates the database file and parent directories.
    /// - Enables WAL journal mode.
    /// - Creates the `audit_log` table and indexes.
    pub fn open(audit_db_path: &Path) -> Result<Self, AlphaError> {
        // Ensure parent directory exists.
        if let Some(parent) = audit_db_path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    AlphaError::Config(format!(
                        "Failed to create audit directory '{}': {}",
                        parent.display(),
                        e
                    ))
                })?;
            }
        }

        let conn = Connection::open(audit_db_path).map_err(|e| {
            AlphaError::Database(format!(
                "Failed to open audit database at '{}': {}",
                audit_db_path.display(),
                e
            ))
        })?;

        // Enable WAL mode.
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;",
        )
        .map_err(|e| AlphaError::Database(format!("Failed to set audit pragmas: {}", e)))?;

        // Create tables.
        conn.execute_batch(audit::DDL)
            .map_err(|e| {
                AlphaError::Database(format!("Failed to create audit_log table: {}", e))
            })?;

        info!(path = %audit_db_path.display(), "Security gate opened (AllowAll mode)");

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Retrieve an audit entry by ID.
    pub fn get_entry(&self, id: &alpha_common::types::AlphaId) -> Result<Option<AuditEntry>, AlphaError> {
        let conn = self.conn.lock().map_err(|e| {
            AlphaError::Other(format!("Mutex poisoned: {}", e))
        })?;
        audit::get_entry(&conn, id)
    }

    /// Retrieve all audit entries.
    pub fn get_all(&self) -> Result<Vec<AuditEntry>, AlphaError> {
        let conn = self.conn.lock().map_err(|e| {
            AlphaError::Other(format!("Mutex poisoned: {}", e))
        })?;
        audit::get_all(&conn)
    }

    /// Get the count of audit entries.
    pub fn count(&self) -> Result<u64, AlphaError> {
        let conn = self.conn.lock().map_err(|e| {
            AlphaError::Other(format!("Mutex poisoned: {}", e))
        })?;
        audit::count(&conn)
    }
}

impl SecurityGate for AllowAllGate {
    /// Evaluate an action request.
    ///
    /// **Sprint 1 behavior**: Always approves. Always logs.
    fn evaluate(&self, request: &ActionRequest) -> SecurityDecision {
        let decision = SecurityDecision::Approved {
            id: new_id(),
            reason: "Sprint 1: AllowAllGate — all actions approved".to_string(),
        };

        // Create audit entry.
        let entry = AuditEntry {
            id: new_id(),
            request: request.clone(),
            decision: decision.clone(),
            timestamp: alpha_common::types::now(),
        };

        // Log to audit database. If logging fails, log the error but
        // still return the decision — fail-open in Sprint 1.
        match self.conn.lock() {
            Ok(conn) => {
                if let Err(e) = audit::write_entry(&conn, &entry) {
                    tracing::error!(error = %e, "Failed to write audit entry — continuing");
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Mutex poisoned — audit entry not written");
            }
        }

        debug!(
            action_type = %request.action_type,
            agent = %request.agent,
            target = %request.target,
            decision = decision.label(),
            "Security evaluation"
        );

        decision
    }
}

impl alpha_common::traits::Service for AllowAllGate {
    fn name(&self) -> &str {
        "security-gate"
    }

    fn init(&mut self) -> Result<(), AlphaError> {
        // Already initialized in open().
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), AlphaError> {
        // Checkpoint WAL.
        let conn = self.conn.lock().map_err(|e| {
            AlphaError::Other(format!("Mutex poisoned on shutdown: {}", e))
        })?;
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|e| {
                AlphaError::Database(format!(
                    "Failed to checkpoint audit WAL on shutdown: {}",
                    e
                ))
            })?;
        info!("Security gate shutdown: WAL checkpointed");
        Ok(())
    }
}

impl alpha_common::traits::Explainable for AllowAllGate {
    fn explain(
        &self,
        _decision_id: alpha_common::types::AlphaId,
    ) -> Option<alpha_common::schemas::explainability::ExplainabilityRecord> {
        // Sprint 1 stub: no explainability records generated.
        None
    }
}
