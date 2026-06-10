//! Graceful shutdown for Project Alpha.
//!
//! Shutdown is **reverse startup order**: services started last are stopped first.
//! This ensures dependencies are still available when a service needs to flush state.

use tracing::info;

use alpha_common::error::AlphaError;
use alpha_common::event::Event;
use alpha_common::traits::Service;

use crate::app::AlphaApp;

/// Perform graceful shutdown of all services.
///
/// Shutdown order:
/// 1. Publish `alpha.system.shutdown` event (while EventBus is still alive).
/// 2. ARIS (flush pending result_log writes).
/// 3. SecurityGate (flush pending audit writes).
/// 4. StateStore (final sync).
/// 5. EventBus (final sync, close SQLite).
pub async fn shutdown(app: &mut AlphaApp) -> Result<(), AlphaError> {
    info!("Initiating graceful shutdown...");

    // 1. Publish shutdown event (EventBus is still alive).
    let shutdown_event = Event::new(
        alpha_common::topics::SYSTEM_SHUTDOWN,
        "alpha-core",
        serde_json::json!({
            "alpha_id": app.identity.alpha_id.to_string(),
            "reason": "graceful_shutdown",
        }),
    );

    if let Err(e) = app.event_bus.publish(shutdown_event).await {
        tracing::error!(error = %e, "Failed to publish shutdown event — continuing shutdown");
    }

    // 2. Shutdown ARIS.
    if let Err(e) = app.aris.shutdown() {
        tracing::error!(error = %e, "ARIS shutdown error — continuing");
    } else {
        info!("ARIS shutdown complete.");
    }

    // 3. Shutdown SecurityGate.
    if let Err(e) = app.security_gate.shutdown() {
        tracing::error!(error = %e, "SecurityGate shutdown error — continuing");
    } else {
        info!("SecurityGate shutdown complete.");
    }

    // 4. Shutdown StateStore.
    if let Err(e) = app.state_store.shutdown() {
        tracing::error!(error = %e, "StateStore shutdown error — continuing");
    } else {
        info!("StateStore shutdown complete.");
    }

    // 5. EventBus shutdown (WAL checkpoint).
    // We need mutable access, but EventBus is behind an Arc.
    // For Sprint 1, WAL checkpoint happens via ARIS/Security's own shutdown.
    // The EventBus itself will flush when dropped.
    info!("EventBus shutdown: releasing resources.");

    info!("Alpha shutdown complete.");
    Ok(())
}
