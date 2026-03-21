use super::*;

#[test]
fn test_unix_sandbox_available() {
    let sandbox = UnixSandbox;
    let expected = cfg!(target_os = "macos") || cfg!(target_os = "linux");
    assert_eq!(sandbox.available(), expected);
}

#[test]
fn test_unix_sandbox_apply() {
    let sandbox = UnixSandbox;
    let config = SandboxConfig::default();
    assert!(sandbox.apply(&config).is_ok());
}
