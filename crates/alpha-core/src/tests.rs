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
// Full Lifecycle
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
