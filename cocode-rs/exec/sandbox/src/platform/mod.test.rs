use super::*;

#[test]
fn test_platform_sandbox_available() {
    let sandbox = platform_sandbox();
    // On any supported platform, this should return true
    assert!(sandbox.available());
}

#[test]
fn test_platform_sandbox_apply_none_mode() {
    let sandbox = platform_sandbox();
    let config = SandboxConfig::default();
    // Applying a no-op sandbox should succeed
    assert!(sandbox.apply(&config).is_ok());
}
