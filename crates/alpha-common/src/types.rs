//! Primitive type aliases and utility functions for Project Alpha.
//!
//! These are the foundational types used throughout the entire codebase.
//! Every crate imports these rather than depending on uuid/chrono directly.

use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Globally unique identifier. Alias for UUID v4.
/// Used as the primary key for all records across Alpha.
pub type AlphaId = Uuid;

/// UTC timestamp. All timestamps in Alpha are UTC.
/// No local time is ever stored — conversion happens at the presentation layer.
pub type Timestamp = DateTime<Utc>;

/// JSON blob for extensible metadata fields.
/// Used in schemas where the shape of additional data is not predetermined.
pub type JsonValue = serde_json::Value;

/// Generate a new random AlphaId (UUID v4).
#[inline]
pub fn new_id() -> AlphaId {
    Uuid::new_v4()
}

/// Get the current UTC timestamp.
#[inline]
pub fn now() -> Timestamp {
    Utc::now()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_id_is_unique() {
        let id1 = new_id();
        let id2 = new_id();
        assert_ne!(id1, id2, "Two generated IDs should be unique");
    }

    #[test]
    fn test_now_returns_utc() {
        let ts = now();
        // Verify it's a valid timestamp by checking it's not zero
        assert!(ts.timestamp() > 0);
    }
}
