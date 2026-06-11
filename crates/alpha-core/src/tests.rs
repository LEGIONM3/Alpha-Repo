//! Integration tests for Alpha Core.
//!
//! These tests exercise the full application lifecycle:
//! identity creation, persistence, events, and shutdown.

use std::sync::Arc;

use alpha_common::config::{AlphaConfig, ModelsConfig, load_config};
use alpha_common::schemas::goal::RiskLevel;
use alpha_security::SecurityGate;
use chrono::{Duration, Utc};

use crate::app::AlphaApp;

/// Create a temp directory with all required config files and return
/// (temp_dir, data_dir, config_dir).
fn setup_test_env() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let data_dir = dir.path().join("data");
    let config_dir = dir.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();

    // Write alpha.toml
    let alpha_toml = format!(
        r#"
[alpha]
data_dir = "{}"

[alpha.logging]
level = "debug"
format = "pretty"

[alpha.event_bus]
purge_after_days = 30

[alpha.identity]
name = "TestAlpha"
tone = "friendly_professional"
verbosity = 0.7
"#,
        data_dir.display().to_string().replace('\\', "/")
    );
    std::fs::write(config_dir.join("alpha.toml"), alpha_toml).unwrap();

    // Write constitution.toml
    let constitution = r#"
[purpose]
primary = "Serve as the user's long-term personal AI companion"
secondary = "Continuously improve capabilities"
non_purpose = "Not surveillance"

[principles]
user_sovereignty = true
transparency = true
privacy_first = true
do_no_harm = true
proportional_autonomy = true
honest_capability = true
persistent_growth = true
graceful_degradation = true

[autonomy]
default_trust_level = "standard"

[security]
fail_secure = true
least_privilege = true
audit_everything = true

[self_improvement]
requires_user_approval = true
sandbox_mandatory = true
regression_detection_mandatory = true
constitution_immutable = true
"#;
    std::fs::write(config_dir.join("constitution.toml"), constitution).unwrap();

    // Write models.toml
    let models = r#"
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
"#;
    std::fs::write(config_dir.join("models.toml"), models).unwrap();

    (dir, data_dir, config_dir)
}

/// Load test configs from the test environment.
fn load_test_configs(
    config_dir: &std::path::Path,
) -> (AlphaConfig, ModelsConfig, std::path::PathBuf) {
    let alpha_config: AlphaConfig =
        load_config(&config_dir.join("alpha.toml")).unwrap();
    let models_config: ModelsConfig =
        load_config(&config_dir.join("models.toml")).unwrap();
    let constitution_path = config_dir.join("constitution.toml");
    (alpha_config, models_config, constitution_path)
}

// ════════════════════════════════════════════════════════════════
// Identity Tests
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_first_run_creates_identity() {
    let (_dir, data_dir, config_dir) = setup_test_env();
    let (alpha_config, models_config, constitution_path) = load_test_configs(&config_dir);

    let app = AlphaApp::start(
        &data_dir,
        &alpha_config,
        &models_config,
        &constitution_path,
    )
    .await
    .unwrap();

    // Identity should exist.
    assert!(!app.identity.alpha_id.is_nil(), "alpha_id should be generated");
    assert_eq!(app.identity.personality.name, "TestAlpha");
    assert_eq!(app.identity.schema_version, "1.0.0");
}

#[tokio::test]
async fn test_second_run_loads_same_identity() {
    let (_dir, data_dir, config_dir) = setup_test_env();
    let (alpha_config, models_config, constitution_path) = load_test_configs(&config_dir);

    let first_id;

    // First run: create identity.
    {
        let mut app = AlphaApp::start(
            &data_dir,
            &alpha_config,
            &models_config,
            &constitution_path,
        )
        .await
        .unwrap();
        first_id = app.identity.alpha_id;
        app.shutdown().await.unwrap();
    }

    // Second run: should load the same identity.
    {
        let app = AlphaApp::start(
            &data_dir,
            &alpha_config,
            &models_config,
            &constitution_path,
        )
        .await
        .unwrap();

        assert_eq!(
            app.identity.alpha_id, first_id,
            "alpha_id must be the same across restarts"
        );
        assert_eq!(app.identity.personality.name, "TestAlpha");
    }
}

