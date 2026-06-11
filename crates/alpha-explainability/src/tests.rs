//! Unit tests for the alpha-explainability crate.

use alpha_common::{
    new_id, Alternative, ExplainabilityRecord, ExplanationType, Factor, FactorDirection,
};

use crate::ExplainabilityStore;

/// Create a temporary explainability store for testing.
fn test_store() -> (ExplainabilityStore, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("explainability.db");
    let store = ExplainabilityStore::open(&db_path).expect("open store");
    (store, dir)
}

/// Helper: create a minimal record.
fn minimal_record() -> ExplainabilityRecord {
    ExplainabilityRecord::new(
        ExplanationType::Model,
        new_id(),
        "Selected llama3.1:8b for text generation".to_string(),
        new_id(),
    )
}

/// Helper: create a record with all fields populated.
fn full_record() -> ExplainabilityRecord {
    let mut record = ExplainabilityRecord::new(
        ExplanationType::Action,
        new_id(),
        "Stored memory about user preference".to_string(),
        new_id(),
    );

    record.confidence = 0.85;

    record.reasoning = vec![
        "User explicitly stated preference".to_string(),
        "High confidence from direct statement".to_string(),
        "No conflicting information found".to_string(),
    ];

    record.factors = vec![
        Factor {
            name: "explicitness".to_string(),
            value: 0.95,
            weight: 0.6,
            direction: FactorDirection::For,
        },
        Factor {
            name: "recency".to_string(),
            value: 1.0,
            weight: 0.3,
            direction: FactorDirection::For,
        },
        Factor {
            name: "contradiction_risk".to_string(),
            value: 0.1,
            weight: 0.1,
            direction: FactorDirection::Against,
        },
    ];

    record.alternatives = vec![
        Alternative {
            option: "Ignore preference".to_string(),
            score: 0.2,
            rejection_reason: "User explicitly stated it".to_string(),
        },
        Alternative {
            option: "Store as low-confidence".to_string(),
            score: 0.5,
            rejection_reason: "Direct statement warrants high confidence".to_string(),
        },
    ];

    record.evidence = vec![new_id(), new_id()];

    record
}

// ── Core: store + get ──

#[test]
fn test_store_get() {
    let (store, _dir) = test_store();

    let record = minimal_record();
    let id = store.store(&record).expect("store");
    assert_eq!(id, record.id);

    let retrieved = store.get(&id).expect("get").expect("should exist");
    assert_eq!(retrieved.id, record.id);
    assert_eq!(retrieved.explanation_type, ExplanationType::Model);
    assert_eq!(retrieved.subject_id, record.subject_id);
    assert_eq!(retrieved.summary, "Selected llama3.1:8b for text generation");
    assert_eq!(retrieved.trace_id, record.trace_id);
    assert!((retrieved.confidence - 1.0).abs() < f32::EPSILON);
    assert!(retrieved.reasoning.is_empty());
    assert!(retrieved.factors.is_empty());
    assert!(retrieved.alternatives.is_empty());
    assert!(retrieved.evidence.is_empty());
}

#[test]
fn test_get_nonexistent() {
    let (store, _dir) = test_store();
    let result = store.get(&new_id()).expect("get should not error");
    assert!(result.is_none());
}

// ── Full roundtrip with all fields ──

#[test]
fn test_explainability_roundtrip() {
    let (store, _dir) = test_store();

    let record = full_record();
    store.store(&record).expect("store");

    let retrieved = store.get(&record.id).expect("get").expect("should exist");

    // Core fields.
    assert_eq!(retrieved.id, record.id);
    assert_eq!(retrieved.explanation_type, ExplanationType::Action);
    assert_eq!(retrieved.subject_id, record.subject_id);
    assert_eq!(retrieved.summary, record.summary);
    assert_eq!(retrieved.trace_id, record.trace_id);
    assert!((retrieved.confidence - 0.85).abs() < f32::EPSILON);

    // Reasoning preserved.
    assert_eq!(retrieved.reasoning.len(), 3);
    assert_eq!(retrieved.reasoning[0], "User explicitly stated preference");

    // Factors preserved.
    assert_eq!(retrieved.factors.len(), 3);
    assert_eq!(retrieved.factors[0].name, "explicitness");
    assert!((retrieved.factors[0].value - 0.95).abs() < f32::EPSILON);
    assert!((retrieved.factors[0].weight - 0.6).abs() < f32::EPSILON);
    assert_eq!(retrieved.factors[0].direction, FactorDirection::For);
    assert_eq!(retrieved.factors[2].direction, FactorDirection::Against);

    // Alternatives preserved.
    assert_eq!(retrieved.alternatives.len(), 2);
    assert_eq!(retrieved.alternatives[0].option, "Ignore preference");
    assert!((retrieved.alternatives[0].score - 0.2).abs() < f32::EPSILON);

    // Evidence preserved.
    assert_eq!(retrieved.evidence.len(), 2);
    assert_eq!(retrieved.evidence[0], record.evidence[0]);
    assert_eq!(retrieved.evidence[1], record.evidence[1]);
}

// ── Count ──

#[test]
fn test_count() {
    let (store, _dir) = test_store();

    assert_eq!(store.count().expect("count"), 0);

    store.store(&minimal_record()).expect("store 1");
    assert_eq!(store.count().expect("count"), 1);

    store.store(&minimal_record()).expect("store 2");
    store.store(&full_record()).expect("store 3");
    assert_eq!(store.count().expect("count"), 3);
}

// ── Persistence ──

#[test]
fn test_persistence_across_restart() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("explainability.db");

    let record = full_record();
    let record_id = record.id;

    // First session.
    {
        let store = ExplainabilityStore::open(&db_path).expect("open first");
        store.store(&record).expect("store");
        assert_eq!(store.count().expect("count"), 1);
    }

    // Second session.
    {
        let store = ExplainabilityStore::open(&db_path).expect("open second");
        assert_eq!(store.count().expect("count"), 1);

        let retrieved = store.get(&record_id).expect("get").expect("should persist");
        assert_eq!(retrieved.summary, record.summary);
        assert_eq!(retrieved.reasoning.len(), 3);
        assert_eq!(retrieved.factors.len(), 3);
        assert_eq!(retrieved.alternatives.len(), 2);
        assert_eq!(retrieved.evidence.len(), 2);
    }
}

// ── WAL Mode ──

#[test]
fn test_wal_mode_enabled() {
    let (store, _dir) = test_store();
    assert!(store.is_wal_mode().expect("check WAL mode"));
}

// ── All ExplanationType variants ──

#[test]
fn test_all_explanation_types() {
    let (store, _dir) = test_store();

    let types = [
        ExplanationType::Action,
        ExplanationType::Model,
        ExplanationType::Task,
        ExplanationType::Recommendation,
    ];

    for explanation_type in &types {
        let record = ExplainabilityRecord::new(
            explanation_type.clone(),
            new_id(),
            format!("Test {explanation_type:?}"),
            new_id(),
        );
        store.store(&record).expect("store");
        let retrieved = store.get(&record.id).expect("get").expect("should exist");
        assert_eq!(&retrieved.explanation_type, explanation_type);
    }

    assert_eq!(store.count().expect("count"), 4);
}
