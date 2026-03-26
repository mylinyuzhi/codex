use crate::config::EnforcementLevel;
use crate::config::SandboxConfig;
use crate::config::WritableRoot;

use super::*;

#[test]
fn test_linux_sandbox_available() {
    let sandbox = LinuxSandbox;
    if cfg!(target_os = "linux") {
        let has_bwrap = BWRAP_PATHS.iter().any(|p| std::path::Path::new(p).exists());
        assert_eq!(sandbox.available(), has_bwrap);
    } else {
        assert!(!sandbox.available());
    }
}

// ==========================================================================
// Safety flags
// ==========================================================================

#[test]
fn test_build_bwrap_args_includes_safety_flags() {
    let config = SandboxConfig {
        enforcement: EnforcementLevel::ReadOnly,
        writable_roots: vec![],
        denied_paths: vec![],
        allow_network: false,
        ..Default::default()
    };

    let args = build_bwrap_args(&config);
    // Critical safety flags from codex-rs
    assert!(args.contains(&"--new-session".to_string()));
    assert!(args.contains(&"--die-with-parent".to_string()));
    assert!(args.contains(&"--unshare-user".to_string()));
}

#[test]
fn test_build_bwrap_args_safety_flags_before_namespace() {
    let config = SandboxConfig {
        enforcement: EnforcementLevel::ReadOnly,
        writable_roots: vec![],
        denied_paths: vec![],
        allow_network: false,
        ..Default::default()
    };

    let args = build_bwrap_args(&config);
    // Safety flags should appear before namespace flags
    let new_session_pos = args
        .iter()
        .position(|a| a == "--new-session")
        .expect("--new-session");
    let unshare_pid_pos = args
        .iter()
        .position(|a| a == "--unshare-pid")
        .expect("--unshare-pid");
    assert!(new_session_pos < unshare_pid_pos);
}

// ==========================================================================
// Namespace isolation
// ==========================================================================

#[test]
fn test_build_bwrap_args_readonly() {
    let config = SandboxConfig {
        enforcement: EnforcementLevel::ReadOnly,
        writable_roots: vec![],
        denied_paths: vec![],
        allow_network: false,
        ..Default::default()
    };

    let args = build_bwrap_args(&config);
    assert!(args.contains(&"--unshare-net".to_string()));
    assert!(args.contains(&"--unshare-pid".to_string()));
    assert!(args.contains(&"--unshare-ipc".to_string()));
    assert!(args.contains(&"--unshare-uts".to_string()));
    assert!(args.contains(&"--unshare-user".to_string()));
    assert!(args.contains(&"--ro-bind".to_string()));
    assert!(args.contains(&"--dev".to_string()));
    assert!(args.contains(&"--proc".to_string()));
    assert!(args.contains(&"--tmpfs".to_string()));
    // No writable roots means no --bind
    assert!(!args.contains(&"--bind".to_string()));
}

#[test]
fn test_build_bwrap_args_network_allowed() {
    let config = SandboxConfig {
        enforcement: EnforcementLevel::WorkspaceWrite,
        writable_roots: vec![],
        denied_paths: vec![],
        allow_network: true,
        ..Default::default()
    };

    let args = build_bwrap_args(&config);
    assert!(!args.contains(&"--unshare-net".to_string()));
    // Still isolate other namespaces
    assert!(args.contains(&"--unshare-pid".to_string()));
    assert!(args.contains(&"--unshare-user".to_string()));
}

// ==========================================================================
// Writable roots
// ==========================================================================