#[tokio::test]
async fn test_constitution_hash_stored() {
    let (_dir, data_dir, config_dir) = setup_test_env();
    let (alpha_config, models_config, constitution_path) = load_test_configs(&config_dir);

    let app = AlphaApp::start(
        &data_dir,
        &alpha_config,
        &models_config,
        &constitution_path,
    )
    .await
    .unwrap();

    // Constitution hash should be a valid hex string.
    assert!(
        !app.identity.constitution_hash.is_empty(),
        "Constitution hash must be computed"
    );
    assert!(
        app.identity.constitution_hash.len() == 64,
        "SHA-256 hash should be 64 hex characters, got {}",
        app.identity.constitution_hash.len()
    );

    // Verify it matches the actual file.
    let expected_hash =
        alpha_common::config::hash_file(&constitution_path).unwrap();
    assert_eq!(app.identity.constitution_hash, expected_hash);
}

// ════════════════════════════════════════════════════════════════
// Lifecycle Event Tests
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_startup_event_published() {
    let (_dir, data_dir, config_dir) = setup_test_env();
    let (alpha_config, models_config, constitution_path) = load_test_configs(&config_dir);

    let app = AlphaApp::start(
        &data_dir,
        &alpha_config,
        &models_config,
        &constitution_path,
    )
    .await
    .unwrap();

    // Replay system events from the last hour.
    let events = app
        .event_bus
        .replay(
            "alpha.system.started",
            Utc::now() - Duration::hours(1),
        )
        .unwrap();

    assert!(
        !events.is_empty(),
        "alpha.system.started event must be published on startup"
    );
    assert_eq!(events[0].event_type, "alpha.system.started");
    assert_eq!(events[0].source, "alpha-core");

    // Payload should contain the alpha_id.
    let payload_id = events[0].payload["alpha_id"]
        .as_str()
        .expect("payload should contain alpha_id");
    assert_eq!(payload_id, app.identity.alpha_id.to_string());
}

#[tokio::test]
async fn test_shutdown_event_published() {
    let (_dir, data_dir, config_dir) = setup_test_env();
    let (alpha_config, models_config, constitution_path) = load_test_configs(&config_dir);

    let mut app = AlphaApp::start(
        &data_dir,
        &alpha_config,
        &models_config,
        &constitution_path,
    )
    .await
    .unwrap();

    let event_bus = Arc::clone(&app.event_bus);

    // Shutdown.
    app.shutdown().await.unwrap();

    // Replay shutdown events.
    let events = event_bus
        .replay(
            "alpha.system.shutdown",
            Utc::now() - Duration::hours(1),
        )
        .unwrap();

    assert!(
        !events.is_empty(),
        "alpha.system.shutdown event must be published on shutdown"
    );
    assert_eq!(events[0].event_type, "alpha.system.shutdown");
    assert_eq!(events[0].source, "alpha-core");
}

// ════════════════════════════════════════════════════════════════
// ARIS Integration
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_aris_resources_loaded() {
    let (_dir, data_dir, config_dir) = setup_test_env();
    let (alpha_config, models_config, constitution_path) = load_test_configs(&config_dir);

    let app = AlphaApp::start(
        &data_dir,
        &alpha_config,
        &models_config,
        &constitution_path,
    )
    .await
    .unwrap();

    let all_resources = app.aris.get_all().unwrap();
    assert_eq!(
        all_resources.len(),
        2,
        "Two resources from models.toml should be loaded"
    );

    let names: Vec<&str> = all_resources.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"ollama/llama3.1:8b"));
    assert!(names.contains(&"ollama/nomic-embed-text"));

    // Query for text_generation should return the llama model.
    let results = app
        .aris
        .query(
            "text_generation",
            &alpha_aris::ResourceConstraints::default(),
        )
        .unwrap();
    assert!(!results.is_empty());
    assert_eq!(results[0].resource.name, "ollama/llama3.1:8b");
}

