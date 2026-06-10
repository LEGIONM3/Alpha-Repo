//! Identity management for Project Alpha.
//!
//! Handles first-run identity creation and subsequent-run identity loading.
//! The identity is persisted in the StateStore under namespace "alpha", key "identity".

use alpha_common::config::{AlphaConfig, hash_file};
use alpha_common::error::AlphaError;
use alpha_common::schemas::identity::{AlphaIdentity, Personality};
use alpha_state::StateStore;

use std::path::Path;
use tracing::{info, warn};

/// Namespace and key for identity storage.
const IDENTITY_NAMESPACE: &str = "alpha";
const IDENTITY_KEY: &str = "identity";

/// Load or create the Alpha identity.
///
/// - On first run: creates a new identity, stores it in the StateStore.
/// - On subsequent runs: loads existing identity, verifies constitution hash.
pub fn load_or_create_identity(
    state: &StateStore,
    config: &AlphaConfig,
    constitution_path: &Path,
) -> Result<AlphaIdentity, AlphaError> {
    let existing = state.get(IDENTITY_NAMESPACE, IDENTITY_KEY)?;

    match existing {
        Some(json_str) => load_existing_identity(state, &json_str, constitution_path),
        None => create_new_identity(state, config, constitution_path),
    }
}

/// Create a brand new identity on first run.
fn create_new_identity(
    state: &StateStore,
    config: &AlphaConfig,
    constitution_path: &Path,
) -> Result<AlphaIdentity, AlphaError> {
    let constitution_hash = hash_file(constitution_path)?;

    let personality = Personality {
        name: config.alpha.identity.name.clone(),
        tone: config.alpha.identity.tone.clone(),
        verbosity: config.alpha.identity.verbosity,
    };

    let mut identity = AlphaIdentity::new(constitution_hash);
    identity.personality = personality;

    // Persist to StateStore.
    let json_str = serde_json::to_string(&identity)?;
    state.set(IDENTITY_NAMESPACE, IDENTITY_KEY, &json_str)?;

    info!(
        alpha_id = %identity.alpha_id,
        name = %identity.personality.name,
        "Welcome. Alpha identity created."
    );

    Ok(identity)
}

/// Load an existing identity and verify the constitution hash.
fn load_existing_identity(
    state: &StateStore,
    json_str: &str,
    constitution_path: &Path,
) -> Result<AlphaIdentity, AlphaError> {
    let mut identity: AlphaIdentity = serde_json::from_str(json_str).map_err(|e| {
        AlphaError::Config(format!("Failed to deserialize stored identity: {}", e))
    })?;

    info!(
        alpha_id = %identity.alpha_id,
        name = %identity.personality.name,
        "Alpha identity loaded."
    );

    // Verify constitution hash.
    match hash_file(constitution_path) {
        Ok(current_hash) => {
            if current_hash != identity.constitution_hash {
                warn!(
                    stored_hash = %identity.constitution_hash,
                    current_hash = %current_hash,
                    "Constitution has changed since last run. Updating stored hash."
                );
                identity.constitution_hash = current_hash;

                // Update stored identity with new hash.
                let json_str = serde_json::to_string(&identity)?;
                state.set(IDENTITY_NAMESPACE, IDENTITY_KEY, &json_str)?;
            }
        }
        Err(e) => {
            warn!(
                error = %e,
                "Could not read constitution file for hash verification. Continuing with stored hash."
            );
        }
    }

    Ok(identity)
}
