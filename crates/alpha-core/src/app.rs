//! AlphaApp — the top-level application struct for Project Alpha.
//!
//! Owns all Sprint 1 services and provides access to them.

use std::path::Path;
use std::sync::Arc;

use alpha_common::config::{AlphaConfig, ModelsConfig};
use alpha_common::error::AlphaError;
use alpha_common::schemas::identity::AlphaIdentity;

use alpha_aris::Aris;
use alpha_event_bus::EventBus;
use alpha_security::AllowAllGate;
use alpha_state::StateStore;

use crate::init;
use crate::shutdown;

/// The top-level Alpha application.
///
/// Owns all Sprint 1 services. Created via [`AlphaApp::start()`],
/// shut down via [`AlphaApp::shutdown()`].
pub struct AlphaApp {
    /// The central event bus.
    pub event_bus: Arc<EventBus>,
    /// Persistent key-value and document store.
    pub state_store: StateStore,
    /// Alpha's core identity.
    pub identity: AlphaIdentity,
    /// Security gate (AllowAll in Sprint 1).
    pub security_gate: AllowAllGate,
    /// AI Resource Intelligence System.
    pub aris: Aris,
}

impl AlphaApp {
    /// Start the Alpha application.
    ///
    /// Runs the full three-phase initialization sequence:
    /// 1. Infrastructure (EventBus, StateStore)
    /// 2. Identity (create or load)
    /// 3. Security + ARIS
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
        })
    }

    /// Gracefully shut down all services in reverse startup order.
    pub async fn shutdown(&mut self) -> Result<(), AlphaError> {
        shutdown::shutdown(self).await
    }
}
