//! Comprehensive unit tests for the Security Gate.

use serde_json::json;

use alpha_common::schemas::goal::RiskLevel;

use crate::types::{ActionRequest, SecurityDecision};
use crate::{AllowAllGate, SecurityGate};

/// Helper: create an AllowAllGate backed by a temp file.
fn temp_gate() -> (AllowAllGate, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("test_audit.db");
    let gate = AllowAllGate::open(&db_path).expect("open gate");
    (gate, dir)
}

/// Helper: create a basic ActionRequest.
fn test_request(action_type: &str, agent: &str, target: &str) -> ActionRequest {
    ActionRequest::new(action_type, agent, target, RiskLevel::Low)
}

// ════════════════════════════════════════════════════════════════
// AllowAllGate Tests
// ════════════════════════════════════════════════════════════════

#[test]
fn test_allow_all_approves() {
    let (gate, _dir) = temp_gate();

    let request = test_request("file.write", "research-agent", "/tmp/output.txt");
    let decision = gate.evaluate(&request);

    assert!(
        decision.is_approved(),
        "AllowAllGate must approve every request"
    );
    assert_eq!(decision.label(), "approved");
}

#[test]
fn test_allow_all_approves_high_risk() {
    let (gate, _dir) = temp_gate();

    let request = ActionRequest::new(
        "shell.execute",
        "operator-agent",
        "rm -rf /",
        RiskLevel::Critical,
    );
    let decision = gate.evaluate(&request);

    assert!(
        decision.is_approved(),
        "Sprint 1 AllowAllGate approves even critical actions"
    );
}

// ════════════════════════════════════════════════════════════════
// Audit Logging Tests
// ════════════════════════════════════════════════════════════════

#[test]
fn test_audit_log_written() {
    let (gate, _dir) = temp_gate();

    let request = test_request("file.read", "memory-agent", "/data/memories.db");
    gate.evaluate(&request);

    let count = gate.count().unwrap();
    assert_eq!(count, 1, "One evaluation should produce one audit entry");

    let entries = gate.get_all().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].request.action_type, "file.read");
    assert_eq!(entries[0].request.agent, "memory-agent");
    assert_eq!(entries[0].request.target, "/data/memories.db");
    assert!(entries[0].decision.is_approved());
}

#[test]
fn test_audit_log_persistence() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("persist_audit.db");

    // Session 1: evaluate and log.
    {
        let gate = AllowAllGate::open(&db_path).unwrap();
        let request = test_request("api.call", "model-router", "https://api.example.com");
        gate.evaluate(&request);

        assert_eq!(gate.count().unwrap(), 1);
    }

    // Session 2: reopen and verify persistence.
    {
        let gate = AllowAllGate::open(&db_path).unwrap();
        let count = gate.count().unwrap();
        assert_eq!(count, 1, "Audit entry must survive restart");

        let entries = gate.get_all().unwrap();
        assert_eq!(entries[0].request.action_type, "api.call");
        assert_eq!(entries[0].request.agent, "model-router");
    }
}

#[test]
fn test_action_request_roundtrip() {
    let request = ActionRequest::new(
        "file.write",
        "research-agent",
        "/data/report.md",
        RiskLevel::Medium,
    )
    .with_parameters(json!({"mode": "overwrite", "size_bytes": 1024}));

    // Serialize to JSON and back.
    let serialized = serde_json::to_string(&request).expect("serialize");
    let deserialized: ActionRequest =
        serde_json::from_str(&serialized).expect("deserialize");

    assert_eq!(request.id, deserialized.id);
    assert_eq!(request.action_type, deserialized.action_type);
    assert_eq!(request.agent, deserialized.agent);
    assert_eq!(request.target, deserialized.target);
    assert_eq!(deserialized.parameters["mode"], "overwrite");
    assert_eq!(deserialized.parameters["size_bytes"], 1024);
}

#[test]
fn test_security_decision_roundtrip() {
    let decision = SecurityDecision::Approved {
        id: alpha_common::types::new_id(),
        reason: "test approval".to_string(),
    };

    let serialized = serde_json::to_string(&decision).expect("serialize");
    let deserialized: SecurityDecision =
        serde_json::from_str(&serialized).expect("deserialize");

    assert_eq!(decision, deserialized);

    let denied = SecurityDecision::Denied {
        id: alpha_common::types::new_id(),
        reason: "too dangerous".to_string(),
    };
    let serialized = serde_json::to_string(&denied).expect("serialize");
    let deserialized: SecurityDecision =
        serde_json::from_str(&serialized).expect("deserialize");
    assert_eq!(denied, deserialized);
}

#[test]
fn test_multiple_requests_logged() {
    let (gate, _dir) = temp_gate();

    let actions = vec![
        ("file.read", "agent-a", "/tmp/a.txt"),
        ("file.write", "agent-b", "/tmp/b.txt"),
        ("shell.execute", "agent-c", "ls -la"),
        ("api.call", "agent-d", "https://example.com"),
        ("memory.store", "agent-e", "episodic"),
    ];

    for (action, agent, target) in &actions {
        let request = test_request(action, agent, target);
        let decision = gate.evaluate(&request);
        assert!(decision.is_approved());
    }

    let count = gate.count().unwrap();
    assert_eq!(count, 5, "All 5 evaluations should be logged");

    let entries = gate.get_all().unwrap();
    assert_eq!(entries.len(), 5);

    // Verify entries are in order.
    assert_eq!(entries[0].request.action_type, "file.read");
    assert_eq!(entries[1].request.action_type, "file.write");
    assert_eq!(entries[2].request.action_type, "shell.execute");
    assert_eq!(entries[3].request.action_type, "api.call");
    assert_eq!(entries[4].request.action_type, "memory.store");
}

#[test]
fn test_count_matches_entries() {
    let (gate, _dir) = temp_gate();

    assert_eq!(gate.count().unwrap(), 0, "Empty gate should have 0 entries");

    gate.evaluate(&test_request("a", "x", "y"));
    assert_eq!(gate.count().unwrap(), 1);

    gate.evaluate(&test_request("b", "x", "y"));
    gate.evaluate(&test_request("c", "x", "y"));
    assert_eq!(gate.count().unwrap(), 3);

    let entries = gate.get_all().unwrap();
    assert_eq!(
        entries.len() as u64,
        gate.count().unwrap(),
        "count() must match get_all().len()"
    );
}

#[test]
fn test_decision_helpers() {
    let approved = SecurityDecision::Approved {
        id: alpha_common::types::new_id(),
        reason: "ok".to_string(),
    };
    assert!(approved.is_approved());
    assert_eq!(approved.label(), "approved");
    assert_eq!(approved.reason(), "ok");

    let denied = SecurityDecision::Denied {
        id: alpha_common::types::new_id(),
        reason: "nope".to_string(),
    };
    assert!(!denied.is_approved());
    assert_eq!(denied.label(), "denied");

    let pending = SecurityDecision::RequiresApproval {
        id: alpha_common::types::new_id(),
        reason: "ask user".to_string(),
    };
    assert!(!pending.is_approved());
    assert_eq!(pending.label(), "requires_approval");
}

#[test]
fn test_wal_mode_enabled() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("wal_test.db");
    let _gate = AllowAllGate::open(&db_path).unwrap();

    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let mode: String = conn
        .query_row("PRAGMA journal_mode;", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "wal", "Audit database must use WAL mode");
}

#[test]
fn test_with_parameters() {
    let request = ActionRequest::new("file.write", "test", "/tmp/x", RiskLevel::Low)
        .with_parameters(json!({"encoding": "utf-8"}));

    assert_eq!(request.parameters["encoding"], "utf-8");
}
