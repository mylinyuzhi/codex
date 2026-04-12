use std::path::PathBuf;

use super::*;

fn default_config() -> SandboxConfig {
    SandboxConfig {
        mode: SandboxMode::Strict,
        writable_roots: vec![PathBuf::from("/project")],
        excluded_commands: vec!["git".into(), "npm:*".into()],
        auto_allow_if_sandboxed: true,
        allow_bypass: true,
        allow_network: false,
        platform_binary: None,
    }
}

// ── SandboxMode ──

#[test]
fn test_sandbox_mode_is_active() {
    assert!(!SandboxMode::None.is_active());
    assert!(SandboxMode::ReadOnly.is_active());
    assert!(SandboxMode::Strict.is_active());
    assert!(SandboxMode::External.is_active());
}

#[test]
fn test_sandbox_mode_blocks_writes() {
    assert!(!SandboxMode::None.blocks_writes());
    assert!(SandboxMode::ReadOnly.blocks_writes());
    assert!(SandboxMode::Strict.blocks_writes());
    assert!(!SandboxMode::External.blocks_writes());
}

// ── should_sandbox_command ──

#[test]
fn test_sandbox_disabled() {
    let config = SandboxConfig::default(); // mode = None
    let decision = should_sandbox_command(&config, "rm -rf /", BypassRequest::No);
    assert!(!decision.is_sandboxed());
}

#[test]
fn test_sandbox_active_command_sandboxed() {
    let config = default_config();
    let decision = should_sandbox_command(&config, "cargo test", BypassRequest::No);
    assert!(decision.is_sandboxed());
    assert_eq!(
        decision,
        SandboxDecision::Sandboxed {
            mode: SandboxMode::Strict
        }
    );
}

#[test]
fn test_bypass_requested_and_allowed() {
    let config = default_config();
    let decision = should_sandbox_command(&config, "cargo test", BypassRequest::Requested);
    assert!(!decision.is_sandboxed());
}

#[test]
fn test_bypass_requested_but_disallowed() {
    let config = SandboxConfig {
        allow_bypass: false,
        ..default_config()
    };
    let decision = should_sandbox_command(&config, "cargo test", BypassRequest::Requested);
    assert!(decision.is_sandboxed());
}

#[test]
fn test_empty_command_unsandboxed() {
    let config = default_config();
    let decision = should_sandbox_command(&config, "   ", BypassRequest::No);
    assert!(!decision.is_sandboxed());
}

#[test]
fn test_excluded_exact_command() {
    let config = default_config();
    let decision = should_sandbox_command(&config, "git status", BypassRequest::No);
    assert!(!decision.is_sandboxed());
}

#[test]
fn test_excluded_prefix_command() {
    let config = default_config();
    let decision = should_sandbox_command(&config, "npm install", BypassRequest::No);
    assert!(!decision.is_sandboxed());
}

#[test]
fn test_non_excluded_command() {
    let config = default_config();
    let decision = should_sandbox_command(&config, "rm -rf /tmp", BypassRequest::No);
    assert!(decision.is_sandboxed());
}

// ── get_sandbox_args ──

#[test]
fn test_linux_sandbox_args_basic() {
    let config = SandboxConfig {
        mode: SandboxMode::Strict,
        writable_roots: vec![PathBuf::from("/home/user/project")],
        allow_network: false,
        ..Default::default()
    };
    let args = get_sandbox_args(&config, Platform::Linux, &PathBuf::from("/home/user/cwd"));

    assert!(args.contains(&"bwrap".to_string()));
    assert!(args.contains(&"--ro-bind".to_string()));
    assert!(args.contains(&"--unshare-net".to_string()));
    assert!(args.contains(&"--die-with-parent".to_string()));
    // Writable root should be present
    assert!(args.contains(&"/home/user/project".to_string()));
}

#[test]
fn test_linux_sandbox_args_with_network() {
    let config = SandboxConfig {
        mode: SandboxMode::Strict,
        allow_network: true,
        ..Default::default()
    };
    let args = get_sandbox_args(&config, Platform::Linux, &PathBuf::from("/tmp"));

    assert!(!args.contains(&"--unshare-net".to_string()));
}

#[test]
fn test_macos_sandbox_args_basic() {
    let config = SandboxConfig {
        mode: SandboxMode::Strict,
        writable_roots: vec![PathBuf::from("/Users/dev/project")],
        allow_network: false,
        ..Default::default()
    };
    let args = get_sandbox_args(&config, Platform::MacOs, &PathBuf::from("/Users/dev/cwd"));

    assert!(args.contains(&"sandbox-exec".to_string()));
    assert!(args.contains(&"-p".to_string()));
    let profile = &args[2];
    assert!(profile.contains("(deny default)"));
    assert!(profile.contains("/Users/dev/project"));
    assert!(profile.contains("/Users/dev/cwd"));
    assert!(!profile.contains("(allow network*)"));
}

#[test]
fn test_macos_sandbox_args_with_network() {
    let config = SandboxConfig {
        mode: SandboxMode::Strict,
        allow_network: true,
        ..Default::default()
    };
    let args = get_sandbox_args(&config, Platform::MacOs, &PathBuf::from("/tmp/test"));

    let profile = &args[2];
    assert!(profile.contains("(allow network*)"));
}

#[test]
fn test_custom_platform_binary() {
    let config = SandboxConfig {
        mode: SandboxMode::Strict,
        platform_binary: Some("/custom/bwrap".into()),
        ..Default::default()
    };
    let args = get_sandbox_args(&config, Platform::Linux, &PathBuf::from("/tmp"));
    assert_eq!(args[0], "/custom/bwrap");
}
