use super::*;
use pretty_assertions::assert_eq;
use std::path::Path;

#[test]
fn test_detect_shell_type_from_full_path() {
    assert_eq!(
        detect_shell_type(Path::new("/bin/bash")),
        Some(ShellType::Bash)
    );
    assert_eq!(
        detect_shell_type(Path::new("/usr/bin/zsh")),
        Some(ShellType::Zsh)
    );
    assert_eq!(detect_shell_type(Path::new("/bin/sh")), Some(ShellType::Sh));
}

#[test]
fn test_detect_shell_type_from_name() {
    assert_eq!(detect_shell_type(Path::new("bash")), Some(ShellType::Bash));
    assert_eq!(
        detect_shell_type(Path::new("pwsh")),
        Some(ShellType::PowerShell)
    );
}

#[test]
fn test_detect_shell_type_unknown() {
    assert_eq!(detect_shell_type(Path::new("/usr/bin/fish")), None);
    assert_eq!(detect_shell_type(Path::new("unknown")), None);
}

#[test]
fn test_shell_name() {
    let shell = Shell {
        shell_type: ShellType::Bash,
        shell_path: PathBuf::from("/bin/bash"),
        shell_snapshot: empty_shell_snapshot_receiver(),
    };
    assert_eq!(shell.name(), "bash");
}

#[test]
fn test_shell_derive_exec_args_login() {
    let shell = Shell {
        shell_type: ShellType::Bash,
        shell_path: PathBuf::from("/bin/bash"),
        shell_snapshot: empty_shell_snapshot_receiver(),
    };
    let args = shell.derive_exec_args("echo hi", true);
    assert_eq!(args, vec!["/bin/bash", "-lc", "echo hi"]);
}

#[test]
fn test_shell_derive_exec_args_no_login() {
    let shell = Shell {
        shell_type: ShellType::Zsh,
        shell_path: PathBuf::from("/bin/zsh"),
        shell_snapshot: empty_shell_snapshot_receiver(),
    };
    let args = shell.derive_exec_args("ls", false);
    assert_eq!(args, vec!["/bin/zsh", "-c", "ls"]);
}

#[test]
fn test_shell_derive_exec_args_powershell() {
    let shell = Shell {
        shell_type: ShellType::PowerShell,
        shell_path: PathBuf::from("pwsh"),
        shell_snapshot: empty_shell_snapshot_receiver(),
    };
    let login_args = shell.derive_exec_args("Get-Date", true);
    assert_eq!(login_args, vec!["pwsh", "-Command", "Get-Date"]);

    let no_login_args = shell.derive_exec_args("Get-Date", false);
    assert_eq!(
        no_login_args,
        vec!["pwsh", "-NoProfile", "-Command", "Get-Date"]
    );
}

#[test]
fn test_shell_equality_ignores_snapshot() {
    let a = Shell {
        shell_type: ShellType::Bash,
        shell_path: PathBuf::from("/bin/bash"),
        shell_snapshot: empty_shell_snapshot_receiver(),
    };
    let b = Shell {
        shell_type: ShellType::Bash,
        shell_path: PathBuf::from("/bin/bash"),
        shell_snapshot: empty_shell_snapshot_receiver(),
    };
    assert_eq!(a, b);
}

#[test]
fn test_default_user_shell_from_path_bash() {
    let shell = default_user_shell_from_path(Some(PathBuf::from("/bin/bash")));
    assert_eq!(shell.shell_type, ShellType::Bash);
}

#[cfg(unix)]
#[test]
fn test_default_user_shell_returns_valid() {
    let shell = default_user_shell();
    assert!(
        matches!(
            shell.shell_type,
            ShellType::Bash | ShellType::Zsh | ShellType::Sh
        ),
        "expected unix shell, got {:?}",
        shell.shell_type
    );
}

#[test]
fn test_empty_snapshot_receiver_returns_none() {
    let rx = empty_shell_snapshot_receiver();
    assert!(rx.borrow().is_none());
}
