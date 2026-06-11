use super::*;

use alpha_common::schemas::relationship::{
    RelationshipCategory, RelationshipCoreRecord, RelationshipSource,
};
use alpha_common::types::new_id;

/// Helper: create a valid test record.
fn test_record(content: &str, category: RelationshipCategory) -> RelationshipCoreRecord {
    RelationshipCoreRecord::new(
        category,
        content.to_string(),
        RelationshipSource::UserExplicit,
        0.8,
    )
}

/// Helper: open a temporary relationship store.
fn temp_store() -> (RelationshipStore, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let store = RelationshipStore::open(&dir.path().join("relationship.db")).unwrap();
    (store, dir)
}

// ── store_get ──

#[test]
fn test_store_get() {
    let (store, _dir) = temp_store();

    let record = test_record("User prefers concise responses", RelationshipCategory::CommunicationPref);
    let id = record.id;

    store.store_with_embedding(&record).unwrap();

    let retrieved = store.get(&id).unwrap().expect("should exist");
    assert_eq!(retrieved.id, id);
    assert_eq!(retrieved.content, "User prefers concise responses");
    assert_eq!(retrieved.category, RelationshipCategory::CommunicationPref);
    assert!(retrieved.protected);
    assert!((retrieved.decay_rate - 0.0).abs() < f32::EPSILON);
    assert_eq!(retrieved.governance_state, "active");
    assert!(retrieved.importance >= 0.5);
    assert!(!retrieved.confirmed_by_user);
}

#[test]
fn test_get_nonexistent() {
    let (store, _dir) = temp_store();
    let id = new_id();
    assert!(store.get(&id).unwrap().is_none());
}

// ── invariant_enforcement_rust ──

#[test]
fn test_invariant_enforcement_rust_protected() {
    let (store, _dir) = temp_store();

    let mut record = test_record("bad", RelationshipCategory::TrustEvolution);
    record.protected = false;

    let result = store.store_with_embedding(&record);
    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_string().contains("protected"),
        "Should mention 'protected'"
    );
}

#[test]
fn test_invariant_enforcement_rust_decay_rate() {
    let (store, _dir) = temp_store();

    let mut record = test_record("bad", RelationshipCategory::TrustEvolution);
    record.decay_rate = 0.1;

    let result = store.store_with_embedding(&record);
    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_string().contains("decay_rate"),
        "Should mention 'decay_rate'"
    );
}

#[test]
fn test_invariant_enforcement_rust_governance() {
    let (store, _dir) = temp_store();

    let mut record = test_record("bad", RelationshipCategory::TrustEvolution);
    record.governance_state = "archived".to_string();

    let result = store.store_with_embedding(&record);
    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_string().contains("governance_state"),
        "Should mention 'governance_state'"
    );
}

#[test]
fn test_invariant_enforcement_rust_importance() {
    let (store, _dir) = temp_store();

    let mut record = test_record("bad", RelationshipCategory::TrustEvolution);
    record.importance = 0.3;

    let result = store.store_with_embedding(&record);
    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_string().contains("importance"),
        "Should mention 'importance'"
    );
}

// ── invariant_enforcement_sql ──

#[test]
fn test_invariant_enforcement_sql_importance() {
    let (store, _dir) = temp_store();

    // Bypass Rust validation by using raw SQL to test CHECK constraints.
    let conn = store.conn.lock().unwrap();
    let now = alpha_common::types::now().to_rfc3339();
    let id = new_id().to_string();

    let result = conn.execute(
        "INSERT INTO relationship_core
            (id, category, content, importance, protected, decay_rate,
             governance_state, source, confidence, confirmed_by_user,
             created_at, updated_at)
         VALUES (?1, 'trust_evolution', 'test', 0.3, 1, 0.0,
                 'active', 'user_explicit', 0.8, 0, ?2, ?2)",
        rusqlite::params![id, now],
    );

    assert!(result.is_err(), "SQL CHECK should reject importance < 0.5");
    assert!(
        result.unwrap_err().to_string().contains("CHECK"),
        "Error should mention CHECK constraint"
    );
}

#[test]
fn test_invariant_enforcement_sql_protected() {
    let (store, _dir) = temp_store();

    let conn = store.conn.lock().unwrap();
    let now = alpha_common::types::now().to_rfc3339();
    let id = new_id().to_string();

    let result = conn.execute(
        "INSERT INTO relationship_core
            (id, category, content, importance, protected, decay_rate,
             governance_state, source, confidence, confirmed_by_user,
             created_at, updated_at)
         VALUES (?1, 'trust_evolution', 'test', 0.8, 0, 0.0,
                 'active', 'user_explicit', 0.8, 0, ?2, ?2)",
        rusqlite::params![id, now],
    );

    assert!(result.is_err(), "SQL CHECK should reject protected = 0");
}

