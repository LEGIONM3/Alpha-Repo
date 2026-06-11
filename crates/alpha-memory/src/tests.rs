use super::*;

use alpha_common::schemas::memory::{GovernanceState, MemoryRecord, MemoryType};
use alpha_common::types::new_id;

/// Helper: create a test memory record.
fn test_memory(content: &str, memory_type: MemoryType) -> MemoryRecord {
    MemoryRecord::new(
        memory_type,
        content.to_string(),
        "test".to_string(),
        0.5,
    )
}

/// Helper: create a test memory with a specific embedding.
fn test_memory_with_embedding(content: &str, embedding: Vec<f32>) -> MemoryRecord {
    let mut record = test_memory(content, MemoryType::Episodic);
    record.embedding = embedding;
    record
}

/// Helper: open a temporary memory store.
fn temp_store(dim: usize) -> (MemoryStore, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(&dir.path().join("memory.db"), dim).unwrap();
    (store, dir)
}

#[test]
fn test_store_get() {
    let (store, _dir) = temp_store(0);

    let record = test_memory("First memory", MemoryType::Episodic);
    let id = record.id;

    store.store_with_embedding(&record).unwrap();

    let retrieved = store.get(&id).unwrap().expect("should exist");
    assert_eq!(retrieved.id, id);
    assert_eq!(retrieved.content, "First memory");
    assert_eq!(retrieved.memory_type, MemoryType::Episodic);
    assert_eq!(retrieved.source, "test");
    assert!((retrieved.importance - 0.5).abs() < 0.001);
    assert_eq!(retrieved.access_count, 0);
    assert_eq!(retrieved.governance_state, GovernanceState::Active);
}

#[test]
fn test_get_nonexistent() {
    let (store, _dir) = temp_store(0);
    let id = new_id();
    let result = store.get(&id).unwrap();
    assert!(result.is_none());
}

#[test]
fn test_delete() {
    let (store, _dir) = temp_store(0);

    let record = test_memory("To delete", MemoryType::Semantic);
    let id = record.id;
    store.store_with_embedding(&record).unwrap();

    assert!(store.get(&id).unwrap().is_some());

    let deleted = store.delete(&id).unwrap();
    assert!(deleted);

    assert!(store.get(&id).unwrap().is_none());

    // Double delete returns false.
    let deleted_again = store.delete(&id).unwrap();
    assert!(!deleted_again);
}

#[test]
fn test_list_by_type() {
    let (store, _dir) = temp_store(0);

    store
        .store_with_embedding(&test_memory("ep1", MemoryType::Episodic))
        .unwrap();
    store
        .store_with_embedding(&test_memory("ep2", MemoryType::Episodic))
        .unwrap();
    store
        .store_with_embedding(&test_memory("sem1", MemoryType::Semantic))
        .unwrap();
    store
        .store_with_embedding(&test_memory("proc1", MemoryType::Procedural))
        .unwrap();

    let episodic = store
        .list(Some(&MemoryType::Episodic), None, 100, 0)
        .unwrap();
    assert_eq!(episodic.len(), 2);

    let semantic = store
        .list(Some(&MemoryType::Semantic), None, 100, 0)
        .unwrap();
    assert_eq!(semantic.len(), 1);
    assert_eq!(semantic[0].content, "sem1");

    let procedural = store
        .list(Some(&MemoryType::Procedural), None, 100, 0)
        .unwrap();
    assert_eq!(procedural.len(), 1);

    let all = store.list(None, None, 100, 0).unwrap();
    assert_eq!(all.len(), 4);
}

#[test]
fn test_list_by_governance() {
    let (store, _dir) = temp_store(0);

    let r1 = test_memory("active1", MemoryType::Episodic);
    let r2 = test_memory("active2", MemoryType::Episodic);
    let r3 = test_memory("to_archive", MemoryType::Episodic);
    let id3 = r3.id;

    store.store_with_embedding(&r1).unwrap();
    store.store_with_embedding(&r2).unwrap();
    store.store_with_embedding(&r3).unwrap();

    // Transition one to Archived.
    store
        .set_governance(&id3, &GovernanceState::Archived)
        .unwrap();

    let active = store
        .list(None, Some(&GovernanceState::Active), 100, 0)
        .unwrap();
    assert_eq!(active.len(), 2);

    let archived = store
        .list(None, Some(&GovernanceState::Archived), 100, 0)
        .unwrap();
    assert_eq!(archived.len(), 1);
    assert_eq!(archived[0].id, id3);
}

#[test]
fn test_list_pagination() {
    let (store, _dir) = temp_store(0);

    for i in 0..5 {
        store
            .store_with_embedding(&test_memory(&format!("mem_{i}"), MemoryType::Episodic))
            .unwrap();
    }

    let page1 = store.list(None, None, 2, 0).unwrap();
    assert_eq!(page1.len(), 2);

    let page2 = store.list(None, None, 2, 2).unwrap();
    assert_eq!(page2.len(), 2);

    let page3 = store.list(None, None, 2, 4).unwrap();
    assert_eq!(page3.len(), 1);
}

