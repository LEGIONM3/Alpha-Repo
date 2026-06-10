//! Goal Node schema (Canonical Schema 5).
//!
//! Stored in: `goals` SQLite DB.
//! The 5-level hierarchy (Mission -> Project -> Goal -> Task -> Action)
//! is encoded in the `goal_type` field and the `parent_id` tree.

use serde::{Deserialize, Serialize};
use crate::types::{AlphaId, Timestamp, JsonValue};

/// The level of the goal in the 5-tier hierarchy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GoalType {
    /// Life-scale. Persistent. Never "completed".
    Mission,
    /// Strategic, weeks to months.
    Project,
    /// Tactical, days to weeks.
    Goal,
    /// Operational, minutes to hours.
    Task,
    /// Atomic, seconds to minutes.
    Action,
}

/// Current status of a goal node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Pending,
    Active,
    Blocked,
    Completed,
    Failed,
    Cancelled,
}

/// Who created this goal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GoalCreator {
    User,
    Alpha,
}

/// Risk level assessment.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    #[default]
    Low,
    Medium,
    High,
    Critical,
}

/// A node in the goal hierarchy tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalNode {
    pub id: AlphaId,
    /// Parent node ID. None for root nodes (Missions).
    pub parent_id: Option<AlphaId>,
    pub title: String,
    pub description: String,
    /// Level in the hierarchy.
    pub goal_type: GoalType,
    pub status: GoalStatus,
    /// Priority: 1 (lowest) to 10 (highest).
    pub priority: u8,
    pub created_by: GoalCreator,
    /// Agent type name assigned to this goal, if any.
    pub assigned_agent: Option<String>,

    // Scheduling
    pub deadline: Option<Timestamp>,
    /// Cron expression for recurring goals.
    pub recurrence: Option<String>,
    pub estimated_effort_mins: Option<u32>,

    // Dependencies
    /// Goals that must complete before this one can start.
    #[serde(default)]
    pub depends_on: Vec<AlphaId>,
    /// Goals that this one blocks.
    #[serde(default)]
    pub blocks: Vec<AlphaId>,

    // Progress
    /// Completion progress: 0.0 to 1.0.
    pub progress: f32,

    // Risk
    pub risk_level: RiskLevel,
    /// Whether this goal requires user approval before execution.
    pub requires_approval: bool,

    // Results
    #[serde(default)]
    pub success_criteria: Vec<String>,
    pub outcome: Option<JsonValue>,

    // Lifecycle
    pub created_at: Timestamp,
    pub started_at: Option<Timestamp>,
    pub completed_at: Option<Timestamp>,
}

impl GoalNode {
    /// Create a new task-level GoalNode (the most common type).
    pub fn new_task(title: String, description: String, created_by: GoalCreator) -> Self {
        let now = crate::types::now();
        Self {
            id: crate::types::new_id(),
            parent_id: None,
            title,
            description,
            goal_type: GoalType::Task,
            status: GoalStatus::Pending,
            priority: 5,
            created_by,
            assigned_agent: None,
            deadline: None,
            recurrence: None,
            estimated_effort_mins: None,
            depends_on: Vec::new(),
            blocks: Vec::new(),
            progress: 0.0,
            risk_level: RiskLevel::Low,
            requires_approval: false,
            success_criteria: Vec::new(),
            outcome: None,
            created_at: now,
            started_at: None,
            completed_at: None,
        }
    }
}