#[test]
fn test_build_bwrap_args_with_writable_roots() {
    let config = SandboxConfig {
        enforcement: EnforcementLevel::WorkspaceWrite,
        writable_roots: vec![WritableRoot::new("/home/user/project")],
        denied_paths: vec![],
        allow_network: false,
        ..Default::default()
    };

    let args = build_bwrap_args(&config);
    let bind_idx = args.iter().position(|a| a == "--bind").expect("--bind");
    assert_eq!(args[bind_idx + 1], "/home/user/project");
    assert_eq!(args[bind_idx + 2], "/home/user/project");

    // Read-only subpath protection
    assert!(args.contains(&"--ro-bind-try".to_string()));
    assert!(args.iter().any(|a| a.contains(".git")));
    assert!(args.iter().any(|a| a.contains(".cocode")));
    assert!(args.iter().any(|a| a.contains(".agents")));

    // CWD set to first writable root
    let chdir_idx = args.iter().position(|a| a == "--chdir").expect("--chdir");
    assert_eq!(args[chdir_idx + 1], "/home/user/project");
}

#[test]
fn test_build_bwrap_args_multiple_writable_roots() {
    let config = SandboxConfig {
        enforcement: EnforcementLevel::WorkspaceWrite,
        writable_roots: vec![
            WritableRoot::new("/home/user/project1"),
            WritableRoot::new("/home/user/project2"),
        ],
        denied_paths: vec![],
        allow_network: false,
        ..Default::default()
    };

    let args = build_bwrap_args(&config);
    // Both roots should have --bind
    let bind_positions: Vec<_> = args
        .iter()
        .enumerate()
        .filter(|(_, a)| *a == "--bind")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(bind_positions.len(), 2);
    assert_eq!(args[bind_positions[0] + 1], "/home/user/project1");
    assert_eq!(args[bind_positions[1] + 1], "/home/user/project2");

    // CWD should be first root
    let chdir_idx = args.iter().position(|a| a == "--chdir").expect("--chdir");
    assert_eq!(args[chdir_idx + 1], "/home/user/project1");
}

#[test]
fn test_build_bwrap_args_no_chdir_without_roots() {
    let config = SandboxConfig {
        enforcement: EnforcementLevel::ReadOnly,
        writable_roots: vec![],
        denied_paths: vec![],
        allow_network: false,
        ..Default::default()
    };

    let args = build_bwrap_args(&config);
    assert!(!args.contains(&"--chdir".to_string()));
}

// ==========================================================================
// Symlink attack prevention
// ==========================================================================

#[test]
fn test_find_attack_symlinks_no_symlinks() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root_path = dir.path().to_path_buf();

    // Create a real .git directory with a regular file
    let git_dir = root_path.join(".git");
    std::fs::create_dir_all(&git_dir).expect("create .git");
    std::fs::write(git_dir.join("config"), "normal file").expect("write file");

    let root = WritableRoot::new(&root_path);
    let symlinks = find_attack_symlinks(&root);
    assert!(symlinks.is_empty());
}

#[test]
fn test_find_attack_symlinks_detects_symlinked_subpath() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root_path = dir.path().to_path_buf();
    let target = dir.path().join("real_target");
    std::fs::create_dir_all(&target).expect("create target");

    // Make .git itself a symlink
    std::os::unix::fs::symlink(&target, root_path.join(".git")).expect("symlink");

    let root = WritableRoot::new(&root_path);
    let symlinks = find_attack_symlinks(&root);
    assert_eq!(symlinks.len(), 1);
    assert_eq!(symlinks[0], root_path.join(".git"));
}

#[test]
fn test_find_attack_symlinks_detects_child_symlinks() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root_path = dir.path().to_path_buf();

    // Create .git with a symlink child
    let git_dir = root_path.join(".git");
    std::fs::create_dir_all(&git_dir).expect("create .git");
    std::fs::write(git_dir.join("normal"), "ok").expect("write");
    let target_file = dir.path().join("escape_target");
    std::fs::write(&target_file, "secret").expect("write target");
    std::os::unix::fs::symlink(&target_file, git_dir.join("sneaky_link")).expect("symlink");

    let root = WritableRoot::new(&root_path);
    let symlinks = find_attack_symlinks(&root);
    assert_eq!(symlinks.len(), 1);
    assert_eq!(symlinks[0], git_dir.join("sneaky_link"));
}

