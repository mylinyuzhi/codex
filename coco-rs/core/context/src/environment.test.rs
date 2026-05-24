use crate::environment::Platform;
use crate::environment::ShellKind;
use crate::environment::get_environment_info;

#[test]
fn test_platform_current() {
    let p = Platform::current();
    assert!(matches!(
        p,
        Platform::Linux | Platform::Darwin | Platform::Windows
    ));
}

#[test]
fn test_shell_detect() {
    let s = ShellKind::detect();
    // Should return some valid shell
    assert!(matches!(
        s,
        ShellKind::Bash | ShellKind::Zsh | ShellKind::Sh | ShellKind::PowerShell
    ));
}

#[test]
fn test_get_environment_info() {
    let info = get_environment_info(std::path::Path::new("/tmp"), "test-model");
    assert_eq!(info.cwd, "/tmp");
    assert_eq!(info.model, "test-model");
    assert!(!info.is_git_repo);
}
