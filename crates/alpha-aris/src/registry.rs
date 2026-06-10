//! SQLite persistence layer for the ARIS resource registry.
//!
//! Tables:
//! - `resources`: registered AI resources with capabilities.
//! - `result_log`: task execution results (stub: no score updates).

use rusqlite::{Connection, OptionalExtension, params};

use alpha_common::error::AlphaError;
use alpha_common::schemas::ai_resource::*;
use alpha_common::types::{AlphaId, new_id, now};

use crate::types::TaskResult;

/// SQL DDL for the resources and result_log tables.
pub(crate) const DDL: &str = "
    CREATE TABLE IF NOT EXISTS resources (
        id              TEXT PRIMARY KEY,
        resource_type   TEXT NOT NULL,
        name            TEXT NOT NULL,
        provider        TEXT NOT NULL,
        status          TEXT NOT NULL DEFAULT 'unknown',
        endpoint        TEXT NOT NULL,
        auth_method     TEXT NOT NULL DEFAULT 'none',
        capabilities    TEXT NOT NULL DEFAULT '[]',
        latency_p50_ms  REAL,
        cost_per_request REAL,
        reliability_pct REAL,
        context_window  INTEGER,
        requires_network INTEGER NOT NULL DEFAULT 0,
        privacy_level   TEXT NOT NULL DEFAULT 'local',
        discovered_at   TEXT NOT NULL,
        last_health_check TEXT,
        metadata        TEXT NOT NULL DEFAULT '{}'
    );

    CREATE INDEX IF NOT EXISTS idx_resources_provider ON resources(provider);
    CREATE INDEX IF NOT EXISTS idx_resources_status ON resources(status);

    CREATE TABLE IF NOT EXISTS result_log (
        id              TEXT PRIMARY KEY,
        resource_id     TEXT NOT NULL REFERENCES resources(id),
        task_domain     TEXT NOT NULL,
        success         INTEGER NOT NULL,
        latency_ms      INTEGER NOT NULL,
        tokens_in       INTEGER,
        tokens_out      INTEGER,
        user_satisfaction REAL,
        timestamp       TEXT NOT NULL
    );
";

/// Insert a resource into the database.
pub(crate) fn insert_resource(
    conn: &Connection,
    resource: &AIResource,
) -> Result<(), AlphaError> {
    let id = resource.id.to_string();
    let resource_type = serde_json::to_string(&resource.resource_type)?;
    let resource_type = resource_type.trim_matches('"');
    let status = serde_json::to_string(&resource.status)?;
    let status = status.trim_matches('"');
    let auth_method = serde_json::to_string(&resource.auth_method)?;
    let auth_method = auth_method.trim_matches('"');
    let capabilities = serde_json::to_string(&resource.capabilities)?;
    let privacy_level = serde_json::to_string(&resource.privacy_level)?;
    let privacy_level = privacy_level.trim_matches('"');
    let discovered_at = resource.discovered_at.to_rfc3339();
    let last_health_check = resource.last_health_check.map(|t| t.to_rfc3339());
    let metadata = serde_json::to_string(&resource.metadata)?;

    conn.execute(
        "INSERT OR REPLACE INTO resources
         (id, resource_type, name, provider, status, endpoint, auth_method,
          capabilities, latency_p50_ms, cost_per_request, reliability_pct,
          context_window, requires_network, privacy_level, discovered_at,
          last_health_check, metadata)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            id,
            resource_type,
            resource.name,
            resource.provider,
            status,
            resource.endpoint,
            auth_method,
            capabilities,
            resource.latency_p50_ms,
            resource.cost_per_request,
            resource.reliability_pct,
            resource.context_window,
            resource.requires_network as i32,
            privacy_level,
            discovered_at,
            last_health_check,
            metadata,
        ],
    )
    .map_err(|e| AlphaError::Database(format!("Failed to insert resource: {}", e)))?;

    Ok(())
}

/// Get a resource by ID.
pub(crate) fn get_resource(
    conn: &Connection,
    id: &AlphaId,
) -> Result<Option<AIResource>, AlphaError> {
    let id_str = id.to_string();

    let result = conn
        .query_row(
            "SELECT id, resource_type, name, provider, status, endpoint, auth_method,
                    capabilities, latency_p50_ms, cost_per_request, reliability_pct,
                    context_window, requires_network, privacy_level, discovered_at,
                    last_health_check, metadata
             FROM resources WHERE id = ?1",
            params![id_str],
            row_to_resource,
        )
        .optional()
        .map_err(|e| AlphaError::Database(format!("Failed to get resource: {}", e)))?;

    Ok(result)
}

