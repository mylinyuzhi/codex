use super::*;

#[test]
fn test_windows_sandbox_available() {
    let sandbox = WindowsSandbox;
    let expected = cfg!(target_os = "windows");
    assert_eq!(sandbox.available(), expected);
}

#[test]
fn test_windows_sandbox_apply() {
    let sandbox = WindowsSandbox;
    let config = SandboxConfig::default();
    assert!(sandbox.apply(&config).is_ok());
}
