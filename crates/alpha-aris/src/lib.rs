//! # alpha-aris
//!
//! AI Resource Intelligence System (Sprint 1 stub).
//!
//! ARIS is Alpha's registry of all connected AI resources — local models,
//! cloud APIs, installed applications, and specialized tools.
//!
//! ## Sprint 1 Scope
//!
//! - **Static registry**: resources are loaded from `models.toml` on startup.
//! - **Capability query**: find resources by task domain, sorted by score.
//! - **Result logging**: task results are recorded but do NOT update scores.
//! - **Event publishing**: `alpha.aris.resource.discovered` on registration.
//!
//! ## NOT in Sprint 1
//!
//! - Learning engine / Bayesian score updates
//! - Dynamic capability discovery
//! - Automatic optimization

pub mod registry;
pub mod types;

#[cfg(test)]
mod tests;

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use tracing::{debug, info};

use alpha_common::config::{ModelsConfig, ResourceConfig};
use alpha_common::error::AlphaError;
use alpha_common::event::Event;
use alpha_common::schemas::ai_resource::*;
use alpha_common::types::{AlphaId, new_id, now};

use alpha_event_bus::EventBus;

pub use types::{ResourceConstraints, ScoredResource, TaskResult};

/// AI Resource Intelligence System.
///
/// Sprint 1: static registry backed by SQLite. No learning engine.
pub struct Aris {
    conn: Mutex<Connection>,
    event_bus: Arc<EventBus>,
}

impl Aris {
    /// Open or create the ARIS registry.
    ///
    /// - Creates the database file and parent directories.
    /// - Enables WAL mode.
    /// - Creates the `resources` and `result_log` tables.
    pub fn open(db_path: &Path, event_bus: Arc<EventBus>) -> Result<Self, AlphaError> {
        // Ensure parent directory exists.
        if let Some(parent) = db_path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    AlphaError::Config(format!(
                        "Failed to create ARIS directory '{}': {}",
                        parent.display(),
                        e
                    ))
                })?;
            }
        }

        let conn = Connection::open(db_path).map_err(|e| {
            AlphaError::Database(format!(
                "Failed to open ARIS database at '{}': {}",
                db_path.display(),
                e
            ))
        })?;

        // Enable WAL mode.
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;",
        )
        .map_err(|e| AlphaError::Database(format!("Failed to set ARIS pragmas: {}", e)))?;

        // Create tables.
        conn.execute_batch(registry::DDL)
            .map_err(|e| AlphaError::Database(format!("Failed to create ARIS tables: {}", e)))?;

        info!(path = %db_path.display(), "ARIS opened");

        Ok(Self { conn: Mutex::new(conn), event_bus })
    }

    /// Load initial resources from a models.toml config.
    ///
    /// For each resource in the config, converts it to an `AIResource`
    /// and registers it (persists + publishes discovery event).
    /// Returns the IDs of all loaded resources.
    pub async fn load_from_config(
        &self,
        config: &ModelsConfig,
    ) -> Result<Vec<AlphaId>, AlphaError> {
        let mut ids = Vec::new();

        for res_config in &config.resources {
            let resource = config_to_resource(res_config);
            let id = self.register(resource).await?;
            ids.push(id);
        }

        info!(count = ids.len(), "Resources loaded from config");
        Ok(ids)
    }

    /// Register a new AI resource.
    ///
    /// 1. Persists to SQLite.
    /// 2. Publishes `alpha.aris.resource.discovered` event.
    /// 3. Returns the resource ID.
    pub async fn register(&self, resource: AIResource) -> Result<AlphaId, AlphaError> {
        let id = resource.id;

        // Persist to database.
        {
            let conn = self.lock()?;
            registry::insert_resource(&conn, &resource)?;
        }

        debug!(name = %resource.name, id = %id, "Resource registered");

        // Publish discovery event.
        let event = Event::new(
            alpha_common::topics::ARIS_RESOURCE_DISCOVERED,
            "aris",
            serde_json::json!({
                "resource_id": id.to_string(),
                "name": resource.name,
                "provider": resource.provider,
                "resource_type": format!("{:?}", resource.resource_type),
            }),
        );

        self.event_bus.publish(event).await?;

        Ok(id)
    }

    /// Query resources matching a task domain and constraints.
    ///
    /// 1. Loads all resources from SQLite.
    /// 2. Filters by constraints (local_only, status, min_capability_score).
    /// 3. Scores each resource by the queried domain's capability score.
    /// 4. Returns sorted descending by score.
    pub fn query(
        &self,
        domain: &str,
        constraints: &ResourceConstraints,
    ) -> Result<Vec<ScoredResource>, AlphaError> {
        let conn = self.lock()?;
        let all_resources = registry::get_all_resources(&conn)?;

        let mut scored: Vec<ScoredResource> = all_resources
            .into_iter()
            .filter_map(|resource| {
                // Find capability score for the queried domain.
                let score = resource
                    .capabilities
                    .iter()
                    .find(|c| c.domain == domain)
                    .map(|c| c.score)?;

                // Apply constraints.

                // local_only: reject cloud resources.
                if constraints.local_only && resource.privacy_level != PrivacyLevel::Local {
                    return None;
                }

                // status_filter: reject mismatched status.
                if let Some(ref required_status) = constraints.status_filter {
                    if resource.status != *required_status {
                        return None;
                    }
                }

                // min_capability_score: reject below threshold.
                if let Some(min_score) = constraints.min_capability_score {
                    if score < min_score {
                        return None;
                    }
                }

                // max_cost_usd: reject too expensive.
                if let Some(max_cost) = constraints.max_cost_usd {
                    if let Some(cost) = resource.cost_per_request {
                        if cost > max_cost {
                            return None;
                        }
                    }
                }

                Some(ScoredResource { resource, score })
            })
            .collect();

        // Sort by score descending.
        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        Ok(scored)
    }

    /// Report an inference result.
    ///
    /// **Sprint 1 stub**: writes to `result_log` but does NOT update
    /// capability scores. Future sprints will implement Bayesian updates.
    pub fn report_result(
        &self,
        result: &TaskResult,
    ) -> Result<(), AlphaError> {
        let conn = self.lock()?;
        registry::insert_result(&conn, result)?;

        debug!(
            resource_id = %result.resource_id,
            domain = %result.task_domain,
            success = result.success,
            latency_ms = result.latency_ms,
            "Result logged (scores NOT updated — Sprint 1 stub)"
        );

        Ok(())
    }

    /// Check if a resource is reachable.
    ///
    /// **Sprint 1 stub**: returns last known status from the database.
    /// Does NOT perform an actual health check probe.
    pub fn health_check(&self, resource_id: AlphaId) -> Result<ResourceStatus, AlphaError> {
        let conn = self.lock()?;
        let resource = registry::get_resource(&conn, &resource_id)?
            .ok_or_else(|| AlphaError::NotFound {
                entity: "AIResource".to_string(),
                id: resource_id.to_string(),
            })?;

        debug!(
            resource_id = %resource_id,
            status = ?resource.status,
            "Health check (stub: returning last known status)"
        );

        Ok(resource.status)
    }

    /// Get all registered resources.
    pub fn get_all(&self) -> Result<Vec<AIResource>, AlphaError> {
        let conn = self.lock()?;
        registry::get_all_resources(&conn)
    }

    /// Get the count of result log entries for a specific resource.
    pub fn result_count(&self, resource_id: &AlphaId) -> Result<u64, AlphaError> {
        let conn = self.lock()?;
        registry::result_count(&conn, resource_id)
    }
}