#[test]
fn test_invariant_enforcement_sql_decay_rate() {
    let (store, _dir) = temp_store();

    let conn = store.conn.lock().unwrap();
    let now = alpha_common::types::now().to_rfc3339();
    let id = new_id().to_string();

    let result = conn.execute(
        "INSERT INTO relationship_core
            (id, category, content, importance, protected, decay_rate,
             governance_state, source, confidence, confirmed_by_user,
             created_at, updated_at)
         VALUES (?1, 'trust_evolution', 'test', 0.8, 1, 0.5,
                 'active', 'user_explicit', 0.8, 0, ?2, ?2)",
        rusqlite::params![id, now],
    );

    assert!(result.is_err(), "SQL CHECK should reject decay_rate != 0");
}

#[test]
fn test_invariant_enforcement_sql_governance() {
    let (store, _dir) = temp_store();

    let conn = store.conn.lock().unwrap();
    let now = alpha_common::types::now().to_rfc3339();
    let id = new_id().to_string();

    let result = conn.execute(
        "INSERT INTO relationship_core
            (id, category, content, importance, protected, decay_rate,
             governance_state, source, confidence, confirmed_by_user,
             created_at, updated_at)
         VALUES (?1, 'trust_evolution', 'test', 0.8, 1, 0.0,
                 'archived', 'user_explicit', 0.8, 0, ?2, ?2)",
        rusqlite::params![id, now],
    );

    assert!(result.is_err(), "SQL CHECK should reject governance != 'active'");
}

// ── communication_preferences ──

#[test]
fn test_communication_preferences() {
    let (store, _dir) = temp_store();

    store
        .store_with_embedding(&test_record(
            "User likes bullet points",
            RelationshipCategory::CommunicationPref,
        ))
        .unwrap();
    store
        .store_with_embedding(&test_record(
            "User prefers formal tone",
            RelationshipCategory::CommunicationPref,
        ))
        .unwrap();
    store
        .store_with_embedding(&test_record(
            "Trust evolved positively",
            RelationshipCategory::TrustEvolution,
        ))
        .unwrap();

    let prefs = store.get_communication_prefs().unwrap();
    assert_eq!(prefs.len(), 2);
    // All should be communication_pref.
    for pref in &prefs {
        assert_eq!(pref.category, RelationshipCategory::CommunicationPref);
    }
}

// ── confirm ──

#[test]
fn test_confirm() {
    let (store, _dir) = temp_store();

    let record = test_record("User values honesty", RelationshipCategory::UserIdentity);
    let id = record.id;
    store.store_with_embedding(&record).unwrap();

    // Before confirmation.
    let r = store.get(&id).unwrap().unwrap();
    assert!(!r.confirmed_by_user);

    // Confirm.
    store.confirm(&id).unwrap();

    // After confirmation.
    let r = store.get(&id).unwrap().unwrap();
    assert!(r.confirmed_by_user);
    // updated_at should have changed.
    assert!(r.updated_at >= r.created_at);
}

#[test]
fn test_confirm_nonexistent() {
    let (store, _dir) = temp_store();
    let id = new_id();
    let result = store.confirm(&id);
    assert!(result.is_err());
}

// ── count ──

#[test]
fn test_count() {
    let (store, _dir) = temp_store();

    assert_eq!(store.count(None).unwrap(), 0);

    store
        .store_with_embedding(&test_record("a", RelationshipCategory::TrustEvolution))
        .unwrap();
    store
        .store_with_embedding(&test_record("b", RelationshipCategory::SharedHistory))
        .unwrap();
    store
        .store_with_embedding(&test_record("c", RelationshipCategory::TrustEvolution))
        .unwrap();

    assert_eq!(store.count(None).unwrap(), 3);
    assert_eq!(
        store.count(Some(&RelationshipCategory::TrustEvolution)).unwrap(),
        2
    );
    assert_eq!(
        store.count(Some(&RelationshipCategory::SharedHistory)).unwrap(),
        1
    );
    assert_eq!(
        store.count(Some(&RelationshipCategory::AlphaPurpose)).unwrap(),
        0
    );
}

// ── persistence_across_restart ──

