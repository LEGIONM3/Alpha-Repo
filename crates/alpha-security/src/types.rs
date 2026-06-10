//! Security types for Project Alpha.
//!
//! These are the data structures used by the Security Gate to evaluate
//! action requests and record decisions.

use serde::{Deserialize, Serialize};

use alpha_common::schemas::goal::RiskLevel;
use alpha_common::types::{AlphaId, JsonValue, Timestamp};

/// A request from an agent or service to perform an action.
///
/// Every operation that could affect the system, user data, or external
/// resources must be submitted as an `ActionRequest` for evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRequest {
    /// Unique request ID.
    pub id: AlphaId,
    /// Type of action, e.g., `"file.write"`, `"shell.execute"`, `"api.call"`.
    pub action_type: String,
    /// Which agent is requesting the action.
    pub agent: String,
    /// What the action targets (file path, URL, etc.).
    pub target: String,
    /// Agent's self-assessed risk level.
    pub risk_level: RiskLevel,
    /// Action-specific parameters.
    pub parameters: JsonValue,
    /// When the request was created.
    pub timestamp: Timestamp,
}

impl ActionRequest {
    /// Create a new ActionRequest with sensible defaults.
    pub fn new(
        action_type: impl Into<String>,
        agent: impl Into<String>,
        target: impl Into<String>,
        risk_level: RiskLevel,
    ) -> Self {
        Self {
            id: alpha_common::types::new_id(),
            action_type: action_type.into(),
            agent: agent.into(),
            target: target.into(),
            risk_level,
            parameters: JsonValue::Null,
            timestamp: alpha_common::types::now(),
        }
    }

    /// Set action-specific parameters.
    pub fn with_parameters(mut self, params: JsonValue) -> Self {
        self.parameters = params;
        self
    }
}

/// The Security Gate's decision on an action request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum SecurityDecision {
    /// The action is approved and may proceed.
    Approved {
        id: AlphaId,
        reason: String,
    },
    /// The action is denied and must NOT proceed.
    Denied {
        id: AlphaId,
        reason: String,
    },
    /// The action requires explicit user approval before proceeding.
    RequiresApproval {
        id: AlphaId,
        reason: String,
    },
}

impl SecurityDecision {
    /// Get the decision ID.
    pub fn id(&self) -> &AlphaId {
        match self {
            Self::Approved { id, .. } => id,
            Self::Denied { id, .. } => id,
            Self::RequiresApproval { id, .. } => id,
        }
    }

    /// Get the reason string.
    pub fn reason(&self) -> &str {
        match self {
            Self::Approved { reason, .. } => reason,
            Self::Denied { reason, .. } => reason,
            Self::RequiresApproval { reason, .. } => reason,
        }
    }

    /// Get a short label for the decision type.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Approved { .. } => "approved",
            Self::Denied { .. } => "denied",
            Self::RequiresApproval { .. } => "requires_approval",
        }
    }

    /// Is this an approval?
    pub fn is_approved(&self) -> bool {
        matches!(self, Self::Approved { .. })
    }
}

/// An audit log entry recording a security evaluation.
///
/// Every call to `SecurityGate::evaluate()` produces one of these,
/// persisted in the `audit_log` SQLite table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Unique audit entry ID.
    pub id: AlphaId,
    /// The original action request.
    pub request: ActionRequest,
    /// The security decision made.
    pub decision: SecurityDecision,
    /// When the evaluation occurred.
    pub timestamp: Timestamp,
}