// ════════════════════════════════════════════════════════════════
// Sprint 1 Full Lifecycle
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_full_lifecycle() {
    let (_dir, data_dir, config_dir) = setup_test_env();
    let (alpha_config, models_config, constitution_path) = load_test_configs(&config_dir);

    let first_id;

    // ── Session 1 ──
    {
        let mut app = AlphaApp::start(
            &data_dir,
            &alpha_config,
            &models_config,
            &constitution_path,
        )
        .await
        .unwrap();

        first_id = app.identity.alpha_id;

        // Store a value in StateStore.
        app.state_store
            .set("test", "greeting", "hello from Alpha")
            .unwrap();

        // Evaluate a security request.
        let request = alpha_security::ActionRequest::new(
            "file.read",
            "test-agent",
            "/tmp/test.txt",
            RiskLevel::Low,
        );
        let decision = app.security_gate.evaluate(&request);
        assert!(decision.is_approved());

        // Verify ARIS resources.
        let resources = app.aris.get_all().unwrap();
        assert_eq!(resources.len(), 2);

        // Shutdown.
        app.shutdown().await.unwrap();
    }

    // ── Session 2: Verify persistence ──
    {
        let app = AlphaApp::start(
            &data_dir,
            &alpha_config,
            &models_config,
            &constitution_path,
        )
        .await
        .unwrap();

        // Same identity.
        assert_eq!(app.identity.alpha_id, first_id);

        // StateStore value persists.
        let value = app.state_store.get("test", "greeting").unwrap();
        assert_eq!(value, Some("hello from Alpha".to_string()));

        // ARIS resources persist (may have duplicates from reload, check >= 2).
        let resources = app.aris.get_all().unwrap();
        assert!(resources.len() >= 2);

        // Audit log persists.
        let audit_count = app.security_gate.count().unwrap();
        assert!(audit_count >= 1, "Audit entries should persist");

        // Events are replayable.
        let started_events = app
            .event_bus
            .replay(
                "alpha.system.started",
                Utc::now() - Duration::hours(1),
            )
            .unwrap();
        assert!(
            started_events.len() >= 2,
            "Both session startup events should be replayable"
        );
    }
}

// ════════════════════════════════════════════════════════════════
// Graceful Shutdown
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_graceful_shutdown() {
    let (_dir, data_dir, config_dir) = setup_test_env();
    let (alpha_config, models_config, constitution_path) = load_test_configs(&config_dir);

    let mut app = AlphaApp::start(
        &data_dir,
        &alpha_config,
        &models_config,
        &constitution_path,
    )
    .await
    .unwrap();

    // Shutdown should succeed without errors.
    let result = app.shutdown().await;
    assert!(result.is_ok(), "Graceful shutdown should complete without error");
}

// ════════════════════════════════════════════════════════════════
// Sprint 2 Phase 4: Integration Tests
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_phase4_initializes() {
    let (_dir, data_dir, config_dir) = setup_test_env();
    let (alpha_config, models_config, constitution_path) = load_test_configs(&config_dir);

    let app = AlphaApp::start(
        &data_dir,
        &alpha_config,
        &models_config,
        &constitution_path,
    )
    .await
    .unwrap();

    // All Sprint 2 services should be present.
    // Hardware profile should be populated.
    assert!(!app.hardware.cpu_name.is_empty(), "CPU name should be detected");
    assert!(app.hardware.cpu_cores_physical > 0, "Physical cores should be > 0");
    assert!(app.hardware.ram_total_mb > 0, "RAM should be > 0");
    assert!(!app.hardware.os.is_empty(), "OS should be detected");
    assert!(!app.hardware.arch.is_empty(), "Architecture should be detected");

    // ModelRouter should be created (endpoint resolved from config).
    assert_eq!(
        app.model_router.default_model(),
        "llama3.1:8b",
        "Default model should be resolved from models.toml"
    );
    assert_eq!(
        app.model_router.embedding_model(),
        "nomic-embed-text",
        "Embedding model should match config"
    );

    // Stores should be accessible (count returns 0 for fresh DBs).
    assert_eq!(app.memory_store.count(None, None).unwrap(), 0);
    assert_eq!(app.relationship_store.count(None).unwrap(), 0);
    assert_eq!(app.knowledge_store.count(None).unwrap(), 0);
    assert_eq!(app.explainability_store.count().unwrap(), 0);
}

