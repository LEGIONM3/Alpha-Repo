//! Schema migration framework.
//!
//! Provides versioned SQL migrations per component. Each component tracks
//! its own migration version independently in the `migrations` table.
//!
//! ## Rules
//!
//! - Migrations are applied in version order (ascending).
//! - Already-applied migrations are skipped (idempotent).
//! - Each migration runs inside a transaction.
//! - A migration failure rolls back that migration only.

use rusqlite::{Connection, OptionalExtension, params};
use tracing::{debug, info, warn};

use alpha_common::error::AlphaError;
use alpha_common::types::now;

/// A single schema migration.
///
/// Migrations are defined as static data by each component and passed
/// to [`StateStore::migrate()`](crate::StateStore::migrate).
#[derive(Debug, Clone)]
pub struct Migration {
    /// Version number. Must be unique per component. Applied in ascending order.
    pub version: u32,
    /// Human-readable description of what this migration does.
    pub description: &'static str,
    /// Raw SQL to execute. May contain multiple statements separated by `;`.
    pub sql: &'static str,
}

/// Run all pending migrations for a component.
///
/// - Queries the `migrations` table for the highest applied version.
/// - Filters out already-applied migrations.
/// - Sorts remaining by version ascending.
/// - Applies each in a transaction.
pub(crate) fn run_migrations(
    conn: &Connection,
    component: &str,
    migrations: &[Migration],
) -> Result<(), AlphaError> {
    if migrations.is_empty() {
        debug!(component, "No migrations to run");
        return Ok(());
    }

    // Get the current highest applied version for this component.
    let current_version: u32 = conn
        .query_row(
            "SELECT MAX(version) FROM migrations WHERE component = ?1",
            params![component],
            |row| row.get::<_, Option<u32>>(0),
        )
        .optional()
        .map_err(|e| AlphaError::Database(format!("Failed to query migration version: {}", e)))?
        .flatten() // Option<Option<u32>> -> Option<u32>
        .unwrap_or(0);

    debug!(component, current_version, "Current migration version");

    // Collect and sort pending migrations.
    let mut pending: Vec<&Migration> = migrations
        .iter()
        .filter(|m| m.version > current_version)
        .collect();
    pending.sort_by_key(|m| m.version);

    if pending.is_empty() {
        debug!(component, "All migrations already applied");
        return Ok(());
    }

    info!(
        component,
        pending_count = pending.len(),
        "Applying migrations"
    );

    for migration in &pending {
        debug!(
            component,
            version = migration.version,
            description = migration.description,
            "Applying migration"
        );

        // Run migration SQL in a transaction.
        let tx = conn.unchecked_transaction().map_err(|e| {
            AlphaError::Database(format!(
                "Failed to begin transaction for migration v{}: {}",
                migration.version, e
            ))
        })?;

        tx.execute_batch(migration.sql).map_err(|e| {
            warn!(
                component,
                version = migration.version,
                error = %e,
                "Migration failed — rolling back"
            );
            AlphaError::Database(format!(
                "Migration v{} ('{}') failed: {}",
                migration.version, migration.description, e
            ))
        })?;

        // Record that this migration was applied.
        let applied_at = now().to_rfc3339();
        tx.execute(
            "INSERT INTO migrations (component, version, description, applied_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![component, migration.version, migration.description, applied_at],
        )
        .map_err(|e| {
            AlphaError::Database(format!(
                "Failed to record migration v{}: {}",
                migration.version, e
            ))
        })?;

        tx.commit().map_err(|e| {
            AlphaError::Database(format!(
                "Failed to commit migration v{}: {}",
                migration.version, e
            ))
        })?;

        info!(
            component,
            version = migration.version,
            description = migration.description,
            "Migration applied"
        );
    }

    info!(
        component,
        applied_count = pending.len(),
        "All migrations applied"
    );

    Ok(())
}
