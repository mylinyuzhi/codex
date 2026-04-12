use std::path::Path;
use std::path::PathBuf;

use super::*;
use crate::config::EnforcementLevel;
use crate::config::SandboxConfig;
use crate::config::WritableRoot;

fn strict_config() -> SandboxConfig {
    SandboxConfig {
        enforcement: EnforcementLevel::Strict,
        writable_roots: vec![WritableRoot::new("/home/user/project")],
        denied_paths: vec![PathBuf::from("/home/user/project/.env")],
        allow_network: false,
        ..Default::default()
    }
}

fn readonly_config() -> SandboxConfig {
    SandboxConfig {
        enforcement: EnforcementLevel::ReadOnly,
        ..Default::default()
    }
}

fn workspace_write_config() -> SandboxConfig {
    SandboxConfig {
        enforcement: EnforcementLevel::WorkspaceWrite,
        writable_roots: vec![WritableRoot::new("/home/user/project")],
        allow_network: true,
        ..Default::default()
    }
}

fn disabled_config() -> SandboxConfig {
    SandboxConfig::default()
}

// ==========================================================================
// Disabled mode
// ==========================================================================

#[test]
fn test_disabled_mode_allows_everything() {
    let checker = PermissionChecker::new(disabled_config());
    assert!(checker.check_path(Path::new("/any/path"), false).is_ok());
    assert!(checker.check_path(Path::new("/any/path"), true).is_ok());
    assert!(checker.check_network().is_ok());
}

// ==========================================================================
// ReadOnly mode
// ==========================================================================

#[test]
fn test_readonly_allows_reads() {
    let checker = PermissionChecker::new(readonly_config());
    assert!(checker.check_path(Path::new("/any/path"), false).is_ok());
}

#[test]
fn test_readonly_denies_writes() {
    let checker = PermissionChecker::new(readonly_config());
    assert!(checker.check_path(Path::new("/any/path"), true).is_err());
}

#[test]
fn test_readonly_denies_network() {
    let checker = PermissionChecker::new(readonly_config());
    assert!(checker.check_network().is_err());
}

// ==========================================================================
// WorkspaceWrite mode
// ==========================================================================

#[test]
fn test_workspace_write_allows_reads_anywhere() {
    let checker = PermissionChecker::new(workspace_write_config());
    assert!(checker.check_path(Path::new("/etc/hosts"), false).is_ok());
    assert!(
        checker
            .check_path(Path::new("/home/user/project/src"), false)
            .is_ok()
    );
}

#[test]
fn test_workspace_write_allows_writes_to_roots() {
    let checker = PermissionChecker::new(workspace_write_config());
    assert!(
        checker
            .check_path(Path::new("/home/user/project/src/main.rs"), true)
            .is_ok()
    );
}

#[test]
fn test_workspace_write_denies_writes_outside_roots() {
    let checker = PermissionChecker::new(workspace_write_config());
    assert!(checker.check_path(Path::new("/etc/passwd"), true).is_err());
}

#[test]
fn test_workspace_write_protects_git_subpath() {
    let checker = PermissionChecker::new(workspace_write_config());
    assert!(
        checker
            .check_path(Path::new("/home/user/project/.git/config"), true)
            .is_err()
    );
    assert!(
        checker
            .check_path(Path::new("/home/user/project/.git"), true)
            .is_err()
    );
}

#[test]
fn test_workspace_write_protects_coco_subpath() {
    let checker = PermissionChecker::new(workspace_write_config());
    assert!(
        checker
            .check_path(Path::new("/home/user/project/.coco/config.json"), true)
            .is_err()
    );
}

#[test]
fn test_workspace_write_allows_network_when_configured() {
    let checker = PermissionChecker::new(workspace_write_config());
    assert!(checker.check_network().is_ok());
}

// ==========================================================================
// Strict mode
// ==========================================================================

#[test]
fn test_strict_allows_read_under_root() {
    let checker = PermissionChecker::new(strict_config());
    assert!(
        checker
            .check_path(Path::new("/home/user/project/src/main.rs"), false)
            .is_ok()
    );
}

#[test]
fn test_strict_denies_non_root_path() {
    let checker = PermissionChecker::new(strict_config());
    assert!(checker.check_path(Path::new("/etc/passwd"), false).is_err());
}

#[test]
fn test_strict_denied_path_takes_precedence() {
    let checker = PermissionChecker::new(strict_config());
    assert!(
        checker
            .check_path(Path::new("/home/user/project/.env"), false)
            .is_err()
    );
}

#[test]
fn test_strict_denies_network_by_default() {
    let checker = PermissionChecker::new(strict_config());
    assert!(checker.check_network().is_err());
}

#[test]
fn test_strict_allows_network_when_configured() {
    let mut config = strict_config();
    config.allow_network = true;
    let checker = PermissionChecker::new(config);
    assert!(checker.check_network().is_ok());
}

#[test]
fn test_strict_write_to_allowed_path() {
    let checker = PermissionChecker::new(strict_config());
    assert!(
        checker
            .check_path(Path::new("/home/user/project/src/main.rs"), true)
            .is_ok()
    );
}

#[test]
fn test_strict_write_to_denied_path() {
    let checker = PermissionChecker::new(strict_config());
    assert!(
        checker
            .check_path(Path::new("/home/user/project/.env"), true)
            .is_err()
    );
}