#[test]
fn test_count() {
    let (store, _dir) = temp_store(0);

    assert_eq!(store.count(None, None).unwrap(), 0);

    store
        .store_with_embedding(&test_memory("a", MemoryType::Episodic))
        .unwrap();
    store
        .store_with_embedding(&test_memory("b", MemoryType::Semantic))
        .unwrap();
    store
        .store_with_embedding(&test_memory("c", MemoryType::Episodic))
        .unwrap();

    assert_eq!(store.count(None, None).unwrap(), 3);
    assert_eq!(
        store.count(Some(&MemoryType::Episodic), None).unwrap(),
        2
    );
    assert_eq!(
        store.count(Some(&MemoryType::Semantic), None).unwrap(),
        1
    );
    assert_eq!(
        store.count(Some(&MemoryType::Procedural), None).unwrap(),
        0
    );
}

#[test]
fn test_count_by_governance() {
    let (store, _dir) = temp_store(0);

    let r1 = test_memory("a", MemoryType::Episodic);
    let r2 = test_memory("b", MemoryType::Episodic);
    let id2 = r2.id;

    store.store_with_embedding(&r1).unwrap();
    store.store_with_embedding(&r2).unwrap();

    store
        .set_governance(&id2, &GovernanceState::Deprecated)
        .unwrap();

    assert_eq!(
        store
            .count(None, Some(&GovernanceState::Active))
            .unwrap(),
        1
    );
    assert_eq!(
        store
            .count(None, Some(&GovernanceState::Deprecated))
            .unwrap(),
        1
    );
}

#[test]
fn test_embedding_roundtrip() {
    let embedding = vec![0.1_f32, -0.5, 1.0, 0.0, 0.99, -1.0, 0.001, 42.42];

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
fn test_embedding_roundtrip_empty() {
    let empty: Vec<f32> = vec![];
    let bytes = embedding_to_bytes(&empty);
    assert!(bytes.is_empty());

    let restored = bytes_to_embedding(&bytes).unwrap();
    assert!(restored.is_empty());
}

#[test]
fn test_embedding_bytes_invalid_length() {
    let bad_bytes = vec![0u8, 1, 2]; // 3 bytes, not divisible by 4.
    let result = bytes_to_embedding(&bad_bytes);
    assert!(result.is_err());
}

#[test]
fn test_embedding_stored_and_retrieved() {
    let (store, _dir) = temp_store(0);

    let embedding = vec![0.1, 0.2, 0.3, 0.4, 0.5];
    let record = test_memory_with_embedding("with embedding", embedding.clone());
    let id = record.id;

    store.store_with_embedding(&record).unwrap();

    let retrieved = store.get(&id).unwrap().expect("should exist");
    assert_eq!(retrieved.embedding.len(), 5);
    for (a, b) in embedding.iter().zip(retrieved.embedding.iter()) {
        assert!(
            (a - b).abs() < f32::EPSILON,
            "Embedding mismatch: {a} vs {b}"
        );
    }
}

#[test]
fn test_dimension_validation_valid() {
    let (store, _dir) = temp_store(4);

    let record = test_memory_with_embedding("valid dim", vec![0.1, 0.2, 0.3, 0.4]);
    assert!(store.store_with_embedding(&record).is_ok());
}

#[test]
fn test_dimension_validation_invalid() {
    let (store, _dir) = temp_store(4);

    let record = test_memory_with_embedding("wrong dim", vec![0.1, 0.2, 0.3]);
    let result = store.store_with_embedding(&record);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("dimension"));
}

#[test]
fn test_dimension_validation_empty_embedding_passes() {
    let (store, _dir) = temp_store(4);

    // Empty embeddings are allowed (not computed yet).
    let record = test_memory("no embedding", MemoryType::Episodic);
    assert!(store.store_with_embedding(&record).is_ok());
}

#[test]
fn test_persistence_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("memory.db");

    let id;
    {
        let store = MemoryStore::open(&db_path, 0).unwrap();
        let record = test_memory("persistent", MemoryType::Semantic);
        id = record.id;
        store.store_with_embedding(&record).unwrap();
    }

    // Reopen.
    {
        let store = MemoryStore::open(&db_path, 0).unwrap();
        let retrieved = store.get(&id).unwrap().expect("should survive restart");
        assert_eq!(retrieved.content, "persistent");
        assert_eq!(retrieved.memory_type, MemoryType::Semantic);
    }
}

