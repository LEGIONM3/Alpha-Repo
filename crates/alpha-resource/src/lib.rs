//! Hardware detection for Project Alpha.
//!
//! Detects local hardware capabilities using the `sysinfo` crate.
//! Used by ARIS and ModelRouter for resource-aware decisions.
//!
//! Sprint 2 scope:
//! - **Required**: CPU, RAM, OS, architecture
//! - **Best-effort**: GPU (returns `None`), NPU (returns `false`)
//!
//! Deep GPU/NPU detection is deferred to Sprint 3+.

use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};
use tracing::info;

/// Hardware capabilities detected on the local machine.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HardwareProfile {
    /// CPU brand/model string (e.g., "AMD Ryzen 9 7945HX").
    pub cpu_name: String,
    /// Number of physical CPU cores.
    pub cpu_cores_physical: u32,
    /// Number of logical CPU cores (including hyperthreads).
    pub cpu_cores_logical: u32,
    /// Total installed RAM in megabytes.
    pub ram_total_mb: u64,
    /// Currently available RAM in megabytes.
    pub ram_available_mb: u64,
    /// GPU information (best-effort; `None` in Sprint 2).
    pub gpu: Option<GpuInfo>,
    /// Whether an NPU was detected (best-effort; `false` in Sprint 2).
    pub npu_detected: bool,
    /// Operating system name (e.g., "windows", "linux", "macos").
    pub os: String,
    /// CPU architecture (e.g., "x86_64", "aarch64").
    pub arch: String,
}

/// GPU information.
///
/// Sprint 2: this struct is defined for forward compatibility but
/// `detect_hardware()` always returns `gpu: None`. True GPU detection
/// (CUDA/ROCm/Metal) is a Sprint 3+ concern.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GpuInfo {
    /// GPU name (e.g., "NVIDIA RTX 4090").
    pub name: String,
    /// Video RAM in megabytes, if detectable.
    pub vram_mb: Option<u64>,
}

/// Detect local hardware capabilities.
///
/// Uses the `sysinfo` crate for CPU and memory information.
/// OS and architecture come from `std::env::consts`.
///
/// # Returns
///
/// A `HardwareProfile` with all required fields populated.
/// GPU and NPU fields are best-effort stubs in Sprint 2.
pub fn detect_hardware() -> HardwareProfile {
    // Create a System instance with CPU and memory info refreshed.
    let sys = System::new_with_specifics(
        RefreshKind::nothing()
            .with_cpu(CpuRefreshKind::nothing())
            .with_memory(MemoryRefreshKind::everything()),
    );

    // CPU brand: take the brand string from the first logical CPU.
    // `brand()` returns something like "AMD Ryzen 9 7945HX with Radeon Graphics".
    let cpu_name = sys
        .cpus()
        .first()
        .map(|cpu| cpu.brand().to_string())
        .unwrap_or_else(|| "Unknown CPU".to_string());

    let cpu_cores_physical = sys.physical_core_count().unwrap_or(0) as u32;
    let cpu_cores_logical = sys.cpus().len() as u32;

    // Memory: sysinfo returns bytes, we convert to megabytes.
    let ram_total_mb = sys.total_memory() / (1024 * 1024);
    let ram_available_mb = sys.available_memory() / (1024 * 1024);

    let os = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();

    let profile = HardwareProfile {
        cpu_name,
        cpu_cores_physical,
        cpu_cores_logical,
        ram_total_mb,
        ram_available_mb,
        gpu: None,         // Best-effort: deferred to Sprint 3+.
        npu_detected: false, // Best-effort: deferred to Sprint 3+.
        os,
        arch,
    };

    info!(
        cpu = %profile.cpu_name,
        physical_cores = profile.cpu_cores_physical,
        logical_cores = profile.cpu_cores_logical,
        ram_total_mb = profile.ram_total_mb,
        ram_available_mb = profile.ram_available_mb,
        os = %profile.os,
        arch = %profile.arch,
        "Hardware profile detected"
    );

    profile
}

impl std::fmt::Display for HardwareProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} | {}C/{}T | {} MB RAM | {} {}",
            self.cpu_name,
            self.cpu_cores_physical,
            self.cpu_cores_logical,
            self.ram_total_mb,
            self.os,
            self.arch,
        )
    }
}

#[cfg(test)]
mod tests;
