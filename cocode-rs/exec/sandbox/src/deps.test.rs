use std::path::PathBuf;

use crate::config::SeccompConfig;

use super::*;

#[test]
fn test_check_dependencies_returns_results() {
    let checks = check_dependencies();
    // Should have at least one check on any supported platform
    if cfg!(target_os = "macos") || cfg!(target_os = "linux") {
        assert!(!checks.is_empty());
    }
}

#[test]
fn test_check_dependencies_macos() {
    if !cfg!(target_os = "macos") {
        return;
    }
    let checks = check_dependencies();
    let sandbox_exec = checks.iter().find(|c| c.name == "sandbox-exec");
    assert!(sandbox_exec.is_some());
    let check = sandbox_exec.expect("sandbox-exec check");
    assert!(check.required);
    // sandbox-exec should always be available on macOS
    assert!(check.available);
}

#[test]
fn test_check_dependencies_linux() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let checks = check_dependencies();
    let bwrap = checks.iter().find(|c| c.name == "bwrap");
    assert!(bwrap.is_some());
    let bwrap_check = bwrap.expect("bwrap check");
    assert!(bwrap_check.required);

    let socat = checks.iter().find(|c| c.name == "socat");
    assert!(socat.is_some());
    let socat_check = socat.expect("socat check");
    assert!(!socat_check.required); // Optional
}

#[test]
fn test_missing_required() {
    let missing = missing_required();
    // Just verify it returns a vec (actual content depends on platform)
    let _ = missing;
}

#[test]
fn test_all_required_available() {
    // On macOS, sandbox-exec should always be present
    if cfg!(target_os = "macos") {
        assert!(all_required_available());
    }
    // On other platforms, just check it doesn't panic
    let _ = all_required_available();
}

// ==========================================================================
// Seccomp dependency checks
// ==========================================================================

#[test]
fn test_check_dependencies_with_seccomp_default_no_seccomp_entries() {
    let checks = check_dependencies_with_seccomp(&SeccompConfig::default());
    // Without seccomp configured, no seccomp-related checks should appear
    assert!(checks.iter().all(|c| c.name != "seccomp-bpf"));
    assert!(checks.iter().all(|c| c.name != "seccomp-apply"));
}

#[test]
fn test_check_dependencies_with_seccomp_bpf_configured() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let seccomp = SeccompConfig {
        bpf_path: Some(PathBuf::from("/nonexistent/filter.bpf")),
        apply_path: None,
    };
    let checks = check_dependencies_with_seccomp(&seccomp);

    let bpf_check = checks
        .iter()
        .find(|c| c.name == "seccomp-bpf")
        .expect("seccomp-bpf check");
    assert!(!bpf_check.required);
    assert!(!bpf_check.available); // Nonexistent path

    let apply_check = checks
        .iter()
        .find(|c| c.name == "seccomp-apply")
        .expect("seccomp-apply check");
    assert!(!apply_check.required);
}

#[test]
fn test_check_dependencies_with_seccomp_bpf_exists() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let bpf = dir.path().join("filter.bpf");
    std::fs::write(&bpf, b"dummy bpf").expect("write bpf");

    let seccomp = SeccompConfig {
        bpf_path: Some(bpf.clone()),
        apply_path: None,
    };
    let checks = check_dependencies_with_seccomp(&seccomp);

    let bpf_check = checks
        .iter()
        .find(|c| c.name == "seccomp-bpf")
        .expect("seccomp-bpf check");
    assert!(bpf_check.available);
    assert_eq!(bpf_check.path, Some(bpf));
}

#[test]
fn test_check_dependencies_with_seccomp_explicit_apply_path() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let bpf = dir.path().join("filter.bpf");
    std::fs::write(&bpf, b"dummy bpf").expect("write bpf");
    let apply = dir.path().join("seccomp-apply");
    std::fs::write(&apply, b"#!/bin/sh").expect("write apply");

    let seccomp = SeccompConfig {
        bpf_path: Some(bpf),
        apply_path: Some(apply.clone()),
    };
    let checks = check_dependencies_with_seccomp(&seccomp);

    let apply_check = checks
        .iter()
        .find(|c| c.name == "seccomp-apply")
        .expect("seccomp-apply check");
    assert!(apply_check.available);
    assert_eq!(apply_check.path, Some(apply));
}