#[test]
fn test_find_attack_symlinks_no_protected_subpaths() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root_path = dir.path().to_path_buf();

    // Unprotected root has no readonly_subpaths to scan
    let root = WritableRoot::unprotected(&root_path);
    let symlinks = find_attack_symlinks(&root);
    assert!(symlinks.is_empty());
}

#[test]
fn test_find_attack_symlinks_nonexistent_subpath() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root_path = dir.path().to_path_buf();

    // Default subpaths (.git, .cocode, .agents) don't exist on disk
    let root = WritableRoot::new(&root_path);
    let symlinks = find_attack_symlinks(&root);
    assert!(symlinks.is_empty());
}

#[test]
fn test_build_bwrap_args_masks_symlinks() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root_path = dir.path().to_path_buf();
    let target = dir.path().join("real_target");
    std::fs::create_dir_all(&target).expect("create target");

    // Make .git a symlink
    std::os::unix::fs::symlink(&target, root_path.join(".git")).expect("symlink");

    let config = SandboxConfig {
        enforcement: EnforcementLevel::WorkspaceWrite,
        writable_roots: vec![WritableRoot::new(&root_path)],
        denied_paths: vec![],
        allow_network: false,
        ..Default::default()
    };

    let args = build_bwrap_args(&config);
    let git_symlink_path = root_path.join(".git").display().to_string();

    // Find the --ro-bind /dev/null <symlink> triple
    let mut found = false;
    for i in 0..args.len().saturating_sub(2) {
        if args[i] == "--ro-bind" && args[i + 1] == "/dev/null" && args[i + 2] == git_symlink_path {
            found = true;
            break;
        }
    }
    assert!(found, "Expected symlink mask not found in args: {args:?}");
}

// ==========================================================================
// Process hardening
// ==========================================================================

#[test]
fn test_build_bwrap_args_unsets_dangerous_env_vars() {
    let config = SandboxConfig {
        enforcement: EnforcementLevel::ReadOnly,
        writable_roots: vec![],
        denied_paths: vec![],
        allow_network: false,
        ..Default::default()
    };

    let args = build_bwrap_args(&config);

    // All three dangerous LD_* vars should be unset
    for var in &["LD_PRELOAD", "LD_LIBRARY_PATH", "LD_AUDIT"] {
        let mut found = false;
        for i in 0..args.len().saturating_sub(1) {
            if args[i] == "--unsetenv" && args[i + 1] == *var {
                found = true;
                break;
            }
        }
        assert!(
            found,
            "Expected --unsetenv {var} not found in args: {args:?}"
        );
    }
}

#[test]
fn test_build_bwrap_args_env_cleanup_before_chdir() {
    let config = SandboxConfig {
        enforcement: EnforcementLevel::WorkspaceWrite,
        writable_roots: vec![WritableRoot::new("/home/user/project")],
        denied_paths: vec![],
        allow_network: false,
        ..Default::default()
    };

    let args = build_bwrap_args(&config);

    // --unsetenv should appear before --chdir
    let unsetenv_pos = args
        .iter()
        .position(|a| a == "--unsetenv")
        .expect("--unsetenv");
    let chdir_pos = args.iter().position(|a| a == "--chdir").expect("--chdir");
    assert!(
        unsetenv_pos < chdir_pos,
        "env cleanup ({unsetenv_pos}) should come before chdir ({chdir_pos})"
    );
}

// ==========================================================================
// Command wrapping
// ==========================================================================

#[test]
fn test_wrap_command_disabled_is_noop() {
    let sandbox = LinuxSandbox;
    let config = SandboxConfig::default();
    let mut cmd = tokio::process::Command::new("echo");
    cmd.arg("hello");

    let result = sandbox.wrap_command(&config, "test command", "_test_SBX", &mut cmd);
    assert!(result.is_ok());
    let inner = cmd.as_std();
    assert_eq!(inner.get_program(), "echo");
}

