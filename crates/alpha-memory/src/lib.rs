//! # alpha-memory
//!
//! Memory persistence and retrieval for Project Alpha.
//!
//! ## Features
//!
//! - **Store**: Persist `MemoryRecord` with packed embedding BLOBs
//! - **Retrieve**: Get by ID, list with filters and pagination
//! - **Search**: Semantic search by embedding similarity with composite scoring
//! - **Governance**: Lifecycle transitions (active → reference → archived → deprecated)
//! - **Validation**: Embedding dimension check (Sprint 2 Amendment §2)
//! - **Scoring**: Cosine similarity, recency decay, composite ranking

pub mod scoring;
pub mod search;
pub mod store;

pub use scoring::{composite_score, cosine_similarity, recency_factor};
pub use search::{ScoredMemory, SearchOptions};
pub use store::{
    MemoryStore, bytes_to_embedding, embedding_to_bytes, validate_embedding_dimension,
};
