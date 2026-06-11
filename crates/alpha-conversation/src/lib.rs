//! # alpha-conversation
//!
//! End-to-end conversation pipeline for Project Alpha.
//!
//! Orchestrates the full message lifecycle:
//! 1. Session management (via `alpha-dialog`)
//! 2. Query embedding generation (via `ModelProvider`)
//! 3. Context assembly (via `alpha-context`)
//! 4. LLM inference (via `ModelProvider`)
//! 5. Memory persistence (via `MemoryWriter`)
//! 6. Event publishing (via `alpha-event-bus`)
//!
//! ## Graceful Degradation
//!
//! If embedding generation fails, the conversation continues
//! without memory retrieval context — the user is never blocked.

pub mod memory_writer;
pub mod service;
pub mod types;

pub use memory_writer::MemoryWriter;
pub use service::ConversationService;
pub use types::{
    ConversationRequest, ConversationResponse, ModelProvider, SessionHistory, SessionSummary,
    StreamEvent, TurnSummary,
};

#[cfg(test)]
mod tests;
