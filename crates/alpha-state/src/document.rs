//! Document storage operations.
//!
//! Provides collection-scoped JSON document storage backed by the `documents`
//! SQLite table. Each document is identified by a UUID within its collection.

use rusqlite::{Connection, OptionalExtension, params};

use alpha_common::error::AlphaError;
use alpha_common::types::{AlphaId, JsonValue, now};

/// Store a JSON document. Uses INSERT OR REPLACE (upsert).
///
/// If a document with the same collection + id already exists, it is overwritten
/// and `updated_at` is refreshed. The original `created_at` is preserved on update.
pub(crate) fn store_doc(
    conn: &Connection,
    collection: &str,
    id: &AlphaId,
    doc: &JsonValue,
) -> Result<(), AlphaError> {
    let id_str = id.to_string();
    let doc_str = serde_json::to_string(doc)?;
    let timestamp = now().to_rfc3339();

    conn.execute(
        "INSERT INTO documents (collection, id, doc, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(collection, id)
         DO UPDATE SET doc = excluded.doc, updated_at = excluded.updated_at",
        params![collection, id_str, doc_str, timestamp, timestamp],
    )
    .map_err(|e| AlphaError::Database(format!("store_doc failed: {}", e)))?;

    Ok(())
}

/// Get a document by collection + id. Returns None if not found.
pub(crate) fn get_doc(
    conn: &Connection,
    collection: &str,
    id: &AlphaId,
) -> Result<Option<JsonValue>, AlphaError> {
    let id_str = id.to_string();

    let result = conn
        .query_row(
            "SELECT doc FROM documents WHERE collection = ?1 AND id = ?2",
            params![collection, id_str],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| AlphaError::Database(format!("get_doc failed: {}", e)))?;

    match result {
        Some(doc_str) => {
            let doc: JsonValue = serde_json::from_str(&doc_str)?;
            Ok(Some(doc))
        }
        None => Ok(None),
    }
}

/// Delete a document by collection + id. Returns true if it existed.
pub(crate) fn delete_doc(
    conn: &Connection,
    collection: &str,
    id: &AlphaId,
) -> Result<bool, AlphaError> {
    let id_str = id.to_string();

    let rows_affected = conn
        .execute(
            "DELETE FROM documents WHERE collection = ?1 AND id = ?2",
            params![collection, id_str],
        )
        .map_err(|e| AlphaError::Database(format!("delete_doc failed: {}", e)))?;

    Ok(rows_affected > 0)
}

/// List all document IDs in a collection.
pub(crate) fn list_docs(
    conn: &Connection,
    collection: &str,
) -> Result<Vec<AlphaId>, AlphaError> {
    let mut stmt = conn
        .prepare("SELECT id FROM documents WHERE collection = ?1 ORDER BY created_at")
        .map_err(|e| AlphaError::Database(format!("list_docs prepare failed: {}", e)))?;

    let ids = stmt
        .query_map(params![collection], |row| {
            let id_str: String = row.get(0)?;
            // Parse UUID from the stored string.
            AlphaId::parse_str(&id_str).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })
        })
        .map_err(|e| AlphaError::Database(format!("list_docs query failed: {}", e)))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| AlphaError::Database(format!("list_docs collect failed: {}", e)))?;

    Ok(ids)
}