impl Aris {
    /// Acquire the connection lock.
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, AlphaError> {
        self.conn.lock().map_err(|e| {
            AlphaError::Database(format!("Failed to acquire ARIS lock: {e}"))
        })
    }
}

impl alpha_common::traits::Service for Aris {
    fn name(&self) -> &str {
        "aris"
    }

    fn init(&mut self) -> Result<(), AlphaError> {
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), AlphaError> {
        let conn = self.conn.lock().map_err(|e| {
            AlphaError::Database(format!("Failed to acquire ARIS lock for shutdown: {e}"))
        })?;
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|e| {
                AlphaError::Database(format!(
                    "Failed to checkpoint ARIS WAL on shutdown: {}",
                    e
                ))
            })?;
        info!("ARIS shutdown: WAL checkpointed");
        Ok(())
    }
}

// ── Helpers ──

/// Convert a `ResourceConfig` (from models.toml) to an `AIResource`.
fn config_to_resource(config: &ResourceConfig) -> AIResource {
    let resource_type: ResourceType =
        serde_json::from_str(&format!("\"{}\"", config.resource_type))
            .unwrap_or(ResourceType::LocalModel);

    let auth_method: AuthMethod =
        serde_json::from_str(&format!("\"{}\"", config.auth_method))
            .unwrap_or_default();

    let privacy_level: PrivacyLevel =
        serde_json::from_str(&format!("\"{}\"", config.privacy_level))
            .unwrap_or_default();

    let capabilities: Vec<Capability> = config
        .capabilities
        .iter()
        .map(|(domain, score)| Capability {
            domain: domain.clone(),
            score: *score,
            sample_count: 0,
        })
        .collect();

    AIResource {
        id: new_id(),
        resource_type,
        name: config.name.clone(),
        provider: config.provider.clone(),
        status: ResourceStatus::Unknown,
        endpoint: config.endpoint.clone(),
        auth_method,
        capabilities,
        latency_p50_ms: None,
        cost_per_request: None,
        reliability_pct: None,
        context_window: config.context_window,
        requires_network: config.requires_network,
        privacy_level,
        discovered_at: now(),
        last_health_check: None,
        metadata: serde_json::Value::Null,
    }
}
