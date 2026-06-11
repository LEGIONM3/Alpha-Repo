//! Unit tests for the alpha-resource crate.

use super::*;

#[test]
fn test_detect_hardware_returns_values() {
    let profile = detect_hardware();

    // All string fields should be non-empty.
    assert!(!profile.cpu_name.is_empty(), "cpu_name must not be empty");
    assert!(!profile.os.is_empty(), "os must not be empty");
    assert!(!profile.arch.is_empty(), "arch must not be empty");

    // Numeric fields should be positive.
    assert!(profile.ram_total_mb > 0, "ram_total_mb must be > 0");

    // Sprint 2: GPU/NPU are best-effort stubs.
    assert!(profile.gpu.is_none(), "GPU should be None in Sprint 2");
    assert!(!profile.npu_detected, "NPU should be false in Sprint 2");
}

#[test]
fn test_cpu_cores_positive() {
    let profile = detect_hardware();

    assert!(
        profile.cpu_cores_physical > 0,
        "Must detect at least 1 physical core, got {}",
        profile.cpu_cores_physical
    );
    assert!(
        profile.cpu_cores_logical > 0,
        "Must detect at least 1 logical core, got {}",
        profile.cpu_cores_logical
    );
    assert!(
        profile.cpu_cores_logical >= profile.cpu_cores_physical,
        "Logical cores ({}) must be >= physical cores ({})",
        profile.cpu_cores_logical,
        profile.cpu_cores_physical
    );
}

#[test]
fn test_ram_positive() {
    let profile = detect_hardware();

    assert!(
        profile.ram_total_mb > 0,
        "Total RAM must be > 0 MB, got {}",
        profile.ram_total_mb
    );
    // Available RAM should be <= total RAM.
    assert!(
        profile.ram_available_mb <= profile.ram_total_mb,
        "Available RAM ({} MB) must be <= total RAM ({} MB)",
        profile.ram_available_mb,
        profile.ram_total_mb
    );
}

#[test]
fn test_os_and_arch_nonempty() {
    let profile = detect_hardware();

    assert!(!profile.os.is_empty(), "OS must not be empty");
    assert!(!profile.arch.is_empty(), "Arch must not be empty");

    // On any real machine, OS should be one of the known values.
    let known_os = ["windows", "linux", "macos", "freebsd", "android", "ios"];
    assert!(
        known_os.contains(&profile.os.as_str()),
        "OS '{}' is not in the expected set {:?}",
        profile.os,
        known_os
    );

    let known_arch = [
        "x86_64", "x86", "aarch64", "arm", "mips", "mips64", "powerpc",
        "powerpc64", "riscv64", "s390x",
    ];
    assert!(
        known_arch.contains(&profile.arch.as_str()),
        "Arch '{}' is not in the expected set {:?}",
        profile.arch,
        known_arch
    );
}

#[test]
fn test_hardware_profile_display() {
    let profile = detect_hardware();
    let display = format!("{}", profile);

    // Display should contain key information.
    assert!(display.contains(&profile.cpu_name));
    assert!(display.contains(&profile.os));
    assert!(display.contains(&profile.arch));
}

#[test]
fn test_hardware_profile_serialization_roundtrip() {
    let profile = detect_hardware();

    let json = serde_json::to_string(&profile).expect("serialize");
    let deserialized: HardwareProfile =
        serde_json::from_str(&json).expect("deserialize");

    assert_eq!(profile.cpu_name, deserialized.cpu_name);
    assert_eq!(profile.cpu_cores_physical, deserialized.cpu_cores_physical);
    assert_eq!(profile.cpu_cores_logical, deserialized.cpu_cores_logical);
    assert_eq!(profile.ram_total_mb, deserialized.ram_total_mb);
    assert_eq!(profile.os, deserialized.os);
    assert_eq!(profile.arch, deserialized.arch);
    assert!(deserialized.gpu.is_none());
    assert!(!deserialized.npu_detected);
}

#[test]
fn test_gpu_info_serialization() {
    let gpu = GpuInfo {
        name: "NVIDIA RTX 4090".to_string(),
        vram_mb: Some(24576),
    };

    let json = serde_json::to_string(&gpu).expect("serialize");
    let deserialized: GpuInfo = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(gpu.name, deserialized.name);
    assert_eq!(gpu.vram_mb, deserialized.vram_mb);
}