/// Get all registered resources.
pub(crate) fn get_all_resources(conn: &Connection) -> Result<Vec<AIResource>, AlphaError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, resource_type, name, provider, status, endpoint, auth_method,
                    capabilities, latency_p50_ms, cost_per_request, reliability_pct,
                    context_window, requires_network, privacy_level, discovered_at,
                    last_health_check, metadata
             FROM resources ORDER BY name",
        )
        .map_err(|e| AlphaError::Database(format!("get_all prepare failed: {}", e)))?;

    let resources = stmt
        .query_map([], row_to_resource)
        .map_err(|e| AlphaError::Database(format!("get_all query failed: {}", e)))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| AlphaError::Database(format!("get_all collect failed: {}", e)))?;

    Ok(resources)
}

/// Insert a task result into the result_log.
pub(crate) fn insert_result(
    conn: &Connection,
    result: &TaskResult,
) -> Result<(), AlphaError> {
    let id = new_id().to_string();
    let resource_id = result.resource_id.to_string();
    let timestamp = now().to_rfc3339();

    conn.execute(
        "INSERT INTO result_log
         (id, resource_id, task_domain, success, latency_ms, tokens_in, tokens_out,
          user_satisfaction, timestamp)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            id,
            resource_id,
            result.task_domain,
            result.success as i32,
            result.latency_ms as i64,
            result.tokens_in,
            result.tokens_out,
            result.user_satisfaction,
            timestamp,
        ],
    )
    .map_err(|e| AlphaError::Database(format!("Failed to insert result: {}", e)))?;

    Ok(())
}

/// Count result log entries for a resource.
pub(crate) fn result_count(conn: &Connection, resource_id: &AlphaId) -> Result<u64, AlphaError> {
    let id_str = resource_id.to_string();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM result_log WHERE resource_id = ?1",
            params![id_str],
            |row| row.get(0),
        )
        .map_err(|e| AlphaError::Database(format!("result_count failed: {}", e)))?;

    Ok(count as u64)
}

// ── Row mapper ──

fn row_to_resource(row: &rusqlite::Row<'_>) -> rusqlite::Result<AIResource> {
    use chrono::DateTime;

    let id_str: String = row.get(0)?;
    let resource_type_str: String = row.get(1)?;
    let name: String = row.get(2)?;
    let provider: String = row.get(3)?;
    let status_str: String = row.get(4)?;
    let endpoint: String = row.get(5)?;
    let auth_method_str: String = row.get(6)?;
    let capabilities_str: String = row.get(7)?;
    let latency_p50_ms: Option<f32> = row.get(8)?;
    let cost_per_request: Option<f32> = row.get(9)?;
    let reliability_pct: Option<f32> = row.get(10)?;
    let context_window: Option<u32> = row.get(11)?;
    let requires_network_int: i32 = row.get(12)?;
    let privacy_level_str: String = row.get(13)?;
    let discovered_at_str: String = row.get(14)?;
    let last_health_check_str: Option<String> = row.get(15)?;
    let metadata_str: String = row.get(16)?;

    // Parse enums with fallback defaults.
    let id = id_str.parse().unwrap_or_else(|_| alpha_common::types::new_id());
    let resource_type: ResourceType =
        serde_json::from_str(&format!("\"{}\"", resource_type_str))
            .unwrap_or(ResourceType::LocalModel);
    let status: ResourceStatus =
        serde_json::from_str(&format!("\"{}\"", status_str))
            .unwrap_or_default();
    let auth_method: AuthMethod =
        serde_json::from_str(&format!("\"{}\"", auth_method_str))
            .unwrap_or_default();
    let capabilities: Vec<Capability> =
        serde_json::from_str(&capabilities_str).unwrap_or_default();
    let privacy_level: PrivacyLevel =
        serde_json::from_str(&format!("\"{}\"", privacy_level_str))
            .unwrap_or_default();
    let discovered_at = DateTime::parse_from_rfc3339(&discovered_at_str)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| alpha_common::types::now());
    let last_health_check = last_health_check_str.and_then(|s| {
        DateTime::parse_from_rfc3339(&s)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .ok()
    });
    let metadata: serde_json::Value =
        serde_json::from_str(&metadata_str).unwrap_or(serde_json::Value::Null);

    Ok(AIResource {
        id,
        resource_type,
        name,
        provider,
        status,
        endpoint,
        auth_method,
        capabilities,
        latency_p50_ms,
        cost_per_request,
        reliability_pct,
        context_window,
        requires_network: requires_network_int != 0,
        privacy_level,
        discovered_at,
        last_health_check,
        metadata,
    })
}
