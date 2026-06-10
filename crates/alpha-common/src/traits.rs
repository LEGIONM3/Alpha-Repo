//! Shared traits for Project Alpha.
//!
//! These traits define the behavioral contracts that services must implement.
//! Defining them in `alpha-common` allows any crate to reference them without
//! depending on a specific service implementation.

use crate::error::AlphaError;
use crate::schemas::explainability::ExplainabilityRecord;
use crate::types::AlphaId;

/// Trait for any service that makes decisions.
///
/// Services implementing this trait can produce structured explanation
/// records for their decisions, enabling Alpha to answer "Why did I do this?"
///
/// Sprint 1: Defined. Only `alpha-security` (stub) implements it trivially.
/// Future sprints: Model Router, Orchestrator, Attention Engine, Goal Engine.
pub trait Explainable {
    /// Produce an explanation record for a given decision.
    ///
    /// Returns `None` if the decision ID is not found or if the service
    /// does not have an explanation for that decision.
    fn explain(&self, decision_id: AlphaId) -> Option<ExplainabilityRecord>;
}

/// Lifecycle trait for all Alpha services.
///
/// Every service in the Alpha runtime must implement this trait to
/// participate in the startup and shutdown sequence.
pub trait Service {
    /// Human-readable service name (e.g., "event-bus", "state-store").
    fn name(&self) -> &str;

    /// Initialize the service. Called during the startup sequence.
    ///
    /// Services should open database connections, register event bus
    /// subscriptions, and perform any one-time setup here.
    fn init(&mut self) -> Result<(), AlphaError>;

    /// Graceful shutdown. Called during the shutdown sequence.
    ///
    /// Services should flush pending writes, close connections,
    /// and release resources here.
    fn shutdown(&mut self) -> Result<(), AlphaError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verify that the traits can be implemented.
    struct MockService {
        initialized: bool,
    }

    impl Service for MockService {
        fn name(&self) -> &str {
            "mock-service"
        }

        fn init(&mut self) -> Result<(), AlphaError> {
            self.initialized = true;
            Ok(())
        }

        fn shutdown(&mut self) -> Result<(), AlphaError> {
            self.initialized = false;
            Ok(())
        }
    }

    impl Explainable for MockService {
        fn explain(&self, _decision_id: AlphaId) -> Option<ExplainabilityRecord> {
            None
        }
    }

    #[test]
    fn test_service_lifecycle() {
        let mut svc = MockService { initialized: false };
        assert_eq!(svc.name(), "mock-service");

        svc.init().unwrap();
        assert!(svc.initialized);

        svc.shutdown().unwrap();
        assert!(!svc.initialized);
    }

    #[test]
    fn test_explainable_returns_none_for_unknown() {
        let svc = MockService { initialized: false };
        let result = svc.explain(crate::types::new_id());
        assert!(result.is_none());
    }
}
