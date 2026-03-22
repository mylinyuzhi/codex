use super::*;

#[test]
fn test_detect_shell_type_simple() {
    assert_eq!(
        detect_shell_type(&PathBuf::from("zsh")),
        Some(ShellType::Zsh)
    );
    assert_eq!(
        detect_shell_type(&PathBuf::from("bash")),
        Some(ShellType::Bash)
    );
    assert_eq!(
        detect_shell_type(&PathBuf::from("pwsh")),
        Some(ShellType::PowerShell)
    );
    assert_eq!(
        detect_shell_type(&PathBuf::from("powershell")),
        Some(ShellType::PowerShell)
    );
    assert_eq!(detect_shell_type(&PathBuf::from("fish")), None);
    assert_eq!(detect_shell_type(&PathBuf::from("other")), None);
}

#[test]
fn test_detect_shell_type_full_path() {
    assert_eq!(
        detect_shell_type(&PathBuf::from("/bin/zsh")),
        Some(ShellType::Zsh)
    );
    assert_eq!(
        detect_shell_type(&PathBuf::from("/bin/bash")),
        Some(ShellType::Bash)
    );
    assert_eq!(
        detect_shell_type(&PathBuf::from("/bin/sh")),
        Some(ShellType::Sh)
    );
    assert_eq!(
        detect_shell_type(&PathBuf::from("/usr/local/bin/pwsh")),
        Some(ShellType::PowerShell)
    );
}

#[test]
fn test_detect_shell_type_with_extension() {
    assert_eq!(
        detect_shell_type(&PathBuf::from("powershell.exe")),
        Some(ShellType::PowerShell)
    );
    assert_eq!(
        detect_shell_type(&PathBuf::from("pwsh.exe")),
        Some(ShellType::PowerShell)
    );
    assert_eq!(
        detect_shell_type(&PathBuf::from("cmd")),
        Some(ShellType::Cmd)
    );
    assert_eq!(
        detect_shell_type(&PathBuf::from("cmd.exe")),
        Some(ShellType::Cmd)
    );
}

#[test]
fn test_shell_name() {
    let shells = [
        (ShellType::Zsh, "zsh"),
        (ShellType::Bash, "bash"),
        (ShellType::Sh, "sh"),
        (ShellType::PowerShell, "powershell"),
        (ShellType::Cmd, "cmd"),
    ];

    for (shell_type, expected_name) in shells {
        let shell = Shell {
            shell_type,
            shell_path: PathBuf::from("/bin/test"),
            shell_snapshot: empty_shell_snapshot_receiver(),
        };
        assert_eq!(shell.name(), expected_name);
    }
}

#[test]
fn test_derive_exec_args_bash() {
    let shell = Shell {
        shell_type: ShellType::Bash,
        shell_path: PathBuf::from("/bin/bash"),
        shell_snapshot: empty_shell_snapshot_receiver(),
    };

    assert_eq!(
        shell.derive_exec_args("echo hello", false),
        vec!["/bin/bash", "-c", "echo hello"]
    );
    assert_eq!(
        shell.derive_exec_args("echo hello", true),
        vec!["/bin/bash", "-lc", "echo hello"]
    );
}

#[test]
fn test_derive_exec_args_zsh() {
    let shell = Shell {
        shell_type: ShellType::Zsh,
        shell_path: PathBuf::from("/bin/zsh"),
        shell_snapshot: empty_shell_snapshot_receiver(),
    };

    assert_eq!(
        shell.derive_exec_args("echo hello", false),
        vec!["/bin/zsh", "-c", "echo hello"]
    );
    assert_eq!(
        shell.derive_exec_args("echo hello", true),
        vec!["/bin/zsh", "-lc", "echo hello"]
    );
}

#[test]
fn test_derive_exec_args_powershell() {
    let shell = Shell {
        shell_type: ShellType::PowerShell,
        shell_path: PathBuf::from("pwsh.exe"),
        shell_snapshot: empty_shell_snapshot_receiver(),
    };

    assert_eq!(
        shell.derive_exec_args("echo hello", false),
        vec!["pwsh.exe", "-NoProfile", "-Command", "echo hello"]
    );
    assert_eq!(
        shell.derive_exec_args("echo hello", true),
        vec!["pwsh.exe", "-Command", "echo hello"]
    );
}

#[test]
fn test_derive_exec_args_cmd() {
    let shell = Shell {
        shell_type: ShellType::Cmd,
        shell_path: PathBuf::from("cmd.exe"),
        shell_snapshot: empty_shell_snapshot_receiver(),
    };

    assert_eq!(
        shell.derive_exec_args("echo hello", false),
        vec!["cmd.exe", "/c", "echo hello"]
    );
}

#[test]
fn test_shell_equality() {
    let shell1 = Shell {
        shell_type: ShellType::Bash,
        shell_path: PathBuf::from("/bin/bash"),
        shell_snapshot: empty_shell_snapshot_receiver(),
    };
    let shell2 = Shell {
        shell_type: ShellType::Bash,
        shell_path: PathBuf::from("/bin/bash"),
        shell_snapshot: empty_shell_snapshot_receiver(),
    };
    let shell3 = Shell {
        shell_type: ShellType::Zsh,
        shell_path: PathBuf::from("/bin/zsh"),
        shell_snapshot: empty_shell_snapshot_receiver(),
    };

    assert_eq!(shell1, shell2);
    assert_ne!(shell1, shell3);
}

#[cfg(unix)]
#[test]
fn test_get_shell_bash() {
    let shell = get_shell(ShellType::Bash, None);
    assert!(shell.is_some());
    let shell = shell.expect("bash should be available");
    assert_eq!(shell.shell_type, ShellType::Bash);
}

#[cfg(unix)]
#[test]
fn test_get_shell_sh() {
    let shell = get_shell(ShellType::Sh, None);
    assert!(shell.is_some());
    let shell = shell.expect("sh should be available");
    assert_eq!(shell.shell_type, ShellType::Sh);
}

#[cfg(target_os = "macos")]
#[test]
fn test_get_shell_zsh_macos() {
    let shell = get_shell(ShellType::Zsh, None);
    assert!(shell.is_some());
    let shell = shell.expect("zsh should be available on macOS");
    assert_eq!(shell.shell_type, ShellType::Zsh);
}

#[test]
fn test_default_user_shell() {
    let shell = default_user_shell();
    // Should always return a valid shell
    assert!(!shell.shell_path.as_os_str().is_empty());
}

#[test]
fn test_ultimate_fallback() {
    let shell = ultimate_fallback_shell();
    if cfg!(windows) {
        assert_eq!(shell.shell_type, ShellType::Cmd);
    } else {
        assert_eq!(shell.shell_type, ShellType::Sh);
    }
}
