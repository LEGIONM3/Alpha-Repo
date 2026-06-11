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
/// Shutdown order (reverse of startup):
/// 1. Publish `alpha.system.shutdown` event (while EventBus is still alive).
/// 2. Sprint 3 services (ConversationService, SessionManager).
/// 3. Sprint 2 stores (ExplainabilityStore, KnowledgeStore, RelationshipStore, MemoryStore).
///    — Drop order: reverse of Phase 4 open order.
/// 4. ModelRouter (drop).
/// 5. ARIS (flush pending result_log writes).
/// 6. SecurityGate (flush pending audit writes).
/// 7. StateStore (final sync).
/// 8. EventBus (final sync, close SQLite).
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

    // 2. Sprint 3 services — reverse of Phase 5 startup order.
    //    ConversationService is dropped first (it holds Arcs to stores),
    //    then SessionManager.
    info!("Releasing Sprint 3 conversation services...");
    // ConversationService and SessionManager are dropped when AlphaApp is dropped.
    // The Arc refs to stores will be released, allowing Sprint 2 cleanup.

    // 3. Sprint 2 stores — these are plain structs with Mutex<Connection>.
    //    Dropping them releases the SQLite connection.
    //    Explicit log for observability.
    info!("Releasing Sprint 2 stores...");
    // Stores are dropped when AlphaApp is dropped; no explicit shutdown needed.
    // The SQLite connections will be closed and WAL checkpointed on drop.

    // 4. ARIS — behind Arc (shared with ModelRouter). WAL checkpoints on Connection drop.
    info!("ARIS: resources will be released on drop.");

    // 5. SecurityGate shutdown.
    if let Err(e) = app.security_gate.shutdown() {
        tracing::error!(error = %e, "SecurityGate shutdown error — continuing");
    } else {
        info!("SecurityGate shutdown complete.");
    }

    // 6. StateStore shutdown.
    if let Err(e) = app.state_store.shutdown() {
        tracing::error!(error = %e, "StateStore shutdown error — continuing");
    } else {
        info!("StateStore shutdown complete.");
    }

    // 7. EventBus shutdown.
    // EventBus is behind Arc — will flush when the last Arc is dropped.
    info!("EventBus shutdown: releasing resources.");

    info!("Alpha shutdown complete.");
    Ok(())
}