#[test]
fn test_persistence_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("relationship.db");

    let id;
    {
        let store = RelationshipStore::open(&db_path).unwrap();
        let record = test_record("persisted", RelationshipCategory::AlphaPurpose);
        id = record.id;
        store.store_with_embedding(&record).unwrap();
    }

    // Reopen.
    {
        let store = RelationshipStore::open(&db_path).unwrap();
        let retrieved = store.get(&id).unwrap().expect("should survive restart");
        assert_eq!(retrieved.content, "persisted");
        assert_eq!(retrieved.category, RelationshipCategory::AlphaPurpose);
        assert!(retrieved.protected);
    }
}

// ── wal_mode_enabled ──

#[test]
fn test_wal_mode_enabled() {
    let (store, _dir) = temp_store();
    assert!(store.is_wal_mode().unwrap(), "WAL mode should be enabled");
}

// ── embedding_roundtrip ──

#[test]
fn test_embedding_roundtrip() {
    let embedding = vec![0.1_f32, -0.5, 1.0, 0.0, 0.99];

    let bytes = embedding_to_bytes(&embedding);
    assert_eq!(bytes.len(), embedding.len() * 4);

    let restored = bytes_to_embedding(&bytes).unwrap();
    assert_eq!(restored.len(), embedding.len());
    for (a, b) in embedding.iter().zip(restored.iter()) {
        assert!(
            (a - b).abs() < f32::EPSILON,
            "Mismatch: {a} vs {b}"
        );
    }
}

#[test]
fn test_embedding_stored_and_retrieved() {
    let (store, _dir) = temp_store();

    let mut record = test_record("with embedding", RelationshipCategory::UserIdentity);
    record.embedding = vec![0.1, 0.2, 0.3, 0.4];
    let id = record.id;

    store.store_with_embedding(&record).unwrap();

    let retrieved = store.get(&id).unwrap().unwrap();
    assert_eq!(retrieved.embedding.len(), 4);
    for (a, b) in record.embedding.iter().zip(retrieved.embedding.iter()) {
        assert!(
            (a - b).abs() < f32::EPSILON,
            "Embedding mismatch: {a} vs {b}"
        );
    }
}

// ── list ──

#[test]
fn test_list() {
    let (store, _dir) = temp_store();

    store
        .store_with_embedding(&test_record("a", RelationshipCategory::TrustEvolution))
        .unwrap();
    store
        .store_with_embedding(&test_record("b", RelationshipCategory::SharedHistory))
        .unwrap();
    store
        .store_with_embedding(&test_record("c", RelationshipCategory::TrustEvolution))
        .unwrap();

    let all = store.list(None, 100, 0).unwrap();
    assert_eq!(all.len(), 3);

    let trust = store
        .list(Some(&RelationshipCategory::TrustEvolution), 100, 0)
        .unwrap();
    assert_eq!(trust.len(), 2);

    let shared = store
        .list(Some(&RelationshipCategory::SharedHistory), 100, 0)
        .unwrap();
    assert_eq!(shared.len(), 1);
}

// ── metadata roundtrip ──

#[test]
fn test_metadata_roundtrip() {
    let (store, _dir) = temp_store();

    let mut record = test_record("with metadata", RelationshipCategory::UserIdentity);
    record.metadata = serde_json::json!({
        "context": "onboarding",
        "session": 1
    });
    let id = record.id;

    store.store_with_embedding(&record).unwrap();

    let retrieved = store.get(&id).unwrap().unwrap();
    assert_eq!(retrieved.metadata["context"], "onboarding");
    assert_eq!(retrieved.metadata["session"], 1);
}

// ── related memories roundtrip ──

#[test]
fn test_related_memories_roundtrip() {
    let (store, _dir) = temp_store();

    let mem_id1 = new_id();
    let mem_id2 = new_id();
    let mut record = test_record("with links", RelationshipCategory::SharedHistory);
    record.related_memories = vec![mem_id1, mem_id2];
    let id = record.id;

    store.store_with_embedding(&record).unwrap();

    let retrieved = store.get(&id).unwrap().unwrap();
    assert_eq!(retrieved.related_memories.len(), 2);
    assert_eq!(retrieved.related_memories[0], mem_id1);
    assert_eq!(retrieved.related_memories[1], mem_id2);
}

// ── all categories ──

#[test]
fn test_all_categories() {
    let (store, _dir) = temp_store();

    let categories = [
        RelationshipCategory::TrustEvolution,
        RelationshipCategory::SharedHistory,
        RelationshipCategory::CommunicationPref,
        RelationshipCategory::UserIdentity,
        RelationshipCategory::AlphaPurpose,
    ];

    for cat in &categories {
        store
            .store_with_embedding(&test_record("test", cat.clone()))
            .unwrap();
    }

    assert_eq!(store.count(None).unwrap(), 5);
}
