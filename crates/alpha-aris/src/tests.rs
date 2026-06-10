//! Comprehensive unit tests for ARIS.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use serde_json::json;

use alpha_common::config::ModelsConfig;
use alpha_common::schemas::ai_resource::*;
use alpha_common::types::new_id;

use alpha_event_bus::EventBus;

use crate::types::{ResourceConstraints, TaskResult};
use crate::Aris;

/// Helper: create an ARIS instance with a temp DB and EventBus.
fn temp_aris() -> (Aris, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("test_aris.db");
    let bus_path = dir.path().join("test_event_bus.db");
    let bus = Arc::new(EventBus::open(&bus_path).expect("open event bus"));
    let aris = Aris::open(&db_path, bus).expect("open aris");
    (aris, dir)
}

/// Helper: create a test AIResource.
fn test_resource(name: &str, capabilities: Vec<(&str, f32)>) -> AIResource {
    let caps = capabilities
        .into_iter()
        .map(|(domain, score)| Capability {
            domain: domain.to_string(),
            score,
            sample_count: 0,
        })
        .collect();

    AIResource {
        id: new_id(),
        resource_type: ResourceType::LocalModel,
        name: name.to_string(),
        provider: "ollama".to_string(),
        status: ResourceStatus::Unknown,
        endpoint: "http://localhost:11434".to_string(),
        auth_method: AuthMethod::None,
        capabilities: caps,
        latency_p50_ms: None,
        cost_per_request: None,
        reliability_pct: None,
        context_window: Some(8192),
        requires_network: false,
        privacy_level: PrivacyLevel::Local,
        discovered_at: alpha_common::types::now(),
        last_health_check: None,
        metadata: json!({}),
    }
}

// ════════════════════════════════════════════════════════════════
// Registration
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_register_resource() {
    let (aris, _dir) = temp_aris();

    let resource = test_resource("test-model", vec![("text_generation", 0.8)]);
    let id = resource.id;

    let returned_id = aris.register(resource).await.unwrap();
    assert_eq!(returned_id, id);

    let all = aris.get_all().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].name, "test-model");
    assert_eq!(all[0].id, id);
}

// ════════════════════════════════════════════════════════════════
// Load from Config
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_load_from_config() {
    let (aris, _dir) = temp_aris();

    let config: ModelsConfig = toml::from_str(
        r#"
[defaults]
embedding_model = "nomic-embed-text"

[[resources]]
name = "ollama/llama3.1:8b"
provider = "ollama"
resource_type = "local_model"
endpoint = "http://localhost:11434"
context_window = 8192
privacy_level = "local"

[resources.capabilities]
text_generation = 0.7
conversation = 0.75

[[resources]]
name = "ollama/nomic-embed-text"
provider = "ollama"
resource_type = "local_model"
endpoint = "http://localhost:11434"
privacy_level = "local"

[resources.capabilities]
embedding = 1.0
"#,
    )
    .unwrap();

    let ids = aris.load_from_config(&config).await.unwrap();
    assert_eq!(ids.len(), 2);

    let all = aris.get_all().unwrap();
    assert_eq!(all.len(), 2);

    let names: Vec<&str> = all.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"ollama/llama3.1:8b"));
    assert!(names.contains(&"ollama/nomic-embed-text"));
}

// ════════════════════════════════════════════════════════════════
// Query
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_query_by_domain() {
    let (aris, _dir) = temp_aris();

    aris.register(test_resource("model-a", vec![("text_generation", 0.8), ("code_generation", 0.5)]))
        .await.unwrap();
    aris.register(test_resource("model-b", vec![("text_generation", 0.9)]))
        .await.unwrap();
    aris.register(test_resource("model-c", vec![("embedding", 1.0)]))
        .await.unwrap();

    // Query text_generation — should find model-a and model-b, sorted by score desc.
    let no_filter = ResourceConstraints {
        local_only: false,
        max_cost_usd: None,
        status_filter: None,
        min_capability_score: None,
    };
    let results = aris.query("text_generation", &no_filter).unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].resource.name, "model-b"); // 0.9 first
    assert_eq!(results[0].score, 0.9);
    assert_eq!(results[1].resource.name, "model-a"); // 0.8 second
    assert_eq!(results[1].score, 0.8);

    // Query embedding — should only find model-c.
    let results = aris.query("embedding", &no_filter).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].resource.name, "model-c");

    // Query non-existent domain — should return empty.
    let results = aris.query("image_generation", &no_filter).unwrap();
    assert!(results.is_empty());
}

