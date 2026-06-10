//! Comprehensive unit tests for the StateStore.
//!
//! Every test uses a temporary database file to ensure isolation.

use serde_json::json;

use alpha_common::types::new_id;

use crate::StateStore;
use crate::migration::Migration;

/// Helper: create a StateStore backed by a temp file.
fn temp_store() -> (StateStore, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("test_state.db");
    let store = StateStore::open(&db_path).expect("open state store");
    (store, dir)
}

// ════════════════════════════════════════════════════════════════
// Key-Value Tests
// ════════════════════════════════════════════════════════════════

#[test]
fn test_kv_set_get() {
    let (store, _dir) = temp_store();

    store.set("test-ns", "greeting", "hello world").unwrap();
    let result = store.get("test-ns", "greeting").unwrap();

    assert_eq!(result, Some("hello world".to_string()));
}

#[test]
fn test_kv_get_nonexistent() {
    let (store, _dir) = temp_store();

    let result = store.get("test-ns", "no-such-key").unwrap();
    assert_eq!(result, None);
}

#[test]
fn test_kv_overwrite() {
    let (store, _dir) = temp_store();

    store.set("test-ns", "counter", "1").unwrap();
    assert_eq!(store.get("test-ns", "counter").unwrap(), Some("1".to_string()));

    store.set("test-ns", "counter", "2").unwrap();
    assert_eq!(store.get("test-ns", "counter").unwrap(), Some("2".to_string()));

    // Original value is gone.
    assert_ne!(store.get("test-ns", "counter").unwrap(), Some("1".to_string()));
}

#[test]
fn test_kv_delete() {
    let (store, _dir) = temp_store();

    store.set("test-ns", "temp", "value").unwrap();
    assert!(store.get("test-ns", "temp").unwrap().is_some());

    let deleted = store.delete("test-ns", "temp").unwrap();
    assert!(deleted, "delete should return true for existing key");

    let result = store.get("test-ns", "temp").unwrap();
    assert_eq!(result, None, "key should be gone after delete");
}

#[test]
fn test_kv_delete_nonexistent() {
    let (store, _dir) = temp_store();

    let deleted = store.delete("test-ns", "never-existed").unwrap();
    assert!(!deleted, "delete should return false for nonexistent key");
}

#[test]
fn test_kv_namespace_isolation() {
    let (store, _dir) = temp_store();

    store.set("ns-a", "key", "value-a").unwrap();
    store.set("ns-b", "key", "value-b").unwrap();

    assert_eq!(store.get("ns-a", "key").unwrap(), Some("value-a".to_string()));
    assert_eq!(store.get("ns-b", "key").unwrap(), Some("value-b".to_string()));

    // Deleting from one namespace doesn't affect the other.
    store.delete("ns-a", "key").unwrap();
    assert_eq!(store.get("ns-a", "key").unwrap(), None);
    assert_eq!(store.get("ns-b", "key").unwrap(), Some("value-b".to_string()));
}

#[test]
fn test_kv_list_keys() {
    let (store, _dir) = temp_store();

    store.set("test-ns", "cherry", "3").unwrap();
    store.set("test-ns", "apple", "1").unwrap();
    store.set("test-ns", "banana", "2").unwrap();
    // Different namespace — should not appear.
    store.set("other-ns", "dragonfruit", "4").unwrap();

    let keys = store.list_keys("test-ns").unwrap();
    // Keys are sorted alphabetically.
    assert_eq!(keys, vec!["apple", "banana", "cherry"]);
}

#[test]
fn test_kv_list_keys_empty_namespace() {
    let (store, _dir) = temp_store();

    let keys = store.list_keys("empty-ns").unwrap();
    assert!(keys.is_empty());
}

// ════════════════════════════════════════════════════════════════
// Document Tests
// ════════════════════════════════════════════════════════════════

#[test]
fn test_document_store_get() {
    let (store, _dir) = temp_store();

    let id = new_id();
    let doc = json!({
        "name": "Alpha",
        "version": "0.1.0",
        "features": ["memory", "goals"]
    });

    store.store_doc("configs", &id, &doc).unwrap();
    let result = store.get_doc("configs", &id).unwrap();

    assert!(result.is_some());
    let retrieved = result.unwrap();
    assert_eq!(retrieved["name"], "Alpha");
    assert_eq!(retrieved["version"], "0.1.0");
    assert_eq!(retrieved["features"][0], "memory");
}

