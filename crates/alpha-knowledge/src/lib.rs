//! # alpha-knowledge
//!
//! Knowledge entity persistence for Project Alpha.
//!
//! Sprint 2 provides simple CRUD operations for knowledge entities
//! (persons, concepts, places, events, skills) backed by SQLite.
//!
//! ## Sprint 2 Scope
//!
//! - Store, retrieve, update, delete entities
//! - List with type filter and pagination
//! - Count entities
//!
//! ## Explicitly Not Implemented (Sprint 3+)
//!
//! - Graph traversal / entity relationships
//! - `search_by_name()` (text search)
//! - Semantic search / embeddings
//! - Knowledge reasoning engine

pub mod store;
pub mod types;

pub use store::KnowledgeStore;
pub use types::KnowledgeEntity;

#[cfg(test)]
mod tests;
