//! Canonical event bus topic constants.
//!
//! Convention:
//! - Dot-separated, lowercase, no trailing dot
//! - Wildcard: `*` matches a single segment (handled by Event Bus matcher)
//!
//! Sprint 1 registers ALL known topics even if no publisher/subscriber exists yet.
//! This serves as the canonical topic registry for the entire system.
//! Adding topics here is free — it's just string constants.

// ── System lifecycle ──

/// Published when Alpha starts up successfully.
pub const SYSTEM_STARTED: &str = "alpha.system.started";
/// Published just before Alpha shuts down.
pub const SYSTEM_SHUTDOWN: &str = "alpha.system.shutdown";
/// Periodic health check heartbeat.
pub const SYSTEM_HEALTH: &str = "alpha.system.health";

// ── Security ──

/// Published after every security evaluation (approved or denied).
pub const SECURITY_EVALUATED: &str = "alpha.security.evaluated";
/// Published when a security request is approved.
pub const SECURITY_APPROVAL: &str = "alpha.security.approval";
/// Published when a security request is denied.
pub const SECURITY_DENIAL: &str = "alpha.security.denial";

// ── ARIS (AI Resource Intelligence) ──

/// Published when a new AI resource is registered.
pub const ARIS_RESOURCE_DISCOVERED: &str = "alpha.aris.resource.discovered";
/// Published when a resource's status changes.
pub const ARIS_RESOURCE_STATUS: &str = "alpha.aris.resource.status";
/// Published when a resource's capability score is updated.
pub const ARIS_PERFORMANCE_UPDATED: &str = "alpha.aris.performance.updated";

// ── Model Router (Sprint 2, registered now) ──

/// Published when an inference request begins.
pub const MODEL_INFERENCE_STARTED: &str = "alpha.model.inference.started";
/// Published when an inference request completes.
pub const MODEL_INFERENCE_COMPLETED: &str = "alpha.model.inference.completed";

// ── Memory (Sprint 2, registered now) ──

/// Published when a memory is successfully stored.
pub const MEMORY_STORE_COMPLETED: &str = "alpha.memory.store.completed";
/// Published when a memory retrieval query completes.
pub const MEMORY_RETRIEVE_COMPLETED: &str = "alpha.memory.retrieve.completed";
/// Trigger event to start a memory consolidation cycle.
pub const MEMORY_CONSOLIDATE_TRIGGER: &str = "alpha.memory.consolidate.trigger";
/// Published when a consolidation cycle completes.
pub const MEMORY_CONSOLIDATE_COMPLETED: &str = "alpha.memory.consolidate.completed";

// ── Relationship Core (Sprint 2, registered now) ──

/// Published when a relationship core record is created or updated.
pub const RELATIONSHIP_UPDATED: &str = "alpha.relationship.updated";
/// Published when the user's trust level changes.
pub const RELATIONSHIP_TRUST_CHANGED: &str = "alpha.relationship.trust.changed";

// ── User Input (Sprint 3, registered now) ──

/// Published when user sends a text input.
pub const USER_INPUT_TEXT: &str = "alpha.user.input.text";
/// Published when user sends a voice input.
pub const USER_INPUT_VOICE: &str = "alpha.user.input.voice";
/// Published when user provides explicit feedback.
pub const USER_FEEDBACK: &str = "alpha.user.feedback";

// ── Agent (Sprint 3, registered now) ──

/// Published when a task is assigned to an agent.
pub const AGENT_TASK_ASSIGNED: &str = "alpha.agent.task.assigned";
/// Published when an agent completes a task.
pub const AGENT_TASK_RESULT: &str = "alpha.agent.task.result";
/// Published when a new agent instance is spawned.
pub const AGENT_SPAWNED: &str = "alpha.agent.lifecycle.spawned";

// ── Goals (Sprint 3, registered now) ──

/// Published when a new goal node is created.
pub const GOAL_CREATED: &str = "alpha.goal.created";
/// Published when a goal's progress is updated.
pub const GOAL_PROGRESS: &str = "alpha.goal.progress";
/// Published when a goal is completed.
pub const GOAL_COMPLETED: &str = "alpha.goal.completed";
/// Published when a goal fails.
pub const GOAL_FAILED: &str = "alpha.goal.failed";
/// Published when a goal is blocked.
pub const GOAL_BLOCKED: &str = "alpha.goal.blocked";

