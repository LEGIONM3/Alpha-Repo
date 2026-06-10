//! Canonical data schemas for Project Alpha.
//!
//! These schemas are foundational contracts. Every future system depends on their shape.
//! Schema governance rules:
//! - No field removal (deprecate instead)
//! - No type changes (add new field instead)
//! - Additive only (new fields must have defaults)
//! - Version bump on any change
//! - Migration script for every change

pub mod identity;
pub mod memory;
pub mod relationship;
pub mod goal;
pub mod ai_resource;
pub mod explainability;

// Re-export all schema types at the schemas level.
pub use identity::*;
pub use memory::*;
pub use relationship::*;
pub use goal::*;
pub use ai_resource::*;
pub use explainability::*;
