//! Unified error type for Project Alpha.
//!
//! All crates in the Alpha workspace use `AlphaError` as their error type.
//! This ensures consistent error handling across the entire system.

use thiserror::Error;

/// The unified error type for all Alpha operations.
#[derive(Debug, Error)]
pub enum AlphaError {
    /// Configuration file could not be loaded or parsed.
    #[error("Configuration error: {0}")]
    Config(String),

    /// Database operation failed.
    #[error("Database error: {0}")]
    Database(String),

    /// Event bus operation failed.
    #[error("Event bus error: {0}")]
    EventBus(String),

    /// Serialization or deserialization failed.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// File or network I/O failed.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// A requested entity was not found.
    #[error("Not found: {entity} with id {id}")]
    NotFound {
        /// The type of entity (e.g., "memory", "goal", "resource").
        entity: String,
        /// The ID that was looked up.
        id: String,
    },

    /// A schema or business invariant was violated.
    #[error("Invariant violation: {0}")]
    Invariant(String),

    /// A security policy violation or enforcement error.
    #[error("Security: {0}")]
    Security(String),

    /// TOML parsing error.
    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),

    /// Catch-all for errors that don't fit other categories.
    #[error("{0}")]
    Other(String),
}

/// Convenience type alias for Results using AlphaError.
pub type AlphaResult<T> = Result<T, AlphaError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = AlphaError::Config("missing field".to_string());
        assert_eq!(err.to_string(), "Configuration error: missing field");
    }

    #[test]
    fn test_not_found_display() {
        let err = AlphaError::NotFound {
            entity: "memory".to_string(),
            id: "abc-123".to_string(),
        };
        assert_eq!(err.to_string(), "Not found: memory with id abc-123");
    }

    #[test]
    fn test_invariant_display() {
        let err = AlphaError::Invariant("decay_rate must be 0.0".to_string());
        assert_eq!(
            err.to_string(),
            "Invariant violation: decay_rate must be 0.0"
        );
    }
}
