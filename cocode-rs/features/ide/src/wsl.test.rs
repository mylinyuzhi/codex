use super::*;

#[test]
fn test_is_wsl_returns_bool() {
    // On non-WSL CI/dev machines this returns false.
    // On WSL it returns true. Either way, it must not panic.
    let _ = is_wsl();
}
