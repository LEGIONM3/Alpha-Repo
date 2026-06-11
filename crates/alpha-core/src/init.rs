//! Startup initialization for Project Alpha.
//!
//! Implements the five-phase startup sequence:
//! - Phase 1: Infrastructure (EventBus, StateStore)
//! - Phase 2: Identity
//! - Phase 3: Security + ARIS
//! - Phase 4: Sprint 2 services (Hardware, ModelRouter, stores)
//! - Phase 5: Sprint 3 conversation systems (SessionManager, ConversationService)

use std::path::Path;
use std::sync::Arc;

use tracing::{info, warn};

use alpha_common::config::{AlphaConfig, ModelsConfig};
use alpha_common::error::AlphaError;
use alpha_common::event::Event;
use alpha_common::schemas::identity::AlphaIdentity;

use alpha_aris::Aris;
use alpha_context::ContextConfig;
use alpha_conversation::ConversationService;
use alpha_dialog::SessionManager;
use alpha_event_bus::EventBus;
use alpha_explainability::ExplainabilityStore;
use alpha_knowledge::KnowledgeStore;
use alpha_memory::MemoryStore;
use alpha_model_router::ModelRouter;
use alpha_relationship::RelationshipStore;
use alpha_resource::HardwareProfile;
use alpha_security::AllowAllGate;
use alpha_state::StateStore;

use crate::identity;
use crate::provider::SyncModelProvider;

/// Result of the full initialization sequence.
pub struct InitResult {
    // Sprint 1
    pub event_bus: Arc<EventBus>,
    pub state_store: StateStore,
    pub identity: AlphaIdentity,
    pub security_gate: AllowAllGate,
    pub aris: Arc<Aris>,
    // Sprint 2
    pub hardware: HardwareProfile,
    pub model_router: ModelRouter,
    pub memory_store: Arc<MemoryStore>,
    pub relationship_store: Arc<RelationshipStore>,
    pub knowledge_store: Arc<KnowledgeStore>,
    pub explainability_store: ExplainabilityStore,
    // Sprint 3
    pub session_manager: Arc<SessionManager>,
    pub conversation_service: ConversationService<SyncModelProvider>,
}

