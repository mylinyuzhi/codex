use super::*;
use std::path::PathBuf;

fn strict_config() -> SandboxConfig {
    SandboxConfig {
        mode: SandboxMode::Strict,
        allowed_paths: vec![PathBuf::from("/home/user/project")],
        denied_paths: vec![PathBuf::from("/home/user/project/.env")],
        allow_network: false,
    }
}

fn readonly_config() -> SandboxConfig {
    SandboxConfig {
        mode: SandboxMode::ReadOnly,
        allowed_paths: vec![],
        denied_paths: vec![],
        allow_network: false,
    }
}

fn none_config() -> SandboxConfig {
    SandboxConfig::default()
}

#[test]
fn test_none_mode_allows_everything() {
    let checker = PermissionChecker::new(none_config());
    assert!(checker.check_path(Path::new("/any/path"), false).is_ok());
    assert!(checker.check_path(Path::new("/any/path"), true).is_ok());
    assert!(checker.check_network().is_ok());
}

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

#[test]
fn test_strict_allows_allowed_path() {
    let checker = PermissionChecker::new(strict_config());
    assert!(
        checker
            .check_path(Path::new("/home/user/project/src/main.rs"), false)
            .is_ok()
    );
}

#[test]
fn test_strict_denies_non_allowed_path() {
    let checker = PermissionChecker::new(strict_config());
    assert!(checker.check_path(Path::new("/etc/passwd"), false).is_err());
}

#[test]
fn test_strict_denied_path_takes_precedence() {
    let checker = PermissionChecker::new(strict_config());
    // .env is under the allowed project path but explicitly denied
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
fn test_is_allowed_path_empty_none_mode() {
    let checker = PermissionChecker::new(none_config());
    // No allowed_paths configured, but mode is None so everything is allowed
    assert!(checker.is_allowed_path(Path::new("/anything")));
}

#[test]
fn test_is_allowed_path_empty_strict_mode() {
    let config = SandboxConfig {
        mode: SandboxMode::Strict,
        allowed_paths: vec![],
        denied_paths: vec![],
        allow_network: false,
    };
    let checker = PermissionChecker::new(config);
    // No allowed_paths in strict mode means nothing is allowed
    assert!(!checker.is_allowed_path(Path::new("/anything")));
}

#[test]
fn test_is_allowed_path_prefix_match() {
    let checker = PermissionChecker::new(strict_config());
    assert!(checker.is_allowed_path(Path::new("/home/user/project")));
    assert!(checker.is_allowed_path(Path::new("/home/user/project/src")));
    assert!(checker.is_allowed_path(Path::new("/home/user/project/src/lib.rs")));
    assert!(!checker.is_allowed_path(Path::new("/home/user/other")));
}

#[test]
fn test_config_accessor() {
    let config = strict_config();
    let checker = PermissionChecker::new(config);
    assert_eq!(checker.config().mode, SandboxMode::Strict);
    assert_eq!(checker.config().allowed_paths.len(), 1);
}

#[test]
fn test_strict_write_to_allowed_path() {
    let checker = PermissionChecker::new(strict_config());
    // Write to an allowed path (not denied) should succeed in strict mode
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
