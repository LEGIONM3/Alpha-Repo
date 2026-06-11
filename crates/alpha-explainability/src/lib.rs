//! # alpha-explainability
//!
//! Explainability record persistence for Project Alpha.
//!
//! Sprint 2 provides minimal storage for `ExplainabilityRecord` from
//! `alpha-common`. Records are stored in SQLite with full preservation
//! of reasoning chains, factors, alternatives, and evidence references.
//!
//! ## Sprint 2 Scope
//!
//! - `store()` — persist a record
//! - `get()` — retrieve by ID
//! - `count()` — total record count
//!
//! ## Explicitly Not Implemented (Sprint 3+)
//!
//! - `get_by_subject()` — query by decision subject
//! - `list()` — paginated listing
//! - Explanation generation engine
//! - Chain traversal / reasoning replay

pub mod store;

pub use store::ExplainabilityStore;

#[cfg(test)]
mod tests;
