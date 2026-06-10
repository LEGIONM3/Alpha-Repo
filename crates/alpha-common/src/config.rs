//! Configuration loading for Project Alpha.
//!
//! Handles loading and parsing of all TOML configuration files:
//! - `alpha.toml` — main application configuration
//! - `constitution.toml` — Alpha Constitution (immutable at runtime)
//! - `models.toml` — AI resource definitions
//! - `permissions.toml` — Security Gate rules

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;

use crate::error::AlphaError;

// ═══════════════════════════════════════════════════════════════
// alpha.toml
// ═══════════════════════════════════════════════════════════════

/// Top-level application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlphaConfig {
    pub alpha: AlphaSection,
}

/// The `[alpha]` section of alpha.toml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlphaSection {
    /// Data directory for all SQLite databases and runtime state.
    #[serde(default = "default_data_dir")]
    pub data_dir: String,
    /// Logging configuration.
    #[serde(default)]
    pub logging: LoggingConfig,
    /// Event bus configuration.
    #[serde(default)]
    pub event_bus: EventBusConfig,
    /// Identity defaults (used on first run only).
    #[serde(default)]
    pub identity: IdentityDefaults,
}

/// Logging configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level: "trace", "debug", "info", "warn", "error".
    #[serde(default = "default_log_level")]
    pub level: String,
    /// Output format: "pretty" or "json".
    #[serde(default = "default_log_format")]
    pub format: String,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            format: default_log_format(),
        }
    }
}

/// Event bus configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventBusConfig {
    /// Purge events older than this many days on startup.
    #[serde(default = "default_purge_days")]
    pub purge_after_days: u32,
}

impl Default for EventBusConfig {
    fn default() -> Self {
        Self {
            purge_after_days: default_purge_days(),
        }
    }
}

/// Default identity values used on first run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityDefaults {
    #[serde(default = "default_name")]
    pub name: String,
    #[serde(default = "default_tone")]
    pub tone: String,
    #[serde(default = "default_verbosity")]
    pub verbosity: f32,
}

impl Default for IdentityDefaults {
    fn default() -> Self {
        Self {
            name: default_name(),
            tone: default_tone(),
            verbosity: default_verbosity(),
        }
    }
}

fn default_data_dir() -> String { "data".to_string() }
fn default_log_level() -> String { "info".to_string() }
fn default_log_format() -> String { "pretty".to_string() }
fn default_purge_days() -> u32 { 30 }
fn default_name() -> String { "Alpha".to_string() }
fn default_tone() -> String { "friendly_professional".to_string() }
fn default_verbosity() -> f32 { 0.7 }

// ═══════════════════════════════════════════════════════════════
// constitution.toml
// ═══════════════════════════════════════════════════════════════

/// The Alpha Constitution — immutable at runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstitutionConfig {
    pub purpose: PurposeConfig,
    pub principles: PrinciplesConfig,
    pub autonomy: AutonomyConfig,
    pub security: SecurityPrinciplesConfig,
    pub self_improvement: SelfImprovementConfig,
}

/// Purpose section of the constitution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PurposeConfig {
    pub primary: String,
    pub secondary: String,
    pub non_purpose: String,
}

/// Core principles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrinciplesConfig {
    pub user_sovereignty: bool,
    pub transparency: bool,
    pub privacy_first: bool,
    pub do_no_harm: bool,
    pub proportional_autonomy: bool,
    pub honest_capability: bool,
    pub persistent_growth: bool,
    pub graceful_degradation: bool,
}

/// Autonomy configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutonomyConfig {
    pub default_trust_level: String,
}

/// Security principles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityPrinciplesConfig {
    pub fail_secure: bool,
    pub least_privilege: bool,
    pub audit_everything: bool,
}

/// Self-improvement rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfImprovementConfig {
    pub requires_user_approval: bool,
    pub sandbox_mandatory: bool,
    pub regression_detection_mandatory: bool,
    pub constitution_immutable: bool,
}

// ═══════════════════════════════════════════════════════════════
// models.toml
// ═══════════════════════════════════════════════════════════════

/// AI resource/model configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsConfig {
    #[serde(default)]
    pub defaults: ModelsDefaults,
    #[serde(default, rename = "resources")]
    pub resources: Vec<ResourceConfig>,
}

/// Default model selections.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsDefaults {
    #[serde(default = "default_embedding_model")]
    pub embedding_model: String,
}

impl Default for ModelsDefaults {
    fn default() -> Self {
        Self {
            embedding_model: default_embedding_model(),
        }
    }
}

fn default_embedding_model() -> String { "nomic-embed-text".to_string() }

/// A single AI resource definition from config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceConfig {
    pub name: String,
    pub provider: String,
    pub resource_type: String,
    pub endpoint: String,
    #[serde(default = "default_auth")]
    pub auth_method: String,
    pub context_window: Option<u32>,
    #[serde(default)]
    pub requires_network: bool,
    #[serde(default = "default_privacy")]
    pub privacy_level: String,
    /// Capability scores by domain name.
    #[serde(default)]
    pub capabilities: HashMap<String, f32>,
}

fn default_auth() -> String { "none".to_string() }
fn default_privacy() -> String { "local".to_string() }

// ═══════════════════════════════════════════════════════════════
// permissions.toml
// ═══════════════════════════════════════════════════════════════