#[test]
fn test_wal_mode_enabled() {
    let (store, _dir) = temp_store(0);
    assert!(store.is_wal_mode().unwrap(), "WAL mode should be enabled");
}

#[test]
fn test_set_governance() {
    let (store, _dir) = temp_store(0);

    let record = test_memory("lifecycle", MemoryType::Episodic);
    let id = record.id;
    store.store_with_embedding(&record).unwrap();

    // Active → Reference
    store
        .set_governance(&id, &GovernanceState::Reference)
        .unwrap();
    let r = store.get(&id).unwrap().unwrap();
    assert_eq!(r.governance_state, GovernanceState::Reference);

    // Reference → Archived
    store
        .set_governance(&id, &GovernanceState::Archived)
        .unwrap();
    let r = store.get(&id).unwrap().unwrap();
    assert_eq!(r.governance_state, GovernanceState::Archived);

    // Archived → Deprecated
    store
        .set_governance(&id, &GovernanceState::Deprecated)
        .unwrap();
    let r = store.get(&id).unwrap().unwrap();
    assert_eq!(r.governance_state, GovernanceState::Deprecated);
}

#[test]
fn test_set_governance_nonexistent() {
    let (store, _dir) = temp_store(0);
    let id = new_id();
    let result = store.set_governance(&id, &GovernanceState::Archived);
    assert!(result.is_err());
}

#[test]
fn test_metadata_roundtrip() {
    let (store, _dir) = temp_store(0);

    let mut record = test_memory("with metadata", MemoryType::Semantic);
    record.metadata = serde_json::json!({
        "context": "conversation_123",
        "turn": 5,
        "tags": ["important", "user_preference"]
    });
    let id = record.id;

    store.store_with_embedding(&record).unwrap();

    let retrieved = store.get(&id).unwrap().unwrap();
    assert_eq!(retrieved.metadata["context"], "conversation_123");
    assert_eq!(retrieved.metadata["turn"], 5);
}

#[test]
fn test_tags_and_associations_roundtrip() {
    let (store, _dir) = temp_store(0);

    let mut record = test_memory("with tags", MemoryType::Episodic);
    record.tags = vec!["tag1".to_string(), "tag2".to_string()];
    let assoc_id = new_id();
    record.associations = vec![assoc_id];
    let id = record.id;

    store.store_with_embedding(&record).unwrap();

    let retrieved = store.get(&id).unwrap().unwrap();
    assert_eq!(retrieved.tags, vec!["tag1", "tag2"]);
    assert_eq!(retrieved.associations, vec![assoc_id]);
}

// ── Phase 2: Search Integration Tests ──

use crate::search::SearchOptions;

/// Helper: create a memory with embedding and specific importance.
fn memory_with_score(
    content: &str,
    embedding: Vec<f32>,
    importance: f32,
) -> MemoryRecord {
    let mut record = MemoryRecord::new(
        MemoryType::Episodic,
        content.to_string(),
        "test".to_string(),
        importance,
    );
    record.embedding = embedding;
    record
}

#[test]
fn test_search_by_embedding() {
    let (store, _dir) = temp_store(3);

    // Store memories with different embeddings.
    store
        .store_with_embedding(&memory_with_score("close", vec![1.0, 0.0, 0.0], 0.8))
        .unwrap();
    store
        .store_with_embedding(&memory_with_score("medium", vec![0.7, 0.7, 0.0], 0.8))
        .unwrap();
    store
        .store_with_embedding(&memory_with_score("far", vec![0.0, 0.0, 1.0], 0.8))
        .unwrap();
    // No embedding — should not appear in search.
    store
        .store_with_embedding(&test_memory("no embed", MemoryType::Episodic))
        .unwrap();

    let query = vec![1.0, 0.0, 0.0];
    let now = alpha_common::types::now();
    let options = SearchOptions {
        limit: 10,
        min_similarity: 0.0,
        governance_filter: None,
    };

    let results = store.search_by_embedding(&query, &options, &now).unwrap();

    // Should find 3 records (the one without embedding is excluded).
    assert_eq!(results.len(), 3);

    // "close" should be first (highest similarity to query).
    assert_eq!(results[0].record.content, "close");
    assert!(
        results[0].similarity > 0.99,
        "Expected ~1.0, got {}",
        results[0].similarity
    );

    // "medium" should be second.
    assert_eq!(results[1].record.content, "medium");

    // "far" should be last (orthogonal).
    assert_eq!(results[2].record.content, "far");
    assert!(
        results[2].similarity < 0.01,
        "Expected ~0.0, got {}",
        results[2].similarity
    );
}