#[tokio::test]
async fn test_memory_store_available() {
    let (_dir, data_dir, config_dir) = setup_test_env();
    let (alpha_config, models_config, constitution_path) = load_test_configs(&config_dir);

    let app = AlphaApp::start(
        &data_dir,
        &alpha_config,
        &models_config,
        &constitution_path,
    )
    .await
    .unwrap();

    // Store a memory and retrieve it.
    use alpha_common::schemas::memory::{MemoryRecord, MemoryType};

    let record = MemoryRecord::new(
        MemoryType::Episodic,
        "Integration test memory".to_string(),
        "alpha-core-test".to_string(),
        0.8,
    );
    let id = record.id;

    app.memory_store.store_with_embedding(&record).unwrap();

    let retrieved = app.memory_store.get(&id).unwrap().expect("should exist");
    assert_eq!(retrieved.content, "Integration test memory");
    assert_eq!(retrieved.source, "alpha-core-test");

    // Count should be 1.
    assert_eq!(app.memory_store.count(None, None).unwrap(), 1);
}

#[tokio::test]
async fn test_relationship_store_available() {
    let (_dir, data_dir, config_dir) = setup_test_env();
    let (alpha_config, models_config, constitution_path) = load_test_configs(&config_dir);

    let app = AlphaApp::start(
        &data_dir,
        &alpha_config,
        &models_config,
        &constitution_path,
    )
    .await
    .unwrap();

    // Store a relationship record.
    use alpha_common::schemas::relationship::{
        RelationshipCoreRecord, RelationshipCategory, RelationshipSource,
    };

    let record = RelationshipCoreRecord::new(
        RelationshipCategory::TrustEvolution,
        "User trusts Alpha with scheduling".to_string(),
        RelationshipSource::UserExplicit,
        0.9,
    );
    let id = record.id;

    app.relationship_store.store_with_embedding(&record).unwrap();

    let retrieved = app.relationship_store.get(&id).unwrap().expect("should exist");
    assert_eq!(retrieved.content, "User trusts Alpha with scheduling");
    assert!(retrieved.protected, "Invariant: protected must be true");
    assert!(retrieved.decay_rate.abs() < 0.000001, "Invariant: decay_rate must be ~0.0");
    assert_eq!(retrieved.governance_state, "active", "Invariant: governance must be active");

    assert_eq!(app.relationship_store.count(None).unwrap(), 1);
}

#[tokio::test]
async fn test_knowledge_store_available() {
    let (_dir, data_dir, config_dir) = setup_test_env();
    let (alpha_config, models_config, constitution_path) = load_test_configs(&config_dir);

    let app = AlphaApp::start(
        &data_dir,
        &alpha_config,
        &models_config,
        &constitution_path,
    )
    .await
    .unwrap();

    // Store a knowledge entity.
    use alpha_knowledge::KnowledgeEntity;

    let entity = KnowledgeEntity::new("person", "TestUser", "alpha-core-test");
    let id = entity.id;

    app.knowledge_store.store(&entity).unwrap();

    let retrieved = app.knowledge_store.get(&id).unwrap().expect("should exist");
    assert_eq!(retrieved.name, "TestUser");
    assert_eq!(retrieved.entity_type, "person");

    assert_eq!(app.knowledge_store.count(None).unwrap(), 1);
}

#[tokio::test]
async fn test_explainability_store_available() {
    let (_dir, data_dir, config_dir) = setup_test_env();
    let (alpha_config, models_config, constitution_path) = load_test_configs(&config_dir);

    let app = AlphaApp::start(
        &data_dir,
        &alpha_config,
        &models_config,
        &constitution_path,
    )
    .await
    .unwrap();

    // Store an explainability record.
    use alpha_common::schemas::explainability::{ExplainabilityRecord, ExplanationType};
    use alpha_common::types::new_id;

    let record = ExplainabilityRecord::new(
        ExplanationType::Action,
        new_id(),
        "Testing explainability integration".to_string(),
        new_id(),
    );
    let id = record.id;

    app.explainability_store.store(&record).unwrap();

    let retrieved = app.explainability_store.get(&id).unwrap().expect("should exist");
    assert_eq!(retrieved.summary, "Testing explainability integration");

    assert_eq!(app.explainability_store.count().unwrap(), 1);
}