/// Run the full five-phase startup sequence.
pub async fn initialize(
    data_dir: &Path,
    alpha_config: &AlphaConfig,
    models_config: &ModelsConfig,
    constitution_path: &Path,
) -> Result<InitResult, AlphaError> {
    // Ensure data directory exists.
    if !data_dir.exists() {
        std::fs::create_dir_all(data_dir).map_err(|e| {
            AlphaError::Config(format!(
                "Failed to create data directory '{}': {}",
                data_dir.display(),
                e
            ))
        })?;
    }

    // ── Phase 1: Infrastructure ──
    info!("Phase 1: Initializing infrastructure...");

    let event_bus_path = data_dir.join("event_bus.db");
    let event_bus = Arc::new(EventBus::open(&event_bus_path)?);

    let state_path = data_dir.join("state.db");
    let state_store = StateStore::open(&state_path)?;

    info!("Phase 1 complete: Infrastructure ready.");

    // ── Phase 2: Identity ──
    info!("Phase 2: Loading identity...");

    let identity = identity::load_or_create_identity(
        &state_store,
        alpha_config,
        constitution_path,
    )?;

    info!(
        alpha_id = %identity.alpha_id,
        "Phase 2 complete: Identity verified."
    );

    // ── Phase 3: Security + ARIS ──
    info!("Phase 3: Initializing security and ARIS...");

    let audit_path = data_dir.join("audit.db");
    let security_gate = AllowAllGate::open(&audit_path)?;

    let aris_path = data_dir.join("aris.db");
    let aris = Arc::new(Aris::open(&aris_path, Arc::clone(&event_bus))?);

    // Load resources from config.
    let resource_ids = aris.load_from_config(models_config).await?;
    info!(
        count = resource_ids.len(),
        "Registered AI resources from config."
    );

    info!("Phase 3 complete: Security + ARIS ready.");

    // ── Phase 4: Sprint 2 Services ──
    info!("Phase 4: Initializing Sprint 2 services...");

    // 4a. Hardware detection.
    let hardware = alpha_resource::detect_hardware();
    info!(
        cpu = %hardware.cpu_name,
        physical_cores = hardware.cpu_cores_physical,
        logical_cores = hardware.cpu_cores_logical,
        ram_total_mb = hardware.ram_total_mb,
        os = %hardware.os,
        arch = %hardware.arch,
        "Hardware detected."
    );

    // 4b. ModelRouter creation.
    let ollama_endpoint = resolve_ollama_endpoint(models_config);
    let default_model = resolve_default_model(models_config);
    let embedding_model = &models_config.defaults.embedding_model;

    let model_router = ModelRouter::new(
        &ollama_endpoint,
        &default_model,
        embedding_model,
        Arc::clone(&aris),
        Arc::clone(&event_bus),
    )?;

    // 4c. Health check (best-effort, non-blocking).
    let health = model_router.health_check().await;
    if health.server_reachable {
        info!(
            chat_model = health.chat_model_available,
            embedding_model = health.embedding_model_available,
            "Ollama health check: server reachable."
        );
    } else {
        warn!("Ollama health check: server unreachable. Inference will fail until Ollama is started.");
    }

    // 4d. MemoryStore.
    let memory_path = data_dir.join("memory.db");
    let memory_store = Arc::new(MemoryStore::open(&memory_path, 0)?);
    info!("MemoryStore opened.");

    // 4e. RelationshipStore.
    let relationship_path = data_dir.join("relationship.db");
    let relationship_store = Arc::new(RelationshipStore::open(&relationship_path)?);
    info!("RelationshipStore opened.");

    // 4f. KnowledgeStore.
    let knowledge_path = data_dir.join("knowledge.db");
    let knowledge_store = Arc::new(KnowledgeStore::open(&knowledge_path)?);
    info!("KnowledgeStore opened.");

    // 4g. ExplainabilityStore.
    let explainability_path = data_dir.join("explainability.db");
    let explainability_store = ExplainabilityStore::open(&explainability_path)?;
    info!("ExplainabilityStore opened.");

    info!("Phase 4 complete: Sprint 2 services ready.");

    // ── Phase 5: Sprint 3 Conversation Systems ──
    info!("Phase 5: Initializing conversation systems...");

    // 5a. SessionManager.
    let dialog_path = data_dir.join("dialog.db");
    let session_manager = Arc::new(SessionManager::open(&dialog_path)?);
    info!("SessionManager opened.");

    // 5b. SyncModelProvider (wraps ModelRouter for thread-safe access).
    let sync_provider = Arc::new(SyncModelProvider::new(model_router));

    // 5c. ConversationService.
    let conversation_service = ConversationService::new(
        Arc::clone(&sync_provider),
        Arc::clone(&memory_store),
        Arc::clone(&relationship_store),
        Arc::clone(&knowledge_store),
        Arc::clone(&session_manager),
        Arc::clone(&event_bus),
        Arc::new(identity.clone()),
        ContextConfig::default(),
    );
    info!("ConversationService created.");

    info!("Phase 5 complete: Conversation systems ready.");

    // ── Publish system.started ──
    let started_event = Event::new(
        alpha_common::topics::SYSTEM_STARTED,
        "alpha-core",
        serde_json::json!({
            "alpha_id": identity.alpha_id.to_string(),
            "schema_version": identity.schema_version,
            "hardware": {
                "cpu": hardware.cpu_name,
                "cores": hardware.cpu_cores_logical,
                "ram_mb": hardware.ram_total_mb,
                "os": hardware.os,
                "arch": hardware.arch,
            },
            "ollama_reachable": health.server_reachable,
            "conversation_ready": true,
        }),
    );
    event_bus.publish(started_event).await?;

    info!(
        alpha_id = %identity.alpha_id,
        "Alpha is alive."
    );

    // Reconstruct a bare ModelRouter for direct access (diagnostics, health checks).
    // The SyncModelProvider owns one for conversation use.
    let diagnostic_router = ModelRouter::new(
        &ollama_endpoint,
        &default_model,
        embedding_model,
        Arc::clone(&aris),
        Arc::clone(&event_bus),
    )?;

    Ok(InitResult {
        event_bus,
        state_store,
        identity,
        security_gate,
        aris,
        hardware,
        model_router: diagnostic_router,
        memory_store,
        relationship_store,
        knowledge_store,
        explainability_store,
        session_manager,
        conversation_service,
    })
}

/// Resolve the Ollama endpoint from models.toml resources.
///
/// Looks for the first resource with provider "ollama" and uses its endpoint.
/// Falls back to `http://localhost:11434` if none found.
fn resolve_ollama_endpoint(models_config: &ModelsConfig) -> String {
    models_config
        .resources
        .iter()
        .find(|r| r.provider == "ollama")
        .map(|r| r.endpoint.clone())
        .unwrap_or_else(|| "http://localhost:11434".to_string())
}

/// Resolve the default chat model from models.toml resources.
///
/// Uses the first Ollama resource with text_generation or conversation capability.
/// Falls back to the first Ollama resource name.
fn resolve_default_model(models_config: &ModelsConfig) -> String {
    // Prefer a model with text_generation or conversation capability.
    if let Some(r) = models_config.resources.iter().find(|r| {
        r.provider == "ollama"
            && (r.capabilities.contains_key("text_generation")
                || r.capabilities.contains_key("conversation"))
    }) {
        return strip_provider_prefix(&r.name);
    }

    // Fallback to first Ollama resource.
    models_config
        .resources
        .iter()
        .find(|r| r.provider == "ollama")
        .map(|r| strip_provider_prefix(&r.name))
        .unwrap_or_else(|| "llama3.1:8b".to_string())
}

/// Strip "ollama/" prefix from a model name if present.
fn strip_provider_prefix(name: &str) -> String {
    name.strip_prefix("ollama/")
        .unwrap_or(name)
        .to_string()
}
