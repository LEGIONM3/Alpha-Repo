//! Types for context assembly.

use alpha_common::types::AlphaId;

/// Configuration for context assembly.
#[derive(Debug, Clone)]
pub struct ContextConfig {
    /// Maximum total tokens for the context window.
    /// Default: 4096 (conservative for 8192 context models).
    pub max_tokens: usize,

    /// Approximate chars-per-token ratio for budget estimation.
    /// Default: 4 (English text average).
    pub chars_per_token: usize,

    /// Maximum number of memories to retrieve.
    /// Default: 10.
    pub max_memories: usize,

    /// Maximum number of relationship records to include.
    /// Default: 5.
    pub max_relationships: usize,

    /// Maximum number of knowledge entities to include.
    /// Default: 5.
    pub max_knowledge: usize,

    /// Minimum similarity threshold for memory retrieval.
    /// Default: 0.3.
    pub similarity_threshold: f32,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_tokens: 4096,
            chars_per_token: 4,
            max_memories: 10,
            max_relationships: 5,
            max_knowledge: 5,
            similarity_threshold: 0.3,
        }
    }
}

/// A labeled block within the assembled context.
#[derive(Debug, Clone)]
pub struct ContextBlock {
    /// Block label: "identity", "relationship", "memories", "knowledge", "history".
    pub label: String,
    /// The text content of this block.
    pub content: String,
    /// Estimated token count.
    pub estimated_tokens: usize,
}

/// A fully assembled context window ready for prompt construction.
#[derive(Debug, Clone)]
pub struct ContextWindow {
    /// The system prompt including identity and constitution.
    pub system_prompt: String,
    /// Ordered context blocks.
    pub blocks: Vec<ContextBlock>,
    /// Total estimated tokens.
    pub total_estimated_tokens: usize,
    /// IDs of memories included in context (for access_count updates).
    pub memory_ids_used: Vec<AlphaId>,
    /// IDs of relationship records included in context.
    pub relationship_ids_used: Vec<AlphaId>,
}

/// Role in a conversation turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnRole {
    /// The human user.
    User,
    /// Alpha's response.
    Alpha,
}

/// A single turn in a conversation.
///
/// Lightweight representation for context assembly.
/// The dialog manager (`alpha-dialog`) will define its own richer type.
#[derive(Debug, Clone)]
pub struct ConversationTurn {
    /// Who spoke.
    pub role: TurnRole,
    /// What was said.
    pub content: String,
}