#[tokio::test]
async fn test_hardware_profile_populated() {
    let (_dir, data_dir, config_dir) = setup_test_env();
    let (alpha_config, models_config, constitution_path) = load_test_configs(&config_dir);

    let app = AlphaApp::start(
        &data_dir,
        &alpha_config,
        &models_config,
        &constitution_path,
    )
    .await
    .unwrap();

    // Hardware profile should match the actual system.
    let hw = &app.hardware;

    assert!(!hw.cpu_name.is_empty(), "CPU brand should be detected");
    assert!(hw.cpu_cores_physical > 0, "At least 1 physical core");
    assert!(
        hw.cpu_cores_logical >= hw.cpu_cores_physical,
        "Logical cores >= physical cores"
    );
    assert!(hw.ram_total_mb > 512, "System should have > 512 MB RAM");
    assert!(
        hw.ram_available_mb > 0,
        "Some RAM should be available"
    );
    assert!(!hw.os.is_empty(), "OS should be detected");
    assert!(!hw.arch.is_empty(), "Architecture should be detected");

    // On the test machine (Windows), verify OS.
    #[cfg(target_os = "windows")]
    assert_eq!(hw.os, "windows");

    #[cfg(target_os = "linux")]
    assert_eq!(hw.os, "linux");

    #[cfg(target_os = "macos")]
    assert_eq!(hw.os, "macos");
}

#[tokio::test]
async fn test_full_lifecycle_sprint2() {
    let (_dir, data_dir, config_dir) = setup_test_env();
    let (alpha_config, models_config, constitution_path) = load_test_configs(&config_dir);

    let first_id;

    // ── Session 1: Create and populate ──
    {
        let mut app = AlphaApp::start(
            &data_dir,
            &alpha_config,
            &models_config,
            &constitution_path,
        )
        .await
        .unwrap();

        first_id = app.identity.alpha_id;

        // Sprint 1: Store data.
        app.state_store
            .set("test", "sprint2", "integrated")
            .unwrap();

        // Sprint 2: Store a memory.
        use alpha_common::schemas::memory::{MemoryRecord, MemoryType};
        let mem = MemoryRecord::new(
            MemoryType::Episodic,
            "Session 1 memory".to_string(),
            "lifecycle-test".to_string(),
            0.7,
        );
        app.memory_store.store_with_embedding(&mem).unwrap();

        // Sprint 2: Store a relationship.
        use alpha_common::schemas::relationship::{
            RelationshipCoreRecord, RelationshipCategory, RelationshipSource,
        };
        let rel = RelationshipCoreRecord::new(
            RelationshipCategory::SharedHistory,
            "First integration test".to_string(),
            RelationshipSource::AlphaObserved,
            0.6,
        );
        app.relationship_store.store_with_embedding(&rel).unwrap();

        // Sprint 2: Store knowledge.
        use alpha_knowledge::KnowledgeEntity;
        let entity = KnowledgeEntity::new("event", "Sprint2Launch", "lifecycle-test");
        app.knowledge_store.store(&entity).unwrap();

        // Shutdown.
        app.shutdown().await.unwrap();
    }

    // ── Session 2: Verify persistence across restart ──
    {
        let app = AlphaApp::start(
            &data_dir,
            &alpha_config,
            &models_config,
            &constitution_path,
        )
        .await
        .unwrap();

        // Same identity.
        assert_eq!(app.identity.alpha_id, first_id);

        // Sprint 1 persists.
        let value = app.state_store.get("test", "sprint2").unwrap();
        assert_eq!(value, Some("integrated".to_string()));

        // Sprint 2 stores persist.
        assert_eq!(
            app.memory_store.count(None, None).unwrap(),
            1,
            "Memory should persist across restart"
        );
        assert_eq!(
            app.relationship_store.count(None).unwrap(),
            1,
            "Relationship should persist across restart"
        );
        assert_eq!(
            app.knowledge_store.count(None).unwrap(),
            1,
            "Knowledge should persist across restart"
        );

        // Hardware should still be detected.
        assert!(app.hardware.cpu_cores_physical > 0);

        // Events from both sessions should be replayable.
        let started_events = app
            .event_bus
            .replay(
                "alpha.system.started",
                Utc::now() - Duration::hours(1),
            )
            .unwrap();
        assert!(
            started_events.len() >= 2,
            "Both session startup events should be replayable, got {}",
            started_events.len()
        );

        // Started event should contain hardware info.
        let last_event = started_events.last().unwrap();
        assert!(
            last_event.payload.get("hardware").is_some(),
            "Started event should contain hardware profile"
        );
    }
}