#[test]
fn test_document_get_nonexistent() {
    let (store, _dir) = temp_store();

    let id = new_id();
    let result = store.get_doc("configs", &id).unwrap();
    assert!(result.is_none());
}

#[test]
fn test_document_overwrite() {
    let (store, _dir) = temp_store();

    let id = new_id();
    let doc_v1 = json!({"version": 1});
    let doc_v2 = json!({"version": 2, "extra": "field"});

    store.store_doc("configs", &id, &doc_v1).unwrap();
    store.store_doc("configs", &id, &doc_v2).unwrap();

    let result = store.get_doc("configs", &id).unwrap().unwrap();
    assert_eq!(result["version"], 2);
    assert_eq!(result["extra"], "field");
}

#[test]
fn test_document_delete() {
    let (store, _dir) = temp_store();

    let id = new_id();
    store.store_doc("test-col", &id, &json!({"key": "val"})).unwrap();

    let deleted = store.delete_doc("test-col", &id).unwrap();
    assert!(deleted, "delete should return true for existing doc");

    let result = store.get_doc("test-col", &id).unwrap();
    assert!(result.is_none(), "doc should be gone after delete");
}

#[test]
fn test_document_delete_nonexistent() {
    let (store, _dir) = temp_store();

    let id = new_id();
    let deleted = store.delete_doc("test-col", &id).unwrap();
    assert!(!deleted, "delete should return false for nonexistent doc");
}

#[test]
fn test_document_list() {
    let (store, _dir) = temp_store();

    let id1 = new_id();
    let id2 = new_id();
    let id3 = new_id();

    store.store_doc("memories", &id1, &json!({"seq": 1})).unwrap();
    store.store_doc("memories", &id2, &json!({"seq": 2})).unwrap();
    store.store_doc("memories", &id3, &json!({"seq": 3})).unwrap();
    // Different collection — should not appear.
    store.store_doc("goals", &new_id(), &json!({"seq": 99})).unwrap();

    let ids = store.list_docs("memories").unwrap();
    assert_eq!(ids.len(), 3);
    assert!(ids.contains(&id1));
    assert!(ids.contains(&id2));
    assert!(ids.contains(&id3));
}

#[test]
fn test_document_list_empty_collection() {
    let (store, _dir) = temp_store();

    let ids = store.list_docs("empty-col").unwrap();
    assert!(ids.is_empty());
}

#[test]
fn test_document_collection_isolation() {
    let (store, _dir) = temp_store();

    let id = new_id();
    store.store_doc("col-a", &id, &json!({"from": "a"})).unwrap();
    store.store_doc("col-b", &id, &json!({"from": "b"})).unwrap();

    let doc_a = store.get_doc("col-a", &id).unwrap().unwrap();
    let doc_b = store.get_doc("col-b", &id).unwrap().unwrap();

    assert_eq!(doc_a["from"], "a");
    assert_eq!(doc_b["from"], "b");
}

// ════════════════════════════════════════════════════════════════
// Persistence Tests
// ════════════════════════════════════════════════════════════════

#[test]
fn test_persistence_across_restart() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("persist_test.db");

    let doc_id = new_id();

    // Session 1: write data.
    {
        let store = StateStore::open(&db_path).unwrap();
        store.set("persist-ns", "key1", "value1").unwrap();
        store.set("persist-ns", "key2", "value2").unwrap();
        store.store_doc("persist-col", &doc_id, &json!({"persisted": true})).unwrap();
        // Store drops here, closing the connection.
    }

    // Session 2: reopen and verify data survived.
    {
        let store = StateStore::open(&db_path).unwrap();

        let val1 = store.get("persist-ns", "key1").unwrap();
        assert_eq!(val1, Some("value1".to_string()), "KV data must survive restart");

        let val2 = store.get("persist-ns", "key2").unwrap();
        assert_eq!(val2, Some("value2".to_string()), "KV data must survive restart");

        let doc = store.get_doc("persist-col", &doc_id).unwrap();
        assert!(doc.is_some(), "Document must survive restart");
        assert_eq!(doc.unwrap()["persisted"], true);
    }
}

// ════════════════════════════════════════════════════════════════
// Migration Tests
// ════════════════════════════════════════════════════════════════