#[test]
fn test_min_similarity_filter() {
    let (store, _dir) = temp_store(3);

    store
        .store_with_embedding(&memory_with_score("very close", vec![1.0, 0.0, 0.0], 0.9))
        .unwrap();
    store
        .store_with_embedding(&memory_with_score("somewhat", vec![0.7, 0.7, 0.0], 0.9))
        .unwrap();
    store
        .store_with_embedding(&memory_with_score("unrelated", vec![0.0, 0.0, 1.0], 0.9))
        .unwrap();

    let query = vec![1.0, 0.0, 0.0];
    let now = alpha_common::types::now();

    // High threshold — should exclude "unrelated".
    let options = SearchOptions {
        limit: 10,
        min_similarity: 0.5,
        governance_filter: None,
    };

    let results = store.search_by_embedding(&query, &options, &now).unwrap();

    // "unrelated" has similarity ~0.0 — should be filtered out.
    assert!(
        results.len() <= 2,
        "Expected at most 2 results, got {}",
        results.len()
    );
    for r in &results {
        assert!(
            r.similarity >= 0.5,
            "All results should have similarity >= 0.5, got {}",
            r.similarity
        );
    }
}

#[test]
fn test_governance_filter() {
    let (store, _dir) = temp_store(3);

    let r1 = memory_with_score("active_mem", vec![1.0, 0.0, 0.0], 0.8);
    let r2 = memory_with_score("archived_mem", vec![0.9, 0.1, 0.0], 0.8);
    let id2 = r2.id;

    store.store_with_embedding(&r1).unwrap();
    store.store_with_embedding(&r2).unwrap();

    // Archive one.
    store
        .set_governance(&id2, &GovernanceState::Archived)
        .unwrap();

    let query = vec![1.0, 0.0, 0.0];
    let now = alpha_common::types::now();

    // Search only active memories.
    let options = SearchOptions {
        limit: 10,
        min_similarity: 0.0,
        governance_filter: Some(GovernanceState::Active),
    };

    let results = store.search_by_embedding(&query, &options, &now).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].record.content, "active_mem");

    // Search only archived memories.
    let options_archived = SearchOptions {
        limit: 10,
        min_similarity: 0.0,
        governance_filter: Some(GovernanceState::Archived),
    };

    let results = store.search_by_embedding(&query, &options_archived, &now).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].record.content, "archived_mem");
}

#[test]
fn test_ranking_order() {
    let (store, _dir) = temp_store(3);

    // Same direction embeddings but different importance.
    // composite_score = similarity * importance * recency
    // similarity will be ~1.0 for all, recency ~1.0 for all (just created).
    // So ranking should be by importance.
    store
        .store_with_embedding(&memory_with_score("low", vec![1.0, 0.0, 0.0], 0.5))
        .unwrap();
    store
        .store_with_embedding(&memory_with_score("high", vec![1.0, 0.0, 0.0], 1.0))
        .unwrap();
    store
        .store_with_embedding(&memory_with_score("medium", vec![1.0, 0.0, 0.0], 0.75))
        .unwrap();

    let query = vec![1.0, 0.0, 0.0];
    let now = alpha_common::types::now();
    let options = SearchOptions::default();

    let results = store.search_by_embedding(&query, &options, &now).unwrap();

    assert_eq!(results.len(), 3);
    assert_eq!(results[0].record.content, "high");
    assert_eq!(results[1].record.content, "medium");
    assert_eq!(results[2].record.content, "low");

    // Verify scores are monotonically decreasing.
    for i in 1..results.len() {
        assert!(
            results[i - 1].score >= results[i].score,
            "Results should be sorted by score descending: {} < {}",
            results[i - 1].score,
            results[i].score
        );
    }
}

#[test]
fn test_search_empty_query_rejected() {
    let (store, _dir) = temp_store(0);

    let now = alpha_common::types::now();
    let options = SearchOptions::default();

    let result = store.search_by_embedding(&[], &options, &now);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("empty"));
}

#[test]
fn test_search_no_results() {
    let (store, _dir) = temp_store(3);

    // Store memory without embedding.
    store
        .store_with_embedding(&test_memory("no embed", MemoryType::Episodic))
        .unwrap();

    let query = vec![1.0, 0.0, 0.0];
    let now = alpha_common::types::now();
    let options = SearchOptions::default();

    let results = store.search_by_embedding(&query, &options, &now).unwrap();
    assert!(results.is_empty());
}

#[test]
fn test_search_limit_respected() {
    let (store, _dir) = temp_store(3);

    for i in 0..10 {
        store
            .store_with_embedding(&memory_with_score(
                &format!("mem_{i}"),
                vec![1.0, 0.0, 0.0],
                0.8,
            ))
            .unwrap();
    }

    let query = vec![1.0, 0.0, 0.0];
    let now = alpha_common::types::now();
    let options = SearchOptions {
        limit: 3,
        min_similarity: 0.0,
        governance_filter: None,
    };

    let results = store.search_by_embedding(&query, &options, &now).unwrap();
    assert_eq!(results.len(), 3);
}
