//! # alpha-common
//!
//! Shared types, schemas, configuration, and traits for Project Alpha.
//!
//! This crate is the **foundation** of the Alpha workspace. Every other crate
//! depends on it. It contains:
//!
//! - **Canonical Schemas**: The 7 foundational data structures that all future
//!   systems depend on (Identity, Event, Memory, Relationship, Goal, AIResource,
//!   Explainability).
//! - **Primitive Types**: `AlphaId`, `Timestamp`, `JsonValue`, and utility functions.
//! - **Error Types**: The unified `AlphaError` enum used across all crates.
//! - **Event Bus Topics**: Canonical topic string constants.
//! - **Traits**: `Service` and `Explainable` traits that services implement.
//! - **Configuration**: Structs for loading TOML config files.
//!
//! ## Design Rules
//!
//! - This crate has **no runtime dependencies** (no async runtime, no database).
//! - All schemas use `serde` for serialization/deserialization.
//! - Canonical schemas are **append-only**: no field removal, no type changes.
//! - New fields must always have defaults.

pub mod config;
pub mod error;
pub mod event;
pub mod schemas;
pub mod topics;
pub mod traits;
pub mod types;

// ── Convenience re-exports ──
// These allow consumers to write `use alpha_common::Event` instead of
// `use alpha_common::event::Event`.

pub use error::{AlphaError, AlphaResult};
pub use event::{Event, EventMetadata};
pub use types::{AlphaId, JsonValue, Timestamp, new_id, now};

// Re-export all schemas at the crate root for ergonomic access.
pub use schemas::identity::{AlphaIdentity, Personality};
pub use schemas::memory::{GovernanceState, MemoryRecord, MemoryType};
pub use schemas::relationship::{RelationshipCategory, RelationshipCoreRecord, RelationshipSource};
pub use schemas::goal::{GoalCreator, GoalNode, GoalStatus, GoalType, RiskLevel};
pub use schemas::ai_resource::{
    AIResource, AuthMethod, Capability, PrivacyLevel, ResourceStatus, ResourceType,
};
pub use schemas::explainability::{
    Alternative, ExplainabilityRecord, ExplanationType, Factor, FactorDirection,
};

