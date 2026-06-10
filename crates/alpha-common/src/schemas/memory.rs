//! Memory Record schema (Canonical Schema 3).
//!
//! Stored in: `memory` SQLite DB + `vectors` Qdrant collection.
//! Every memory operation depends on this schema.

use serde::{Deserialize, Serialize};
use crate::types::{AlphaId, Timestamp, JsonValue};

/// The type of memory being stored.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryType {
    /// Time-stamped records of specific events, conversations, and actions.
    Episodic,
    /// Facts, beliefs, preferences, and general knowledge.
    Semantic,
    /// Stored knowledge about how to perform tasks.
    Procedural,
}

/// Knowledge governance lifecycle state.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GovernanceState {
    /// Full visibility, highest retrieval priority.
    #[default]
    Active,
    /// Available on explicit queries, not proactively surfaced.
    Reference,
    /// Searchable only by deep/historical searches.
    Archived,
    /// Hidden from all normal queries.
    Deprecated,
}

/// A single memory record in Alpha's memory system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: AlphaId,
    /// Type of memory.
    pub memory_type: MemoryType,
    /// Natural language content.
    pub content: String,
    /// Vector embedding. Dimension is model-dependent (typically 768 or 1024).
    /// Empty vec means embedding has not been computed yet.
    #[serde(default)]
    pub embedding: Vec<f32>,
    /// Computed importance. 0.0 = trivial, 1.0 = critical.
    pub importance: f32,
    /// How often this memory has been retrieved.
    pub access_count: u32,
    /// When this memory was last accessed.
    pub last_accessed: Timestamp,
    /// When this memory was created.
    pub created_at: Timestamp,
    /// Origin: "conversation", "observation", "user_explicit", etc.
    pub source: String,
    /// Links to related memories.
    #[serde(default)]
    pub associations: Vec<AlphaId>,
    /// Categorization tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Confidence score (0.0-1.0).
    pub confidence: f32,
    /// Decay rate. 0.0 = never decays. Higher = faster decay.
    pub decay_rate: f32,
    /// Knowledge governance state.
    #[serde(default)]
    pub governance_state: GovernanceState,
    /// Extensible metadata.
    #[serde(default)]
    pub metadata: JsonValue,
}

impl MemoryRecord {
    /// Create a new MemoryRecord with sensible defaults.
    pub fn new(
        memory_type: MemoryType,
        content: String,
        source: String,
        importance: f32,
    ) -> Self {
        let now = crate::types::now();
        Self {
            id: crate::types::new_id(),
            memory_type,
            content,
            embedding: Vec::new(),
            importance: importance.clamp(0.0, 1.0),
            access_count: 0,
            last_accessed: now,
            created_at: now,
            source,
            associations: Vec::new(),
            tags: Vec::new(),
            confidence: 1.0,
            decay_rate: 0.01,
            governance_state: GovernanceState::Active,
            metadata: JsonValue::Null,
        }
    }
}
