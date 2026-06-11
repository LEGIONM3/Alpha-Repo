//! Knowledge entity types for Project Alpha.
//!
//! Sprint 2: simple entity persistence only.
//! No graph reasoning, no relation traversal, no semantic search.

use alpha_common::{AlphaId, JsonValue, Timestamp};
use serde::{Deserialize, Serialize};

/// A knowledge entity representing a person, concept, place, event, or skill.
///
/// Sprint 2 stores flat entities with extensible properties.
/// Graph relationships and reasoning are deferred to Sprint 3+.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeEntity {
    /// Unique identifier.
    pub id: AlphaId,
    /// Entity category (e.g., "person", "concept", "place", "event", "skill").
    pub entity_type: String,
    /// Human-readable name.
    pub name: String,
    /// Free-text description.
    pub description: String,
    /// Extensible key-value properties (JSON object).
    pub properties: JsonValue,
    /// How this entity was discovered (e.g., "conversation", "user_explicit").
    pub source: String,
    /// Confidence in this entity's accuracy (0.0–1.0).
    pub confidence: f32,
    /// When this entity was created.
    pub created_at: Timestamp,
    /// When this entity was last modified.
    pub updated_at: Timestamp,
    /// Extensible metadata.
    pub metadata: JsonValue,
}

impl KnowledgeEntity {
    /// Create a new entity with sensible defaults.
    pub fn new(
        entity_type: impl Into<String>,
        name: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        let now = alpha_common::now();
        Self {
            id: alpha_common::new_id(),
            entity_type: entity_type.into(),
            name: name.into(),
            description: String::new(),
            properties: JsonValue::Object(serde_json::Map::new()),
            source: source.into(),
            confidence: 1.0,
            created_at: now,
            updated_at: now,
            metadata: JsonValue::Object(serde_json::Map::new()),
        }
    }

    /// Builder: set description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Builder: set confidence.
    #[must_use]
    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }

    /// Builder: set properties.
    #[must_use]
    pub fn with_properties(mut self, properties: JsonValue) -> Self {
        self.properties = properties;
        self
    }
}
