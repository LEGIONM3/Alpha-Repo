//! Unit tests for the alpha-knowledge crate.

use serde_json::json;

use crate::{KnowledgeEntity, KnowledgeStore};

/// Create a temporary knowledge store for testing.
fn test_store() -> (KnowledgeStore, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("knowledge.db");
    let store = KnowledgeStore::open(&db_path).expect("open store");
    (store, dir)
}

/// Helper: create a sample entity.
fn sample_entity(entity_type: &str, name: &str) -> KnowledgeEntity {
    KnowledgeEntity::new(entity_type, name, "test")
        .with_description(format!("A {entity_type} named {name}"))
        .with_confidence(0.9)
        .with_properties(json!({"key": "value"}))
}

// ── Core CRUD ──

#[test]
fn test_store_get() {
    let (store, _dir) = test_store();

    let entity = sample_entity("person", "Alice");
    let id = store.store(&entity).expect("store");
    assert_eq!(id, entity.id);

    let retrieved = store.get(&id).expect("get").expect("should exist");
    assert_eq!(retrieved.id, entity.id);
    assert_eq!(retrieved.entity_type, "person");
    assert_eq!(retrieved.name, "Alice");
    assert_eq!(retrieved.description, "A person named Alice");
    assert_eq!(retrieved.source, "test");
    assert!((retrieved.confidence - 0.9).abs() < f32::EPSILON);
    assert_eq!(retrieved.properties, json!({"key": "value"}));
}

#[test]
fn test_get_nonexistent() {
    let (store, _dir) = test_store();

    let fake_id = alpha_common::new_id();
    let result = store.get(&fake_id).expect("get should not error");
    assert!(result.is_none());
}

#[test]
fn test_update() {
    let (store, _dir) = test_store();

    let mut entity = sample_entity("concept", "Rust");
    store.store(&entity).expect("store");

    // Update fields.
    entity.name = "Rust Programming Language".to_string();
    entity.description = "A systems programming language".to_string();
    entity.confidence = 0.95;
    entity.updated_at = alpha_common::now();
    store.update(&entity).expect("update");

    let retrieved = store.get(&entity.id).expect("get").expect("should exist");
    assert_eq!(retrieved.name, "Rust Programming Language");
    assert_eq!(retrieved.description, "A systems programming language");
    assert!((retrieved.confidence - 0.95).abs() < f32::EPSILON);
}

#[test]
fn test_update_nonexistent() {
    let (store, _dir) = test_store();

    let entity = sample_entity("concept", "Ghost");
    let result = store.update(&entity);
    assert!(result.is_err(), "updating nonexistent entity should fail");
}

#[test]
fn test_delete() {
    let (store, _dir) = test_store();

    let entity = sample_entity("place", "Tokyo");
    let id = store.store(&entity).expect("store");

    assert!(store.delete(&id).expect("delete"), "should return true");
    assert!(store.get(&id).expect("get").is_none(), "should be gone");
}

#[test]
fn test_delete_nonexistent() {
    let (store, _dir) = test_store();

    let fake_id = alpha_common::new_id();
    assert!(!store.delete(&fake_id).expect("delete"), "should return false");
}

// ── List ──

#[test]
fn test_list() {
    let (store, _dir) = test_store();

    // Store mixed entity types.
    store.store(&sample_entity("person", "Alice")).expect("store");
    store.store(&sample_entity("person", "Bob")).expect("store");
    store.store(&sample_entity("concept", "Rust")).expect("store");
    store.store(&sample_entity("place", "Tokyo")).expect("store");

    // List all.
    let all = store.list(None, 100, 0).expect("list all");
    assert_eq!(all.len(), 4);

    // List by type.
    let people = store.list(Some("person"), 100, 0).expect("list people");
    assert_eq!(people.len(), 2);

    let concepts = store.list(Some("concept"), 100, 0).expect("list concepts");
    assert_eq!(concepts.len(), 1);
    assert_eq!(concepts[0].name, "Rust");

    // List with limit.
    let limited = store.list(None, 2, 0).expect("list limited");
    assert_eq!(limited.len(), 2);

    // List with offset.
    let offset = store.list(None, 100, 3).expect("list offset");
    assert_eq!(offset.len(), 1);

    // List empty type.
    let empty = store.list(Some("nonexistent"), 100, 0).expect("list empty");
    assert!(empty.is_empty());
}

// ── Count ──

#[test]
fn test_count() {
    let (store, _dir) = test_store();

    assert_eq!(store.count(None).expect("count"), 0);

    store.store(&sample_entity("person", "Alice")).expect("store");
    store.store(&sample_entity("person", "Bob")).expect("store");
    store.store(&sample_entity("concept", "Rust")).expect("store");

    assert_eq!(store.count(None).expect("count all"), 3);
    assert_eq!(store.count(Some("person")).expect("count people"), 2);
    assert_eq!(store.count(Some("concept")).expect("count concepts"), 1);
    assert_eq!(store.count(Some("nonexistent")).expect("count empty"), 0);
}

// ── Persistence ──

#[test]
fn test_persistence_across_restart() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("knowledge.db");

    let entity_id;

    // First session: store an entity.
    {
        let store = KnowledgeStore::open(&db_path).expect("open first");
        let entity = sample_entity("skill", "Cooking");
        entity_id = entity.id;
        store.store(&entity).expect("store");
        assert_eq!(store.count(None).expect("count"), 1);
    }
    // Store dropped, connection closed.

    // Second session: verify persistence.
    {
        let store = KnowledgeStore::open(&db_path).expect("open second");
        let retrieved = store.get(&entity_id).expect("get").expect("should persist");
        assert_eq!(retrieved.name, "Cooking");
        assert_eq!(retrieved.entity_type, "skill");
        assert_eq!(store.count(None).expect("count"), 1);
    }
}

// ── WAL Mode ──

#[test]
fn test_wal_mode_enabled() {
    let (store, _dir) = test_store();
    assert!(store.is_wal_mode().expect("check WAL mode"));
}

// ── Serialization ──

#[test]
fn test_entity_serialization_roundtrip() {
    let entity = sample_entity("event", "Conference")
        .with_properties(json!({"year": 2025, "location": "Berlin"}));

    let json_str = serde_json::to_string(&entity).expect("serialize");
    let deserialized: KnowledgeEntity =
        serde_json::from_str(&json_str).expect("deserialize");

    assert_eq!(entity.id, deserialized.id);
    assert_eq!(entity.name, deserialized.name);
    assert_eq!(entity.entity_type, deserialized.entity_type);
    assert_eq!(entity.properties, deserialized.properties);
}
