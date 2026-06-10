//! Alpha Identity schema (Canonical Schema 1).
//!
//! Stored in: state_store -> table `alpha_identity`
//! Every service, device, and sync operation references `alpha_id`.

use serde::{Deserialize, Serialize};
use crate::types::{AlphaId, Timestamp};

/// Alpha's core identity. Generated once on first run. Never changes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AlphaIdentity {
    /// Immutable UUID. Generated once. Never changes.
    pub alpha_id: AlphaId,
    /// When Alpha was first created.
    pub created_at: Timestamp,
    /// SHA-256 hash of constitution.toml at last load.
    pub constitution_hash: String,
    /// Alpha's personality configuration.
    pub personality: Personality,
    /// Schema version (SemVer). Incremented on schema changes.
    pub schema_version: String,
}

/// Alpha's personality traits.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Personality {
    /// Alpha's own name.
    pub name: String,
    /// Default communication tone.
    pub tone: String,
    /// Default verbosity (0.0 = terse, 1.0 = verbose).
    pub verbosity: f32,
}

impl Default for Personality {
    fn default() -> Self {
        Self {
            name: "Alpha".to_string(),
            tone: "friendly_professional".to_string(),
            verbosity: 0.7,
        }
    }
}

impl AlphaIdentity {
    /// Create a new AlphaIdentity with defaults.
    pub fn new(constitution_hash: String) -> Self {
        Self {
            alpha_id: crate::types::new_id(),
            created_at: crate::types::now(),
            constitution_hash,
            personality: Personality::default(),
            schema_version: "1.0.0".to_string(),
        }
    }
}