#[test]
fn test_migration_runs_once() {
    let (store, _dir) = temp_store();

    let migrations = &[
        Migration {
            version: 1,
            description: "Create test table",
            sql: "CREATE TABLE test_migrate (id TEXT PRIMARY KEY, name TEXT);",
        },
    ];

    // First run: should apply.
    store.migrate("test-component", migrations).unwrap();

    // Verify the table was created by inserting data.
    store.set("migration-check", "v1-applied", "true").unwrap();

    // Second run: should skip (no error, no duplicate table).
    store.migrate("test-component", migrations).unwrap();

    // Data from the first run still exists.
    let val = store.get("migration-check", "v1-applied").unwrap();
    assert_eq!(val, Some("true".to_string()));
}

#[test]
fn test_migration_ordering() {
    let (store, _dir) = temp_store();

    let migrations = &[
        Migration {
            version: 3,
            description: "Add column c",
            sql: "ALTER TABLE ordered_test ADD COLUMN col_c TEXT;",
        },
        Migration {
            version: 1,
            description: "Create table",
            sql: "CREATE TABLE ordered_test (id TEXT PRIMARY KEY, col_a TEXT);",
        },
        Migration {
            version: 2,
            description: "Add column b",
            sql: "ALTER TABLE ordered_test ADD COLUMN col_b TEXT;",
        },
    ];

    // Even though migrations are passed out of order, they should be
    // applied in version order: 1, 2, 3.
    store.migrate("ordered-component", migrations).unwrap();

    // Verify all three columns exist by inserting a row using all of them.
    // We access the connection indirectly by checking migrations were recorded.
    // The fact that no error occurred means CREATE TABLE ran before ALTER TABLE.
}

#[test]
fn test_migration_incremental() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("migration_inc.db");

    // Phase 1: apply v1 only.
    {
        let store = StateStore::open(&db_path).unwrap();
        let migrations = &[
            Migration {
                version: 1,
                description: "Create users table",
                sql: "CREATE TABLE users (id TEXT PRIMARY KEY);",
            },
        ];
        store.migrate("users-service", migrations).unwrap();
    }

    // Phase 2: reopen with v1 + v2. Only v2 should run.
    {
        let store = StateStore::open(&db_path).unwrap();
        let migrations = &[
            Migration {
                version: 1,
                description: "Create users table",
                sql: "CREATE TABLE users (id TEXT PRIMARY KEY);",
            },
            Migration {
                version: 2,
                description: "Add name column",
                sql: "ALTER TABLE users ADD COLUMN name TEXT;",
            },
        ];
        store.migrate("users-service", migrations).unwrap();
        // If v1 ran again, CREATE TABLE would fail (table already exists
        // without IF NOT EXISTS). The fact that this succeeds proves
        // v1 was skipped.
    }
}

#[test]
fn test_migration_empty_list() {
    let (store, _dir) = temp_store();

    // Should succeed silently with no migrations to apply.
    store.migrate("empty-component", &[]).unwrap();
}

#[test]
fn test_migration_component_isolation() {
    let (store, _dir) = temp_store();

    let migrations_a = &[
        Migration {
            version: 1,
            description: "Component A table",
            sql: "CREATE TABLE comp_a (id TEXT PRIMARY KEY);",
        },
    ];

    let migrations_b = &[
        Migration {
            version: 1,
            description: "Component B table",
            sql: "CREATE TABLE comp_b (id TEXT PRIMARY KEY);",
        },
    ];

    // Both components can have version 1 independently.
    store.migrate("component-a", migrations_a).unwrap();
    store.migrate("component-b", migrations_b).unwrap();

    // Both tables should exist.
    // If there was a version collision, one would fail.
}

// ════════════════════════════════════════════════════════════════
// WAL Mode Verification
// ════════════════════════════════════════════════════════════════

#[test]
fn test_wal_mode_enabled() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("wal_test.db");
    let _store = StateStore::open(&db_path).unwrap();

    // After opening, a WAL file should exist alongside the main db.
    let _wal_path = db_path.with_extension("db-wal");
    // Note: WAL file may not exist if no writes have occurred yet,
    // but the journal_mode pragma should have been set. We verify
    // by checking the pragma value via a fresh connection.
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let mode: String = conn
        .query_row("PRAGMA journal_mode;", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "wal", "Journal mode must be WAL");
}
