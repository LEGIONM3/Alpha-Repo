//! AlphaApp — the top-level application struct for Project Alpha.
//!
//! Owns all Sprint 1, Sprint 2, and Sprint 3 services and provides access to them.

use std::path::Path;
use std::sync::Arc;

use alpha_common::config::{AlphaConfig, ModelsConfig};
use alpha_common::error::AlphaError;
use alpha_common::schemas::identity::AlphaIdentity;

use alpha_aris::Aris;
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

use crate::init;
use crate::provider::SyncModelProvider;
use crate::shutdown;

/// The top-level Alpha application.
///
/// Owns all Sprint 1, Sprint 2, and Sprint 3 services.
/// Created via [`AlphaApp::start()`], shut down via [`AlphaApp::shutdown()`].
pub struct AlphaApp {
    // ── Sprint 1 Services ──
    /// The central event bus.
    pub event_bus: Arc<EventBus>,
    /// Persistent key-value and document store.
    pub state_store: StateStore,
    /// Alpha's core identity.
    pub identity: AlphaIdentity,
    /// Security gate (AllowAll in Sprint 1).
    pub security_gate: AllowAllGate,
    /// AI Resource Intelligence System.
    pub aris: Arc<Aris>,

    // ── Sprint 2 Services ──
    /// Detected hardware capabilities.
    pub hardware: HardwareProfile,
    /// Model router for LLM inference and embeddings.
    pub model_router: ModelRouter,
    /// Memory persistence and semantic search.
    pub memory_store: Arc<MemoryStore>,
    /// Protected relationship core persistence.
    pub relationship_store: Arc<RelationshipStore>,
    /// Knowledge entity persistence.
    pub knowledge_store: Arc<KnowledgeStore>,
    /// Explainability record persistence.
    pub explainability_store: ExplainabilityStore,

    // ── Sprint 3 Services ──
    /// Dialog session manager.
    pub session_manager: Arc<SessionManager>,
    /// Conversation service (orchestrates the message pipeline).
    pub conversation_service: ConversationService<SyncModelProvider>,
}

impl AlphaApp {
    /// Start the Alpha application.
    ///
    /// Runs the full five-phase initialization sequence:
    /// 1. Infrastructure (EventBus, StateStore)
    /// 2. Identity (create or load)
    /// 3. Security + ARIS
    /// 4. Sprint 2 services (Hardware, ModelRouter, stores)
    /// 5. Sprint 3 conversation systems (SessionManager, ConversationService)
    ///
    /// Returns a fully initialized `AlphaApp` ready to operate.
    pub async fn start(
        data_dir: &Path,
        alpha_config: &AlphaConfig,
        models_config: &ModelsConfig,
        constitution_path: &Path,
    ) -> Result<Self, AlphaError> {
        let result = init::initialize(
            data_dir,
            alpha_config,
            models_config,
            constitution_path,
        )
        .await?;

        Ok(Self {
            event_bus: result.event_bus,
            state_store: result.state_store,
            identity: result.identity,
            security_gate: result.security_gate,
            aris: result.aris,
            hardware: result.hardware,
            model_router: result.model_router,
            memory_store: result.memory_store,
            relationship_store: result.relationship_store,
            knowledge_store: result.knowledge_store,
            explainability_store: result.explainability_store,
            session_manager: result.session_manager,
            conversation_service: result.conversation_service,
        })
    }

    /// Gracefully shut down all services in reverse startup order.
    pub async fn shutdown(&mut self) -> Result<(), AlphaError> {
        shutdown::shutdown(self).await
    }
}
