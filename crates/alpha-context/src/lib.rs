//! # alpha-context
//!
//! Context assembly for Project Alpha.
//!
//! Builds a prompt context window from Alpha's knowledge systems:
//! - **Identity**: Alpha's name, personality, constitution summary
//! - **Relationship**: Communication preferences and user understanding
//! - **Memories**: Relevant episodic/semantic/procedural memories via embedding similarity
//! - **Knowledge**: Structured entities (people, concepts, places)
//! - **History**: Recent conversation turns
//!
//! ## Token Budget
//!
//! The assembler enforces a token budget with priority-based truncation:
//! 1. System prompt (never truncated)
//! 2. Relationship records (removed last)
//! 3. Memories (removed third)
//! 4. Conversation history (removed second)
//! 5. Knowledge entities (removed first)

pub mod assembler;
pub mod types;

pub use assembler::ContextAssembler;
pub use types::{ContextBlock, ContextConfig, ContextWindow, ConversationTurn, TurnRole};

#[cfg(test)]
mod tests;
