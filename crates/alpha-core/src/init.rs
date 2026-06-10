//! Startup initialization for Project Alpha.
//!
//! Implements the three-phase startup sequence:
//! - Phase 1: Infrastructure (EventBus, StateStore)
//! - Phase 2: Identity
//! - Phase 3: Security + ARIS

use std::path::Path;
use std::sync::Arc;

use tracing::info;

use alpha_common::config::{AlphaConfig, ModelsConfig};
use alpha_common::error::AlphaError;
use alpha_common::event::Event;
use alpha_common::schemas::identity::AlphaIdentity;

use alpha_aris::Aris;
use alpha_event_bus::EventBus;
use alpha_security::AllowAllGate;
use alpha_state::StateStore;

use crate::identity;

/// Result of the full initialization sequence.
pub struct InitResult {
    pub event_bus: Arc<EventBus>,
    pub state_store: StateStore,
    pub identity: AlphaIdentity,
    pub security_gate: AllowAllGate,
    pub aris: Aris,
}

/// Run the full three-phase startup sequence.
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
    let aris = Aris::open(&aris_path, Arc::clone(&event_bus))?;

    // Load resources from config.
    let resource_ids = aris.load_from_config(models_config).await?;
    info!(
        count = resource_ids.len(),
        "Registered AI resources from config."
    );

    info!("Phase 3 complete: Security + ARIS ready.");

    // ── Publish system.started ──
    let started_event = Event::new(
        alpha_common::topics::SYSTEM_STARTED,
        "alpha-core",
        serde_json::json!({
            "alpha_id": identity.alpha_id.to_string(),
            "schema_version": identity.schema_version,
        }),
    );
    event_bus.publish(started_event).await?;

    info!(
        alpha_id = %identity.alpha_id,
        "Alpha is alive."
    );

    Ok(InitResult {
        event_bus,
        state_store,
        identity,
        security_gate,
        aris,
    })
}
