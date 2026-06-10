//! Explainability Record schema (Canonical Schema 7).
//!
//! Stored in: `explainability` SQLite DB.
//! Every service implementing the `Explainable` trait produces records in this shape.

use serde::{Deserialize, Serialize};
use crate::types::{AlphaId, Timestamp};

/// The type of decision being explained.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExplanationType {
    /// Explanation for an action taken.
    Action,
    /// Explanation for a model selection.
    Model,
    /// Explanation for a task prioritization.
    Task,
    /// Explanation for a recommendation made.
    Recommendation,
}

/// Whether a factor supported or opposed the decision.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FactorDirection {
    For,
    Against,
}

/// A quantified factor that influenced a decision.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Factor {
    pub name: String,
    pub value: f32,
    pub weight: f32,
    pub direction: FactorDirection,
}

/// An alternative that was considered but not chosen.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Alternative {
    pub option: String,
    pub score: f32,
    pub rejection_reason: String,
}

/// A structured explanation record for a decision Alpha made.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplainabilityRecord {
    pub id: AlphaId,
    pub explanation_type: ExplanationType,
    /// The ID of the decision being explained.
    pub subject_id: AlphaId,
    /// One-sentence summary.
    pub summary: String,
    /// Step-by-step reasoning.
    #[serde(default)]
    pub reasoning: Vec<String>,
    /// Quantified decision factors.
    #[serde(default)]
    pub factors: Vec<Factor>,
    /// Alternatives considered.
    #[serde(default)]
    pub alternatives: Vec<Alternative>,
    /// Links to supporting records (memories, knowledge, etc.).
    #[serde(default)]
    pub evidence: Vec<AlphaId>,
    /// Confidence in the decision (0.0-1.0).
    pub confidence: f32,
    /// Links to telemetry trace.
    pub trace_id: AlphaId,
    pub timestamp: Timestamp,
}

impl ExplainabilityRecord {
    /// Create a new ExplainabilityRecord.
    pub fn new(
        explanation_type: ExplanationType,
        subject_id: AlphaId,
        summary: String,
        trace_id: AlphaId,
    ) -> Self {
        Self {
            id: crate::types::new_id(),
            explanation_type,
            subject_id,
            summary,
            reasoning: Vec::new(),
            factors: Vec::new(),
            alternatives: Vec::new(),
            evidence: Vec::new(),
            confidence: 1.0,
            trace_id,
            timestamp: crate::types::now(),
        }
    }
}
