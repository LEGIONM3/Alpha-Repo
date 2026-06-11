//! # alpha-relationship
//!
//! Protected relationship core persistence for Project Alpha.
//!
//! The relationship core stores the fundamental substrate of the
//! user-Alpha relationship — trust evolution, shared history,
//! communication preferences, user identity, and Alpha's purpose.
//!
//! ## Invariants (always enforced)
//!
//! - `protected = true` — cannot be removed
//! - `decay_rate = 0.0` — never decays
//! - `governance_state = "active"` — cannot transition
//! - `importance >= 0.5` — minimum importance floor
//!
//! ## Two-Layer Enforcement
//!
//! 1. **Rust**: `RelationshipCoreRecord::validate_invariants()` before insert
//! 2. **SQL**: `CHECK` constraints on the `relationship_core` table

pub mod store;

pub use store::{RelationshipStore, bytes_to_embedding, embedding_to_bytes};