// ════════════════════════════════════════════════════════════════
// Sprint 3 Phase 5: Integration Tests
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_phase5_initializes() {
    let (_dir, data_dir, config_dir) = setup_test_env();
    let (alpha_config, models_config, constitution_path) = load_test_configs(&config_dir);

    let app = AlphaApp::start(
        &data_dir,
        &alpha_config,
        &models_config,
        &constitution_path,
    )
    .await
    .unwrap();

    // SessionManager should be functional.
    assert_eq!(
        app.session_manager.count(None).unwrap(),
        0,
        "No sessions should exist in a fresh database"
    );

    // ConversationService should be wired up (verified by build_prompt_messages).
    let context = alpha_context::ContextAssembler::new(
        &app.memory_store,
        &app.relationship_store,
        &app.knowledge_store,
        &app.identity,
        alpha_context::ContextConfig::default(),
    )
    .assemble("test", &[], &[])
    .unwrap();

    let messages = alpha_conversation::ConversationService::<crate::provider::SyncModelProvider>::build_prompt_messages(
        &context,
        "test",
    );

    assert!(messages.len() >= 2, "Prompt should have system + user messages");

    // Startup event should contain conversation_ready.
    let events = app
        .event_bus
        .replay(
            "alpha.system.started",
            Utc::now() - Duration::hours(1),
        )
        .unwrap();
    let latest = events.last().unwrap();
    assert_eq!(
        latest.payload["conversation_ready"].as_bool(),
        Some(true),
        "system.started should have conversation_ready=true"
    );
}

#[tokio::test]
async fn test_session_manager_available() {
    let (_dir, data_dir, config_dir) = setup_test_env();
    let (alpha_config, models_config, constitution_path) = load_test_configs(&config_dir);

    let app = AlphaApp::start(
        &data_dir,
        &alpha_config,
        &models_config,
        &constitution_path,
    )
    .await
    .unwrap();

    // Create a session.
    let session_id = app.session_manager.create_session().unwrap();

    // Session should be retrievable.
    let session = app
        .session_manager
        .get_session(&session_id)
        .unwrap()
        .expect("Session should exist");

    assert_eq!(session.status, alpha_dialog::SessionStatus::Active);
    assert_eq!(session.turn_count, 0);

    // Add a turn.
    app.session_manager
        .add_user_turn(&session_id, "Hello from integration test")
        .unwrap();

    let session = app
        .session_manager
        .get_session(&session_id)
        .unwrap()
        .unwrap();
    assert_eq!(session.turn_count, 1);

    // WAL mode should be enabled.
    assert!(app.session_manager.is_wal_mode().unwrap());
}

#[tokio::test]
async fn test_conversation_service_available() {
    let (_dir, data_dir, config_dir) = setup_test_env();
    let (alpha_config, models_config, constitution_path) = load_test_configs(&config_dir);

    let app = AlphaApp::start(
        &data_dir,
        &alpha_config,
        &models_config,
        &constitution_path,
    )
    .await
    .unwrap();

    // ConversationService should be present and have its dependencies wired.
    // We verify this by checking that build_prompt_messages works correctly.
    let context = alpha_context::ContextAssembler::new(
        &app.memory_store,
        &app.relationship_store,
        &app.knowledge_store,
        &app.identity,
        alpha_context::ContextConfig::default(),
    )
    .assemble("test", &[], &[])
    .unwrap();

    let messages = alpha_conversation::ConversationService::<crate::provider::SyncModelProvider>::build_prompt_messages(
        &context,
        "What can you do?",
    );

    // System message should contain Alpha's identity.
    assert_eq!(
        messages[0].role,
        alpha_model_router::types::ChatRole::System
    );
    assert!(messages[0].content.contains("TestAlpha"));

    // User message should be last.
    let last = messages.last().unwrap();
    assert_eq!(last.role, alpha_model_router::types::ChatRole::User);
    assert_eq!(last.content, "What can you do?");
}