#[test]
fn test_strict_write_to_git_subpath_denied() {
    let checker = PermissionChecker::new(strict_config());
    assert!(
        checker
            .check_path(Path::new("/home/user/project/.git/config"), true)
            .is_err()
    );
}

// ==========================================================================
// Helper methods
// ==========================================================================

#[test]
fn test_is_writable_path() {
    let checker = PermissionChecker::new(workspace_write_config());
    assert!(checker.is_writable_path(Path::new("/home/user/project/src")));
    assert!(!checker.is_writable_path(Path::new("/home/user/project/.git")));
    assert!(!checker.is_writable_path(Path::new("/etc/hosts")));
}

#[test]
fn test_is_under_any_root() {
    let checker = PermissionChecker::new(strict_config());
    assert!(checker.is_under_any_root(Path::new("/home/user/project/src")));
    assert!(checker.is_under_any_root(Path::new("/home/user/project/.git")));
    assert!(!checker.is_under_any_root(Path::new("/etc/hosts")));
}

#[test]
fn test_is_under_any_root_empty_disabled() {
    let checker = PermissionChecker::new(disabled_config());
    // Disabled mode with no roots: returns true (everything allowed)
    assert!(checker.is_under_any_root(Path::new("/anything")));
}

#[test]
fn test_is_under_any_root_empty_strict() {
    let config = SandboxConfig {
        enforcement: EnforcementLevel::Strict,
        writable_roots: vec![],
        ..Default::default()
    };
    let checker = PermissionChecker::new(config);
    // Strict mode with no roots: nothing is under any root
    assert!(!checker.is_under_any_root(Path::new("/anything")));
}

#[test]
fn test_config_accessor() {
    let config = strict_config();
    let checker = PermissionChecker::new(config);
    assert_eq!(checker.config().enforcement, EnforcementLevel::Strict);
    assert_eq!(checker.config().writable_roots.len(), 1);
}

// ==========================================================================
// Deny-read paths
// ==========================================================================

#[test]
fn test_deny_read_blocks_read_in_workspace_write() {
    let config = SandboxConfig {
        enforcement: EnforcementLevel::WorkspaceWrite,
        writable_roots: vec![WritableRoot::new("/home/user/project")],
        denied_read_paths: vec![PathBuf::from("/etc/shadow")],
        ..Default::default()
    };
    let checker = PermissionChecker::new(config);
    assert!(
        checker
            .check_path(Path::new("/etc/shadow"), /*write*/ false)
            .is_err()
    );
    // Non-denied read is fine
    assert!(
        checker
            .check_path(Path::new("/etc/hosts"), /*write*/ false)
            .is_ok()
    );
}

#[test]
fn test_deny_read_not_applied_in_disabled() {
    let config = SandboxConfig {
        enforcement: EnforcementLevel::Disabled,
        denied_read_paths: vec![PathBuf::from("/etc/shadow")],
        ..Default::default()
    };
    let checker = PermissionChecker::new(config);
    // Disabled mode: deny-read is not enforced
    assert!(
        checker
            .check_path(Path::new("/etc/shadow"), /*write*/ false)
            .is_ok()
    );
}

// ==========================================================================
// Deny-write paths
// ==========================================================================

#[test]
fn test_deny_write_blocks_write() {
    let config = SandboxConfig {
        enforcement: EnforcementLevel::WorkspaceWrite,
        writable_roots: vec![WritableRoot::new("/home/user/project")],
        deny_write_paths: vec![PathBuf::from("/home/user/project/protected")],
        ..Default::default()
    };
    let checker = PermissionChecker::new(config);
    // Write to protected path denied even though it's under writable root
    assert!(
        checker
            .check_path(
                Path::new("/home/user/project/protected/file"),
                /*write*/ true
            )
            .is_err()
    );
    // Write to non-protected path is fine
    assert!(
        checker
            .check_path(
                Path::new("/home/user/project/src/main.rs"),
                /*write*/ true
            )
            .is_ok()
    );
}

// ==========================================================================
// Git config protection
// ==========================================================================

#[test]
fn test_git_config_blocked_by_default() {
    let config = SandboxConfig {
        enforcement: EnforcementLevel::WorkspaceWrite,
        writable_roots: vec![WritableRoot::unprotected("/home/user/project")],
        allow_git_config: false,
        ..Default::default()
    };
    let checker = PermissionChecker::new(config);
    assert!(
        checker
            .check_path(
                Path::new("/home/user/project/.git/config"),
                /*write*/ true
            )
            .is_err()
    );
    assert!(
        checker
            .check_path(Path::new("/home/user/.gitconfig"), /*write*/ true)
            .is_err()
    );
}

#[test]
fn test_git_config_allowed_when_enabled() {
    let config = SandboxConfig {
        enforcement: EnforcementLevel::WorkspaceWrite,
        writable_roots: vec![WritableRoot::unprotected("/home/user/project")],
        allow_git_config: true,
        ..Default::default()
    };
    let checker = PermissionChecker::new(config);
    assert!(
        checker
            .check_path(
                Path::new("/home/user/project/.git/config"),
                /*write*/ true
            )
            .is_ok()
    );
}