#[test]
fn test_wrap_command_readonly_rewrites() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let sandbox = LinuxSandbox;
    if !sandbox.available() {
        return; // Skip if bwrap not installed
    }
    let config = SandboxConfig {
        enforcement: EnforcementLevel::ReadOnly,
        writable_roots: vec![],
        denied_paths: vec![],
        allow_network: false,
        ..Default::default()
    };
    let mut cmd = tokio::process::Command::new("/bin/bash");
    cmd.arg("-c").arg("echo hello");

    let result = sandbox.wrap_command(&config, "test command", "_test_SBX", &mut cmd);
    assert!(result.is_ok());

    let inner = cmd.as_std();
    // Program should be bwrap
    let program = inner.get_program().to_string_lossy().to_string();
    assert!(program.contains("bwrap"), "Expected bwrap, got {program}");
    // Args should contain -- separator and original command
    let args: Vec<_> = inner
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();
    assert!(args.contains(&"--".to_string()));
    assert!(args.contains(&"/bin/bash".to_string()));
}

// ==========================================================================
// In-process seccomp mode selection
// ==========================================================================

#[test]
fn test_wrap_command_with_seccomp_inserts_apply_seccomp_inner() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let sandbox = LinuxSandbox;
    if !sandbox.available() {
        return;
    }

    // Network blocked + no proxy → Restricted seccomp mode
    let config = SandboxConfig {
        enforcement: EnforcementLevel::ReadOnly,
        writable_roots: vec![],
        denied_paths: vec![],
        allow_network: false,
        proxy_active: false,
        ..Default::default()
    };

    let mut cmd = tokio::process::Command::new("/bin/echo");
    cmd.arg("hello");

    let result = sandbox.wrap_command(&config, "test command", "_test_SBX", &mut cmd);
    assert!(result.is_ok());

    let inner = cmd.as_std();
    let args: Vec<_> = inner
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();

    // Should contain --apply-seccomp restricted -- in the inner args
    assert!(
        args.contains(&APPLY_SECCOMP_ARG1.to_string()),
        "Expected --apply-seccomp in args: {args:?}"
    );
    assert!(
        args.contains(&"restricted".to_string()),
        "Expected 'restricted' mode in args: {args:?}"
    );

    // Two "--" separators: before seccomp inner, before real command
    let sep_count = args.iter().filter(|a| *a == "--").count();
    assert_eq!(
        sep_count, 2,
        "Expected two '--' separators, got {sep_count} in {args:?}"
    );
}

#[test]
fn test_wrap_command_full_network_skips_seccomp() {
    if !cfg!(target_os = "linux") {
        return;
    }
    let sandbox = LinuxSandbox;
    if !sandbox.available() {
        return;
    }

    // Full network, no proxy → no seccomp
    let config = SandboxConfig {
        enforcement: EnforcementLevel::ReadOnly,
        writable_roots: vec![],
        denied_paths: vec![],
        allow_network: true,
        proxy_active: false,
        ..Default::default()
    };

    let mut cmd = tokio::process::Command::new("/bin/echo");
    cmd.arg("hello");

    let result = sandbox.wrap_command(&config, "test command", "_test_SBX", &mut cmd);
    assert!(result.is_ok());

    let inner = cmd.as_std();
    let args: Vec<_> = inner
        .get_args()
        .map(|a| a.to_string_lossy().to_string())
        .collect();

    // No seccomp: only one "--" separator
    let sep_count = args.iter().filter(|a| *a == "--").count();
    assert_eq!(
        sep_count, 1,
        "Expected one '--' separator without seccomp, got {sep_count} in {args:?}"
    );
    assert!(
        !args.contains(&APPLY_SECCOMP_ARG1.to_string()),
        "Should NOT contain --apply-seccomp: {args:?}"
    );
}