#[tokio::test]
async fn test_query_local_only() {
    let (aris, _dir) = temp_aris();

    // Register a local model.
    let mut local = test_resource("local-model", vec![("text_generation", 0.7)]);
    local.privacy_level = PrivacyLevel::Local;
    aris.register(local).await.unwrap();

    // Register a cloud model.
    let mut cloud = test_resource("cloud-model", vec![("text_generation", 0.95)]);
    cloud.privacy_level = PrivacyLevel::Cloud;
    aris.register(cloud).await.unwrap();

    // Query with local_only=true.
    let constraints = ResourceConstraints {
        local_only: true,
        max_cost_usd: None,
        status_filter: None,
        min_capability_score: None,
    };
    let results = aris.query("text_generation", &constraints).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].resource.name, "local-model");
}

#[tokio::test]
async fn test_query_status_filter() {
    let (aris, _dir) = temp_aris();

    let mut online = test_resource("online-model", vec![("text_generation", 0.8)]);
    online.status = ResourceStatus::Online;
    aris.register(online).await.unwrap();

    let mut offline = test_resource("offline-model", vec![("text_generation", 0.9)]);
    offline.status = ResourceStatus::Offline;
    aris.register(offline).await.unwrap();

    // Query with status_filter=Online.
    let constraints = ResourceConstraints {
        local_only: false,
        max_cost_usd: None,
        status_filter: Some(ResourceStatus::Online),
        min_capability_score: None,
    };
    let results = aris.query("text_generation", &constraints).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].resource.name, "online-model");
}

#[tokio::test]
async fn test_query_min_capability_score() {
    let (aris, _dir) = temp_aris();

    aris.register(test_resource("weak", vec![("text_generation", 0.3)]))
        .await.unwrap();
    aris.register(test_resource("medium", vec![("text_generation", 0.6)]))
        .await.unwrap();
    aris.register(test_resource("strong", vec![("text_generation", 0.9)]))
        .await.unwrap();

    let constraints = ResourceConstraints {
        local_only: false,
        max_cost_usd: None,
        status_filter: None,
        min_capability_score: Some(0.5),
    };
    let results = aris.query("text_generation", &constraints).unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].resource.name, "strong"); // 0.9
    assert_eq!(results[1].resource.name, "medium"); // 0.6
}

// ════════════════════════════════════════════════════════════════
// Result Reporting
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_report_result_logs() {
    let (aris, _dir) = temp_aris();

    let resource = test_resource("model", vec![("text_generation", 0.7)]);
    let resource_id = resource.id;
    aris.register(resource).await.unwrap();

    let result = TaskResult {
        resource_id,
        task_domain: "text_generation".to_string(),
        success: true,
        latency_ms: 150,
        tokens_in: Some(100),
        tokens_out: Some(200),
        user_satisfaction: Some(0.9),
    };

    aris.report_result(&result).unwrap();

    let count = aris.result_count(&resource_id).unwrap();
    assert_eq!(count, 1, "Result should be logged");
}

#[tokio::test]
async fn test_report_result_no_score_change() {
    let (aris, _dir) = temp_aris();

    let resource = test_resource("model", vec![("text_generation", 0.7)]);
    let resource_id = resource.id;
    aris.register(resource).await.unwrap();

    // Record original capability score.
    let before = aris.query("text_generation", &ResourceConstraints {
        local_only: false,
        max_cost_usd: None,
        status_filter: None,
        min_capability_score: None,
    }).unwrap();
    let original_score = before[0].score;

    // Report multiple results.
    for _ in 0..5 {
        let result = TaskResult {
            resource_id,
            task_domain: "text_generation".to_string(),
            success: true,
            latency_ms: 100,
            tokens_in: Some(50),
            tokens_out: Some(100),
            user_satisfaction: Some(1.0),
        };
        aris.report_result(&result).unwrap();
    }

    // Score should NOT have changed (Sprint 1 stub).
    let after = aris.query("text_generation", &ResourceConstraints {
        local_only: false,
        max_cost_usd: None,
        status_filter: None,
        min_capability_score: None,
    }).unwrap();
    assert_eq!(
        after[0].score, original_score,
        "Sprint 1: scores must NOT change after report_result"
    );

    let count = aris.result_count(&resource_id).unwrap();
    assert_eq!(count, 5, "All 5 results should be logged");
}