// ── Attention (Sprint 3, registered now) ──

/// Published when the focus stack is recomputed.
pub const ATTENTION_FOCUS_UPDATED: &str = "alpha.attention.focus.updated";
/// Published when an item's priority increases.
pub const ATTENTION_ITEM_ESCALATED: &str = "alpha.attention.item.escalated";

// ── Operational State (Sprint 4, registered now) ──

/// Published when the operational state transitions.
pub const STATE_TRANSITION: &str = "alpha.state.transition";

// ── Hardware (future, registered now) ──

/// Published when hardware capabilities change.
pub const HARDWARE_CAPABILITY_CHANGED: &str = "alpha.hardware.capability_changed";

/// Returns a slice of all registered topic constants for validation.
pub fn all_topics() -> &'static [&'static str] {
    &[
        SYSTEM_STARTED,
        SYSTEM_SHUTDOWN,
        SYSTEM_HEALTH,
        SECURITY_EVALUATED,
        SECURITY_APPROVAL,
        SECURITY_DENIAL,
        ARIS_RESOURCE_DISCOVERED,
        ARIS_RESOURCE_STATUS,
        ARIS_PERFORMANCE_UPDATED,
        MODEL_INFERENCE_STARTED,
        MODEL_INFERENCE_COMPLETED,
        MEMORY_STORE_COMPLETED,
        MEMORY_RETRIEVE_COMPLETED,
        MEMORY_CONSOLIDATE_TRIGGER,
        MEMORY_CONSOLIDATE_COMPLETED,
        RELATIONSHIP_UPDATED,
        RELATIONSHIP_TRUST_CHANGED,
        USER_INPUT_TEXT,
        USER_INPUT_VOICE,
        USER_FEEDBACK,
        AGENT_TASK_ASSIGNED,
        AGENT_TASK_RESULT,
        AGENT_SPAWNED,
        GOAL_CREATED,
        GOAL_PROGRESS,
        GOAL_COMPLETED,
        GOAL_FAILED,
        GOAL_BLOCKED,
        ATTENTION_FOCUS_UPDATED,
        ATTENTION_ITEM_ESCALATED,
        STATE_TRANSITION,
        HARDWARE_CAPABILITY_CHANGED,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_topics_are_dot_separated_lowercase() {
        for topic in all_topics() {
            // Must not be empty
            assert!(!topic.is_empty(), "Topic must not be empty");

            // Must not start or end with a dot
            assert!(
                !topic.starts_with('.'),
                "Topic '{}' must not start with a dot",
                topic
            );
            assert!(
                !topic.ends_with('.'),
                "Topic '{}' must not end with a dot",
                topic
            );

            // Must be lowercase ASCII + dots + underscores only
            for ch in topic.chars() {
                assert!(
                    ch.is_ascii_lowercase() || ch == '.' || ch == '_',
                    "Topic '{}' contains invalid character '{}'. Only lowercase ASCII, dots, and underscores allowed.",
                    topic,
                    ch
                );
            }

            // Must start with "alpha."
            assert!(
                topic.starts_with("alpha."),
                "Topic '{}' must start with 'alpha.'",
                topic
            );

            // Must have at least 3 segments (alpha.x.y)
            let segments: Vec<&str> = topic.split('.').collect();
            assert!(
                segments.len() >= 3,
                "Topic '{}' must have at least 3 dot-separated segments",
                topic
            );

            // No empty segments (no double dots)
            for segment in &segments {
                assert!(
                    !segment.is_empty(),
                    "Topic '{}' has an empty segment (double dot)",
                    topic
                );
            }
        }
    }

    #[test]
    fn test_no_duplicate_topics() {
        let topics = all_topics();
        for (i, topic) in topics.iter().enumerate() {
            for (j, other) in topics.iter().enumerate() {
                if i != j {
                    assert_ne!(
                        topic, other,
                        "Duplicate topic found: '{}'",
                        topic
                    );
                }
            }
        }
    }

    #[test]
    fn test_topic_count() {
        // Ensure we registered the expected number of topics
        assert_eq!(all_topics().len(), 32);
    }
}
