//! # Alpha — Main Entry Point
//!
//! The binary target for Project Alpha.
//!
//! Startup:
//! 1. Parse configuration files.
//! 2. Initialize tracing.
//! 3. Start AlphaApp (three-phase initialization).
//! 4. Wait for Ctrl+C.
//! 5. Graceful shutdown.

pub mod app;
pub mod identity;
pub mod init;
pub mod provider;
pub mod shutdown;

#[cfg(test)]
mod tests;

use std::path::PathBuf;

use tracing::info;
use tracing_subscriber::EnvFilter;

use alpha_common::config::{AlphaConfig, ModelsConfig, load_config};
use alpha_common::error::AlphaError;

use app::AlphaApp;

/// Default paths relative to the project root.
const DEFAULT_CONFIG_DIR: &str = "config";
const DEFAULT_ALPHA_CONFIG: &str = "alpha.toml";
const DEFAULT_CONSTITUTION: &str = "constitution.toml";
const DEFAULT_MODELS_CONFIG: &str = "models.toml";

#[tokio::main]
async fn main() -> Result<(), AlphaError> {
    // ── 1. Initialize tracing ──
    // Use env filter so RUST_LOG can override.
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_ids(false)
        .init();

    info!("Starting Project Alpha...");

    // ── 2. Load configuration files ──
    let config_dir = PathBuf::from(DEFAULT_CONFIG_DIR);

    let alpha_config_path = config_dir.join(DEFAULT_ALPHA_CONFIG);
    let alpha_config: AlphaConfig = load_config(&alpha_config_path)?;

    let models_config_path = config_dir.join(DEFAULT_MODELS_CONFIG);
    let models_config: ModelsConfig = load_config(&models_config_path)?;

    let constitution_path = config_dir.join(DEFAULT_CONSTITUTION);

    let data_dir = PathBuf::from(&alpha_config.alpha.data_dir);

    // ── 3. Start AlphaApp ──
    let mut app = AlphaApp::start(
        &data_dir,
        &alpha_config,
        &models_config,
        &constitution_path,
    )
    .await?;

    info!(
        alpha_id = %app.identity.alpha_id,
        "Alpha is ready. Press Ctrl+C to shutdown."
    );

    // ── 4. Wait for shutdown signal ──
    tokio::signal::ctrl_c()
        .await
        .map_err(|e| AlphaError::Other(format!("Failed to listen for Ctrl+C: {}", e)))?;

    info!("Shutdown signal received.");

    // ── 5. Graceful shutdown ──
    app.shutdown().await?;

    Ok(())
}