// ════════════════════════════════════════════════════════════════
// Health Check
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_health_check() {
    let (aris, _dir) = temp_aris();

    let mut resource = test_resource("model", vec![("text_generation", 0.7)]);
    resource.status = ResourceStatus::Online;
    let id = resource.id;
    aris.register(resource).await.unwrap();

    let status = aris.health_check(id).unwrap();
    assert_eq!(status, ResourceStatus::Online);
}

#[tokio::test]
async fn test_health_check_not_found() {
    let (aris, _dir) = temp_aris();

    let fake_id = new_id();
    let result = aris.health_check(fake_id);
    assert!(result.is_err(), "Health check on unknown resource should error");
}

// ════════════════════════════════════════════════════════════════
// Event Publishing
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_resource_discovered_event() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("aris.db");
    let bus_path = dir.path().join("event_bus.db");
    let bus = Arc::new(EventBus::open(&bus_path).expect("open event bus"));

    // Subscribe to discovery events.
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = Arc::clone(&counter);

    bus.subscribe("alpha.aris.resource.discovered", move |event| {
        counter_clone.fetch_add(1, Ordering::SeqCst);
        // Verify the event payload contains resource info.
        assert!(event.payload.get("resource_id").is_some());
        assert!(event.payload.get("name").is_some());
    })
    .await
    .unwrap();

    let aris = Aris::open(&db_path, bus).unwrap();

    // Register 2 resources.
    aris.register(test_resource("model-1", vec![("text_generation", 0.7)]))
        .await.unwrap();
    aris.register(test_resource("model-2", vec![("embedding", 1.0)]))
        .await.unwrap();

    assert_eq!(
        counter.load(Ordering::SeqCst),
        2,
        "Two resource.discovered events should have been published"
    );
}

// ════════════════════════════════════════════════════════════════
// Persistence
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_persistence_across_restart() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("persist_aris.db");
    let bus_path = dir.path().join("event_bus.db");

    let resource_id;

    // Session 1: register a resource.
    {
        let bus = Arc::new(EventBus::open(&bus_path).unwrap());
        let aris = Aris::open(&db_path, bus).unwrap();

        let resource = test_resource("persistent-model", vec![("text_generation", 0.85)]);
        resource_id = resource.id;
        aris.register(resource).await.unwrap();

        assert_eq!(aris.get_all().unwrap().len(), 1);
    }

    // Session 2: reopen and verify.
    {
        let bus = Arc::new(EventBus::open(&bus_path).unwrap());
        let aris = Aris::open(&db_path, bus).unwrap();

        let all = aris.get_all().unwrap();
        assert_eq!(all.len(), 1, "Resource must survive restart");
        assert_eq!(all[0].name, "persistent-model");
        assert_eq!(all[0].id, resource_id);

        // Capabilities should be preserved.
        let results = aris.query("text_generation", &ResourceConstraints {
            local_only: false,
            max_cost_usd: None,
            status_filter: None,
            min_capability_score: None,
        }).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].score, 0.85);
    }
}

// ════════════════════════════════════════════════════════════════
// WAL Mode
// ════════════════════════════════════════════════════════════════

#[test]
fn test_wal_mode_enabled() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let db_path = dir.path().join("wal_test.db");
    let bus_path = dir.path().join("event_bus.db");
    let bus = Arc::new(EventBus::open(&bus_path).unwrap());
    let _aris = Aris::open(&db_path, bus).unwrap();

    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let mode: String = conn
        .query_row("PRAGMA journal_mode;", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "wal", "ARIS database must use WAL mode");
}
