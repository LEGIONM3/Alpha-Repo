//! Relationship Core Record schema (Canonical Schema 4).
//!
//! Stored in: `relationship_core` SQLite DB + `vectors` Qdrant collection.
//! This is the protected substrate of the user-Alpha relationship.
//!
//! INVARIANTS:
//! - `protected` is ALWAYS `true`
//! - `decay_rate` is ALWAYS `0.0`
//! - `governance_state` is ALWAYS `"active"`
//! - `importance` is ALWAYS `>= 0.5`
//!
//! These invariants are enforced at construction time and must be
//! enforced at the storage layer.

use serde::{Deserialize, Serialize};
use crate::types::{AlphaId, Timestamp, JsonValue};

/// Category of relationship core data.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipCategory {
    /// How trust has evolved over time.
    TrustEvolution,
    /// Significant moments in the user-Alpha relationship.
    SharedHistory,
    /// How the user wants Alpha to communicate.
    CommunicationPref,
    /// Alpha's deep understanding of who the user is.
    UserIdentity,
    /// Alpha's understanding of its role for this specific user.
    AlphaPurpose,
}

/// How the relationship data was sourced.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipSource {
    /// User directly stated this.
    UserExplicit,
    /// Alpha inferred from user behavior.
    UserImplicit,
    /// Alpha observed this happen.
    AlphaObserved,
}

/// A protected relationship memory record.
///
/// Unlike regular memories, relationship core records:
/// - Never decay
/// - Cannot be archived or deprecated
/// - Cannot have their protection removed
/// - Have a minimum importance floor of 0.5
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipCoreRecord {
    pub id: AlphaId,
    pub category: RelationshipCategory,
    /// Natural language content.
    pub content: String,
    /// Vector embedding for semantic retrieval.
    #[serde(default)]
    pub embedding: Vec<f32>,
    /// Always >= 0.5. Enforced at construction.
    pub importance: f32,
    /// INVARIANT: Always true. Cannot be set to false.
    pub protected: bool,
    /// INVARIANT: Always 0.0. Cannot be changed.
    pub decay_rate: f32,
    /// INVARIANT: Always "active". Cannot transition.
    pub governance_state: String,
    /// How this data was sourced.
    pub source: RelationshipSource,
    /// Confidence score (0.0-1.0).
    pub confidence: f32,
    /// Whether the user has explicitly validated this record.
    pub confirmed_by_user: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    /// If this record corrects a previous one, the ID of the superseded record.
    pub supersedes: Option<AlphaId>,
    /// Links to episodic memories that serve as evidence.
    #[serde(default)]
    pub related_memories: Vec<AlphaId>,
    /// Extensible metadata.
    #[serde(default)]
    pub metadata: JsonValue,
}

impl RelationshipCoreRecord {
    /// Create a new RelationshipCoreRecord with all invariants enforced.
    ///
    /// - `protected` is set to `true`
    /// - `decay_rate` is set to `0.0`
    /// - `governance_state` is set to `"active"`
    /// - `importance` is floored at `0.5`
    pub fn new(
        category: RelationshipCategory,
        content: String,
        source: RelationshipSource,
        confidence: f32,
    ) -> Self {
        let now = crate::types::now();
        let confidence = confidence.clamp(0.0, 1.0);
        Self {
            id: crate::types::new_id(),
            category,
            content,
            embedding: Vec::new(),
            importance: 0.5_f32.max(confidence),
            protected: true,
            decay_rate: 0.0,
            governance_state: "active".to_string(),
            source,
            confidence,
            confirmed_by_user: false,
            created_at: now,
            updated_at: now,
            supersedes: None,
            related_memories: Vec::new(),
            metadata: JsonValue::Null,
        }
    }

    /// Validate that all invariants hold on an existing record.
    /// Returns an error if any invariant is violated.
    pub fn validate_invariants(&self) -> Result<(), crate::error::AlphaError> {
        if !self.protected {
            return Err(crate::error::AlphaError::Invariant(
                "RelationshipCoreRecord.protected must always be true".to_string(),
            ));
        }
        if self.decay_rate != 0.0 {
            return Err(crate::error::AlphaError::Invariant(
                "RelationshipCoreRecord.decay_rate must always be 0.0".to_string(),
            ));
        }
        if self.governance_state != "active" {
            return Err(crate::error::AlphaError::Invariant(
                "RelationshipCoreRecord.governance_state must always be \"active\"".to_string(),
            ));
        }
        if self.importance < 0.5 {
            return Err(crate::error::AlphaError::Invariant(
                "RelationshipCoreRecord.importance must be >= 0.5".to_string(),
            ));
        }
        Ok(())
    }
}