/// Security permissions configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionsConfig {
    pub default: PermissionsDefault,
}

/// Default permission mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionsDefault {
    /// "allow_all", "deny_all", or "policy_based".
    pub mode: String,
}

// ═══════════════════════════════════════════════════════════════
// Loading Functions
// ═══════════════════════════════════════════════════════════════

/// Load and parse a TOML configuration file.
pub fn load_config<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, AlphaError> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        AlphaError::Config(format!("Failed to read config file '{}': {}", path.display(), e))
    })?;
    let config: T = toml::from_str(&content)?;
    Ok(config)
}

/// Compute the SHA-256 hash of a file's contents.
///
/// Used to create the constitution hash for integrity verification.
pub fn hash_file(path: &Path) -> Result<String, AlphaError> {
    let content = std::fs::read(path).map_err(|e| {
        AlphaError::Config(format!(
            "Failed to read file for hashing '{}': {}",
            path.display(),
            e
        ))
    })?;
    let mut hasher = Sha256::new();
    hasher.update(&content);
    let result = hasher.finalize();
    Ok(format!("{:x}", result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_load_alpha_config() {
        let toml_content = r#"
[alpha]
data_dir = "test_data"

[alpha.logging]
level = "debug"
format = "json"

[alpha.event_bus]
purge_after_days = 7

[alpha.identity]
name = "TestAlpha"
tone = "casual"
verbosity = 0.5
"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("alpha.toml");
        std::fs::write(&path, toml_content).unwrap();

        let config: AlphaConfig = load_config(&path).unwrap();
        assert_eq!(config.alpha.data_dir, "test_data");
        assert_eq!(config.alpha.logging.level, "debug");
        assert_eq!(config.alpha.logging.format, "json");
        assert_eq!(config.alpha.event_bus.purge_after_days, 7);
        assert_eq!(config.alpha.identity.name, "TestAlpha");
        assert_eq!(config.alpha.identity.tone, "casual");
        assert_eq!(config.alpha.identity.verbosity, 0.5);
    }

    #[test]
    fn test_load_constitution_config() {
        let toml_content = r#"
[purpose]
primary = "Serve the user"
secondary = "Improve capabilities"
non_purpose = "Not surveillance"

[principles]
user_sovereignty = true
transparency = true
privacy_first = true
do_no_harm = true
proportional_autonomy = true
honest_capability = true
persistent_growth = true
graceful_degradation = true

[autonomy]
default_trust_level = "standard"

[security]
fail_secure = true
least_privilege = true
audit_everything = true

[self_improvement]
requires_user_approval = true
sandbox_mandatory = true
regression_detection_mandatory = true
constitution_immutable = true
"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("constitution.toml");
        std::fs::write(&path, toml_content).unwrap();

        let config: ConstitutionConfig = load_config(&path).unwrap();
        assert!(config.principles.user_sovereignty);
        assert!(config.principles.privacy_first);
        assert!(config.security.fail_secure);
        assert!(config.self_improvement.constitution_immutable);
        assert_eq!(config.autonomy.default_trust_level, "standard");
    }

    #[test]
    fn test_load_models_config() {
        let toml_content = r#"
[defaults]
embedding_model = "nomic-embed-text"

[[resources]]
name = "ollama/llama3.1:8b"
provider = "ollama"
resource_type = "local_model"
endpoint = "http://localhost:11434"
context_window = 8192
privacy_level = "local"

[resources.capabilities]
text_generation = 0.7
conversation = 0.75
"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.toml");
        std::fs::write(&path, toml_content).unwrap();

        let config: ModelsConfig = load_config(&path).unwrap();
        assert_eq!(config.defaults.embedding_model, "nomic-embed-text");
        assert_eq!(config.resources.len(), 1);
        assert_eq!(config.resources[0].name, "ollama/llama3.1:8b");
        assert_eq!(config.resources[0].provider, "ollama");
        let caps = &config.resources[0].capabilities;
        assert_eq!(*caps.get("text_generation").unwrap(), 0.7);
        assert_eq!(*caps.get("conversation").unwrap(), 0.75);
    }

    #[test]
    fn test_load_permissions_config() {
        let toml_content = r#"
[default]
mode = "allow_all"
"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("permissions.toml");
        std::fs::write(&path, toml_content).unwrap();

        let config: PermissionsConfig = load_config(&path).unwrap();
        assert_eq!(config.default.mode, "allow_all");
    }

    #[test]
    fn test_hash_file_produces_hex() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"hello world").unwrap();
        drop(f);

        let hash = hash_file(&path).unwrap();
        // SHA-256 of "hello world" is known
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn test_hash_file_not_found() {
        let result = hash_file(Path::new("/nonexistent/file.toml"));
        assert!(result.is_err());
    }

    #[test]
    fn test_config_defaults() {
        // Verify defaults work when sections are minimal
        let toml_content = r#"
[alpha]
"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("alpha.toml");
        std::fs::write(&path, toml_content).unwrap();

        let config: AlphaConfig = load_config(&path).unwrap();
        assert_eq!(config.alpha.data_dir, "data");
        assert_eq!(config.alpha.logging.level, "info");
        assert_eq!(config.alpha.logging.format, "pretty");
        assert_eq!(config.alpha.event_bus.purge_after_days, 30);
        assert_eq!(config.alpha.identity.name, "Alpha");
    }
}
