//! ARIS types for resource querying and result reporting.

use serde::{Deserialize, Serialize};

use alpha_common::schemas::ai_resource::{AIResource, ResourceStatus};
use alpha_common::types::AlphaId;

/// Constraints for querying resources.
///
/// Used by the Model Router to filter resources based on operational
/// requirements before selecting the best one for a task.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceConstraints {
    /// If true, only return resources with `privacy_level == Local`.
    pub local_only: bool,
    /// Maximum cost per request in USD.
    pub max_cost_usd: Option<f32>,
    /// Only return resources matching this status.
    pub status_filter: Option<ResourceStatus>,
    /// Minimum capability score for the queried domain.
    pub min_capability_score: Option<f32>,
}

/// A resource paired with its score for a specific task domain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredResource {
    /// The full resource record.
    pub resource: AIResource,
    /// Capability score for the queried domain (0.0–1.0).
    pub score: f32,
}

/// Result of an inference/task executed against a resource.
///
/// Sprint 1: stored in `result_log` for future learning.
/// Scores are NOT updated based on results in Sprint 1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    /// Which resource was used.
    pub resource_id: AlphaId,
    /// Task domain (e.g., "text_generation", "code_generation").
    pub task_domain: String,
    /// Whether the task succeeded.
    pub success: bool,
    /// How long the inference took.
    pub latency_ms: u64,
    /// Input token count (if applicable).
    pub tokens_in: Option<u32>,
    /// Output token count (if applicable).
    pub tokens_out: Option<u32>,
    /// User satisfaction rating (0.0–1.0, if available).
    pub user_satisfaction: Option<f32>,
}