// Re-export traits.
pub use traits::{Explainable, Service};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── Event Tests ──

    #[test]
    fn test_event_creation_defaults() {
        let event = Event::new(
            "alpha.system.started",
            "alpha-core",
            json!({"version": "0.1.0"}),
        );
        assert_eq!(event.event_type, "alpha.system.started");
        assert_eq!(event.source, "alpha-core");
        assert_eq!(event.priority, 5);
        assert_eq!(event.metadata.retry_count, 0);
        assert!(event.metadata.ttl_ms.is_none());
        assert_eq!(event.correlation_id, event.metadata.trace_id);
    }

    #[test]
    fn test_event_serialization_roundtrip() {
        let event = Event::new(
            "alpha.test.roundtrip",
            "test-suite",
            json!({"nested": {"key": "value"}, "array": [1, 2, 3]}),
        );
        let serialized = serde_json::to_string(&event).expect("serialize");
        let deserialized: Event = serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(event, deserialized);
    }

    // ── Identity Tests ──

    #[test]
    fn test_identity_creation() {
        let identity = AlphaIdentity::new("abc123hash".to_string());
        assert_eq!(identity.personality.name, "Alpha");
        assert_eq!(identity.schema_version, "1.0.0");
        assert_eq!(identity.constitution_hash, "abc123hash");
    }

    #[test]
    fn test_identity_serialization_roundtrip() {
        let identity = AlphaIdentity::new("testhash".to_string());
        let serialized = serde_json::to_string(&identity).expect("serialize");
        let deserialized: AlphaIdentity =
            serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(identity, deserialized);
    }

    // ── Memory Tests ──

    #[test]
    fn test_memory_creation() {
        let mem = MemoryRecord::new(
            MemoryType::Episodic,
            "User said hello".to_string(),
            "conversation".to_string(),
            0.5,
        );
        assert_eq!(mem.memory_type, MemoryType::Episodic);
        assert_eq!(mem.importance, 0.5);
        assert_eq!(mem.access_count, 0);
        assert_eq!(mem.governance_state, GovernanceState::Active);
        assert!(mem.embedding.is_empty());
    }

    #[test]
    fn test_memory_importance_clamped() {
        let mem = MemoryRecord::new(
            MemoryType::Semantic,
            "test".to_string(),
            "test".to_string(),
            1.5, // Over max
        );
        assert_eq!(mem.importance, 1.0);

        let mem = MemoryRecord::new(
            MemoryType::Semantic,
            "test".to_string(),
            "test".to_string(),
            -0.5, // Under min
        );
        assert_eq!(mem.importance, 0.0);
    }

    #[test]
    fn test_memory_serialization_roundtrip() {
        let mem = MemoryRecord::new(
            MemoryType::Procedural,
            "How to deploy".to_string(),
            "observation".to_string(),
            0.8,
        );
        let serialized = serde_json::to_string(&mem).expect("serialize");
        let deserialized: MemoryRecord =
            serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(mem.id, deserialized.id);
        assert_eq!(mem.memory_type, deserialized.memory_type);
        assert_eq!(mem.content, deserialized.content);
    }

    // ── Relationship Core Tests ──

    #[test]
    fn test_relationship_invariants_enforced_at_construction() {
        let record = RelationshipCoreRecord::new(
            RelationshipCategory::CommunicationPref,
            "User prefers concise answers".to_string(),
            RelationshipSource::UserExplicit,
            0.9,
        );

        assert!(record.protected, "protected must always be true");
        assert_eq!(record.decay_rate, 0.0, "decay_rate must always be 0.0");
        assert_eq!(
            record.governance_state, "active",
            "governance_state must always be 'active'"
        );
        assert!(
            record.importance >= 0.5,
            "importance must be >= 0.5, got {}",
            record.importance
        );
    }

    #[test]
    fn test_relationship_importance_floor() {
        let record = RelationshipCoreRecord::new(
            RelationshipCategory::UserIdentity,
            "test".to_string(),
            RelationshipSource::AlphaObserved,
            0.2, // Low confidence → importance should be floored at 0.5
        );
        assert_eq!(record.importance, 0.5);
        assert_eq!(record.confidence, 0.2);
    }

    #[test]
    fn test_relationship_importance_uses_confidence_when_higher() {
        let record = RelationshipCoreRecord::new(
            RelationshipCategory::TrustEvolution,
            "test".to_string(),
            RelationshipSource::UserExplicit,
            0.9, // High confidence → importance should be 0.9
        );
        assert_eq!(record.importance, 0.9);
    }

    #[test]
    fn test_relationship_validate_invariants_success() {
        let record = RelationshipCoreRecord::new(
            RelationshipCategory::SharedHistory,
            "test".to_string(),
            RelationshipSource::AlphaObserved,
            0.7,
        );
        assert!(record.validate_invariants().is_ok());
    }

    #[test]
    fn test_relationship_validate_invariants_catches_violations() {
        let mut record = RelationshipCoreRecord::new(
            RelationshipCategory::AlphaPurpose,
            "test".to_string(),
            RelationshipSource::UserExplicit,
            0.8,
        );

        // Tamper with invariants (this should never happen in production,
        // but validate_invariants must catch it if it does).
        record.protected = false;
        assert!(record.validate_invariants().is_err());

        record.protected = true;
        record.decay_rate = 0.1;
        assert!(record.validate_invariants().is_err());

        record.decay_rate = 0.0;
        record.governance_state = "archived".to_string();
        assert!(record.validate_invariants().is_err());

        record.governance_state = "active".to_string();
        record.importance = 0.3;
        assert!(record.validate_invariants().is_err());
    }

    #[test]
    fn test_relationship_serialization_roundtrip() {
        let record = RelationshipCoreRecord::new(
            RelationshipCategory::CommunicationPref,
            "Call me Ryan".to_string(),
            RelationshipSource::UserExplicit,
            1.0,
        );
        let serialized = serde_json::to_string(&record).expect("serialize");
        let deserialized: RelationshipCoreRecord =
            serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(record.id, deserialized.id);
        assert!(deserialized.protected);
        assert_eq!(deserialized.decay_rate, 0.0);
        assert_eq!(deserialized.governance_state, "active");
    }

    // ── Goal Tests ──

    #[test]
    fn test_goal_creation() {
        let goal = GoalNode::new_task(
            "Research event bus crates".to_string(),
            "Find suitable Rust crates for pub/sub".to_string(),
            GoalCreator::Alpha,
        );
        assert_eq!(goal.goal_type, GoalType::Task);
        assert_eq!(goal.status, GoalStatus::Pending);
        assert_eq!(goal.priority, 5);
        assert_eq!(goal.progress, 0.0);
        assert_eq!(goal.risk_level, RiskLevel::Low);
        assert!(!goal.requires_approval);
        assert!(goal.parent_id.is_none());
    }

    #[test]
    fn test_goal_serialization_roundtrip() {
        let goal = GoalNode::new_task(
            "Test goal".to_string(),
            "Description".to_string(),
            GoalCreator::User,
        );
        let serialized = serde_json::to_string(&goal).expect("serialize");
        let deserialized: GoalNode =
            serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(goal.id, deserialized.id);
        assert_eq!(goal.title, deserialized.title);
        assert_eq!(goal.goal_type, deserialized.goal_type);
    }

    // ── AIResource Tests ──

    #[test]
    fn test_ai_resource_creation() {
        let resource = AIResource::new_ollama_model(
            "ollama/llama3.1:8b".to_string(),
            "http://localhost:11434".to_string(),
            Some(8192),
            vec![Capability {
                domain: "text_generation".to_string(),
                score: 0.7,
                sample_count: 0,
            }],
        );
        assert_eq!(resource.resource_type, ResourceType::LocalModel);
        assert_eq!(resource.provider, "ollama");
        assert_eq!(resource.privacy_level, PrivacyLevel::Local);
        assert!(!resource.requires_network);
        assert_eq!(resource.capabilities.len(), 1);
    }

    #[test]
    fn test_ai_resource_serialization_roundtrip() {
        let resource = AIResource::new_ollama_model(
            "test-model".to_string(),
            "http://localhost:11434".to_string(),
            None,
            vec![],
        );
        let serialized = serde_json::to_string(&resource).expect("serialize");
        let deserialized: AIResource =
            serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(resource.id, deserialized.id);
        assert_eq!(resource.name, deserialized.name);
    }

    // ── Explainability Tests ──

    #[test]
    fn test_explainability_creation() {
        let trace = new_id();
        let subject = new_id();
        let record = ExplainabilityRecord::new(
            ExplanationType::Model,
            subject,
            "Selected ollama/llama3.1:8b for text generation".to_string(),
            trace,
        );
        assert_eq!(record.explanation_type, ExplanationType::Model);
        assert_eq!(record.subject_id, subject);
        assert_eq!(record.trace_id, trace);
        assert_eq!(record.confidence, 1.0);
        assert!(record.reasoning.is_empty());
        assert!(record.factors.is_empty());
        assert!(record.alternatives.is_empty());
    }

    #[test]
    fn test_explainability_serialization_roundtrip() {
        let record = ExplainabilityRecord::new(
            ExplanationType::Action,
            new_id(),
            "Opened file".to_string(),
            new_id(),
        );
        let serialized = serde_json::to_string(&record).expect("serialize");
        let deserialized: ExplainabilityRecord =
            serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(record.id, deserialized.id);
        assert_eq!(record.summary, deserialized.summary);
    }

    // ── Topic Tests ──

    #[test]
    fn test_topics_all_valid_format() {
        for topic in topics::all_topics() {
            assert!(!topic.is_empty());
            assert!(!topic.starts_with('.'));
            assert!(!topic.ends_with('.'));
            assert!(topic.starts_with("alpha."));
            for ch in topic.chars() {
                assert!(
                    ch.is_ascii_lowercase() || ch == '.' || ch == '_',
                    "Invalid char '{}' in topic '{}'",
                    ch,
                    topic
                );
            }
        }
    }
}
