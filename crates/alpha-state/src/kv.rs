//! Key-value storage operations.
//!
//! Provides namespaced key-value storage backed by the `kv` SQLite table.
//! All keys live within a namespace, providing natural isolation between
//! services without risk of key collisions.

use rusqlite::{Connection, OptionalExtension, params};

use alpha_common::error::AlphaError;
use alpha_common::types::now;

/// Set a key-value pair. Uses INSERT OR REPLACE (upsert).
pub(crate) fn set(
    conn: &Connection,
    namespace: &str,
    key: &str,
    value: &str,
) -> Result<(), AlphaError> {
    let updated_at = now().to_rfc3339();

    conn.execute(
        "INSERT INTO kv (namespace, key, value, updated_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(namespace, key)
         DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![namespace, key, value, updated_at],
    )
    .map_err(|e| AlphaError::Database(format!("kv set failed: {}", e)))?;

    Ok(())
}

/// Get a value by namespace + key. Returns None if not found.
pub(crate) fn get(
    conn: &Connection,
    namespace: &str,
    key: &str,
) -> Result<Option<String>, AlphaError> {
    let result = conn
        .query_row(
            "SELECT value FROM kv WHERE namespace = ?1 AND key = ?2",
            params![namespace, key],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| AlphaError::Database(format!("kv get failed: {}", e)))?;

    Ok(result)
}

/// Delete a key. Returns true if the key existed and was removed.
pub(crate) fn delete(
    conn: &Connection,
    namespace: &str,
    key: &str,
) -> Result<bool, AlphaError> {
    let rows_affected = conn
        .execute(
            "DELETE FROM kv WHERE namespace = ?1 AND key = ?2",
            params![namespace, key],
        )
        .map_err(|e| AlphaError::Database(format!("kv delete failed: {}", e)))?;

    Ok(rows_affected > 0)
}

/// List all keys in a given namespace.
pub(crate) fn list_keys(
    conn: &Connection,
    namespace: &str,
) -> Result<Vec<String>, AlphaError> {
    let mut stmt = conn
        .prepare("SELECT key FROM kv WHERE namespace = ?1 ORDER BY key")
        .map_err(|e| AlphaError::Database(format!("kv list_keys prepare failed: {}", e)))?;

    let keys = stmt
        .query_map(params![namespace], |row| row.get::<_, String>(0))
        .map_err(|e| AlphaError::Database(format!("kv list_keys query failed: {}", e)))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| AlphaError::Database(format!("kv list_keys collect failed: {}", e)))?;

    Ok(keys)
}
