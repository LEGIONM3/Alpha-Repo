//! AI Resource Record schema (Canonical Schema 6).
//!
//! Stored in: `aris` SQLite DB.
//! The Model Router selects resources by querying this schema.

use serde::{Deserialize, Serialize};
use crate::types::{AlphaId, Timestamp, JsonValue};

/// Type of AI resource.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceType {
    LocalModel,
    CloudApi,
    InstalledApp,
    SpecializedTool,
    McpServer,
}

/// Current status of an AI resource.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceStatus {
    Online,
    Offline,
    Degraded,
    #[default]
    Unknown,
}

/// Authentication method for connecting to a resource.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    #[default]
    None,
    ApiKey,
    Oauth,
    UiAutomation,
}

/// Privacy level of the resource.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyLevel {
    /// Data stays on the local machine.
    #[default]
    Local,
    /// Data is sent to a cloud provider.
    Cloud,
}

/// A capability score for a specific task domain.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Capability {
    /// Task taxonomy domain (e.g., "text_generation", "code_generation").
    pub domain: String,
    /// Score from 0.0 to 1.0. Initially static, later learned via ARIS.
    pub score: f32,
    /// Number of data points this score is based on.
    pub sample_count: u32,
}

/// A registered AI resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIResource {
    pub id: AlphaId,
    pub resource_type: ResourceType,
    /// Human-readable name, e.g., "ollama/llama3.1:8b".
    pub name: String,
    /// Provider identifier, e.g., "ollama".
    pub provider: String,
    pub status: ResourceStatus,

    // Connection
    /// URL, path, or connection descriptor.
    pub endpoint: String,
    pub auth_method: AuthMethod,

    // Capabilities
    #[serde(default)]
    pub capabilities: Vec<Capability>,

    // Performance (populated over time)
    pub latency_p50_ms: Option<f32>,
    /// Cost per request in USD.
    pub cost_per_request: Option<f32>,
    /// Uptime percentage.
    pub reliability_pct: Option<f32>,

    // Metadata
    /// Maximum context window in tokens.
    pub context_window: Option<u32>,
    pub requires_network: bool,
    pub privacy_level: PrivacyLevel,
    pub discovered_at: Timestamp,
    pub last_health_check: Option<Timestamp>,
    #[serde(default)]
    pub metadata: JsonValue,
}

impl AIResource {
    /// Create a new local Ollama model resource.
    pub fn new_ollama_model(
        name: String,
        endpoint: String,
        context_window: Option<u32>,
        capabilities: Vec<Capability>,
    ) -> Self {
        Self {
            id: crate::types::new_id(),
            resource_type: ResourceType::LocalModel,
            name,
            provider: "ollama".to_string(),
            status: ResourceStatus::Unknown,
            endpoint,
            auth_method: AuthMethod::None,
            capabilities,
            latency_p50_ms: None,
            cost_per_request: None,
            reliability_pct: None,
            context_window,
            requires_network: false,
            privacy_level: PrivacyLevel::Local,
            discovered_at: crate::types::now(),
            last_health_check: None,
            metadata: JsonValue::Null,
        }
    }
}