#[tokio::test]
async fn test_full_lifecycle_sprint3() {
    let (_dir, data_dir, config_dir) = setup_test_env();
    let (alpha_config, models_config, constitution_path) = load_test_configs(&config_dir);

    let first_id;

    // ── Session 1: Create session and add turns ──
    {
        let mut app = AlphaApp::start(
            &data_dir,
            &alpha_config,
            &models_config,
            &constitution_path,
        )
        .await
        .unwrap();

        first_id = app.identity.alpha_id;

        // Sprint 3: Create a dialog session.
        let session_id = app.session_manager.create_session().unwrap();
        app.session_manager.set_title(&session_id, "Lifecycle Test").unwrap();

        app.session_manager
            .add_user_turn(&session_id, "Hello Alpha")
            .unwrap();
        app.session_manager
            .add_alpha_turn(&session_id, "Hello! How can I help?", "llama3.1:8b", 15, 500)
            .unwrap();

        let session = app.session_manager.get_session(&session_id).unwrap().unwrap();
        assert_eq!(session.turn_count, 2);

        // Sprint 2: Store a memory.
        use alpha_common::schemas::memory::{MemoryRecord, MemoryType};
        let mem = MemoryRecord::new(
            MemoryType::Episodic,
            "Sprint 3 lifecycle memory".to_string(),
            "lifecycle-test".to_string(),
            0.6,
        );
        app.memory_store.store_with_embedding(&mem).unwrap();

        app.shutdown().await.unwrap();
    }

    // ── Session 2: Verify persistence ──
    {
        let app = AlphaApp::start(
            &data_dir,
            &alpha_config,
            &models_config,
            &constitution_path,
        )
        .await
        .unwrap();

        // Same identity.
        assert_eq!(app.identity.alpha_id, first_id);

        // Sprint 3: Dialog session persists.
        let sessions = app.session_manager.list_sessions(None, 10, 0).unwrap();
        assert!(!sessions.is_empty(), "Dialog sessions should persist");

        let session = &sessions[0];
        assert_eq!(session.title, "Lifecycle Test");
        assert_eq!(session.turn_count, 2);

        // Turns persist.
        let turns = app.session_manager.get_turns(&session.id).unwrap();
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].content, "Hello Alpha");
        assert_eq!(turns[1].content, "Hello! How can I help?");

        // Sprint 2: Memory persists.
        assert_eq!(app.memory_store.count(None, None).unwrap(), 1);

        // Sprint 3 session count.
        assert_eq!(app.session_manager.count(None).unwrap(), 1);

        // Startup events should include conversation_ready.
        let events = app
            .event_bus
            .replay(
                "alpha.system.started",
                Utc::now() - Duration::hours(1),
            )
            .unwrap();
        assert!(events.len() >= 2, "Both startup events should be replayable");
        let latest = events.last().unwrap();
        assert_eq!(
            latest.payload["conversation_ready"].as_bool(),
            Some(true),
        );
    }
}

#[tokio::test]
async fn test_conversation_event_published() {
    let (_dir, data_dir, config_dir) = setup_test_env();
    let (alpha_config, models_config, constitution_path) = load_test_configs(&config_dir);

    let app = AlphaApp::start(
        &data_dir,
        &alpha_config,
        &models_config,
        &constitution_path,
    )
    .await
    .unwrap();

    // The system.started event should include conversation_ready=true.
    let events = app
        .event_bus
        .replay(
            "alpha.system.started",
            Utc::now() - Duration::hours(1),
        )
        .unwrap();

    assert!(!events.is_empty());
    let latest = events.last().unwrap();
    assert_eq!(
        latest.payload["conversation_ready"].as_bool(),
        Some(true),
        "system.started event should signal conversation readiness"
    );

    // Verify that the event contains all expected Sprint 3 fields.
    assert!(
        latest.payload.get("hardware").is_some(),
        "Started event should contain hardware info"
    );
    assert!(
        latest.payload.get("alpha_id").is_some(),
        "Started event should contain alpha_id"
    );
}
