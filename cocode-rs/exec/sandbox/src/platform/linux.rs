//! Linux sandbox implementation using bubblewrap + in-process seccomp.
//!
//! Wraps commands with `bwrap` for namespace isolation (network, PID, IPC, UTS, user)
//! and applies seccomp BPF filters to block sandbox-escaping syscalls.
//!
//! The seccomp filter is compiled at runtime (via `seccompiler`) and passed
//! to the inner stage using the cocode binary's `--apply-seccomp` arg0
//! dispatch. This eliminates external binary dependencies.

use std::path::PathBuf;

use tracing::info;
use tracing::warn;

use snafu::OptionExt;

use crate::config::EnforcementLevel;
use crate::config::SandboxConfig;
use crate::config::WritableRoot;
use crate::error::Result;
use crate::error::sandbox_error::*;
use crate::platform::SandboxPlatform;
use crate::seccomp;

/// Default paths to search for bubblewrap.
const BWRAP_PATHS: &[&str] = &["/usr/bin/bwrap", "/usr/local/bin/bwrap"];

/// Arg1 flag for the seccomp-apply inner stage dispatch.
pub const APPLY_SECCOMP_ARG1: &str = "--apply-seccomp";

/// Linux bubblewrap sandbox implementation.
pub struct LinuxSandbox;

impl LinuxSandbox {
    /// Find the bubblewrap binary.
    fn find_bwrap() -> Option<PathBuf> {
        BWRAP_PATHS.iter().map(PathBuf::from).find(|p| p.exists())
    }
}

impl SandboxPlatform for LinuxSandbox {
    fn available(&self) -> bool {
        cfg!(target_os = "linux") && Self::find_bwrap().is_some()
    }

    fn wrap_command(
        &self,
        config: &SandboxConfig,
        _command: &str,
        _session_tag: &str,
        cmd: &mut tokio::process::Command,
    ) -> Result<()> {
        if config.enforcement == EnforcementLevel::Disabled {
            return Ok(());
        }

        let bwrap_path = Self::find_bwrap().context(PlatformNotAvailableSnafu {
            message: "bubblewrap (bwrap) not found",
        })?;

        let bwrap_args = build_bwrap_args(config);
        let seccomp_mode =
            seccomp::determine_seccomp_mode(config.allow_network, config.proxy_active);

        info!(
            enforcement = ?config.enforcement,
            writable_roots = config.writable_roots.len(),
            allow_network = config.allow_network,
            bwrap_args_count = bwrap_args.len(),
            seccomp_mode = ?seccomp_mode,
            "Wrapping command with Linux bubblewrap sandbox"
        );

        // Extract current program and args
        let inner = cmd.as_std();
        let program = inner.get_program().to_os_string();
        let args: Vec<_> = inner
            .get_args()
            .map(std::ffi::OsStr::to_os_string)
            .collect();

        // Two-stage sandbox pattern:
        //   Stage 1 (outer): bwrap provides namespace isolation
        //   Stage 2 (inner): in-process seccomp via arg0 dispatch
        //
        // Without seccomp:  bwrap [args] -- <program> <args>
        // With seccomp:     bwrap [args] -- <cocode> --apply-seccomp <mode> -- <program> <args>
        *cmd = tokio::process::Command::new(&bwrap_path);
        for arg in &bwrap_args {
            cmd.arg(arg);
        }
        cmd.arg("--");

        if let Some(mode) = seccomp_mode {
            // Binary is visible inside bwrap via the read-only root bind.
            let cocode_exe = std::env::current_exe().ok().unwrap_or_else(|| {
                // Fallback: search PATH for the binary
                PathBuf::from("cocode")
            });
            cmd.arg(&cocode_exe);
            cmd.arg(APPLY_SECCOMP_ARG1);
            cmd.arg(mode.as_str_arg());
            cmd.arg("--");
        }

        cmd.arg(&program);
        cmd.args(&args);

        Ok(())
    }
}

/// Build bubblewrap arguments from the sandbox configuration.
fn build_bwrap_args(config: &SandboxConfig) -> Vec<String> {
    let mut args = Vec::new();

    // Safety flags (from codex-rs):
    // --new-session: isolate from process group (prevents TIOCSTI escape)
    // --die-with-parent: kill sandbox if parent exits (prevents orphans)
    args.extend_from_slice(&["--new-session".into(), "--die-with-parent".into()]);

    // Namespace isolation
    // --unshare-user: user namespace (required for uid 0 mapping, prevents priv-esc)
    args.push("--unshare-user".into());
    if !config.allow_network {
        args.push("--unshare-net".into());
    }
    args.extend_from_slice(&[
        "--unshare-pid".into(),
        "--unshare-ipc".into(),
        "--unshare-uts".into(),
    ]);

    // Base filesystem: read-only bind of the entire root
    args.extend_from_slice(&["--ro-bind".into(), "/".into(), "/".into()]);

    // Minimal device and process filesystems
    args.extend_from_slice(&[
        "--dev".into(),
        "/dev".into(),
        "--proc".into(),
        "/proc".into(),
        "--tmpfs".into(),
        "/tmp".into(),
    ]);

    // Writable roots
    for root in &config.writable_roots {
        let root_path = root.path.display().to_string();
        args.extend_from_slice(&["--bind".into(), root_path.clone(), root_path.clone()]);
        // Re-apply read-only for protected subpaths
        for sub in &root.readonly_subpaths {
            let sub_path = root.path.join(sub);
            let sub_str = sub_path.display().to_string();
            args.extend_from_slice(&["--ro-bind-try".into(), sub_str.clone(), sub_str]);
        }
    }

    // Symlink attack prevention: mask dangerous symlinks with /dev/null
    for root in &config.writable_roots {
        for symlink_path in find_attack_symlinks(root) {
            warn!(
                symlink = %symlink_path.display(),
                root = %root.path.display(),
                "Masking symlink in protected subpath to prevent escape attack"
            );
            let path_str = symlink_path.display().to_string();
            args.extend(["--ro-bind".to_string(), "/dev/null".to_string(), path_str]);
        }
    }

    // Extra read-only bind mounts (e.g., proxy bridge UDS sockets)
    for path in &config.extra_bind_ro {
        let path_str = path.display().to_string();
        args.extend_from_slice(&["--bind".into(), path_str.clone(), path_str]);
    }

    // Process hardening: clear dangerous env vars
    for var in &["LD_PRELOAD", "LD_LIBRARY_PATH", "LD_AUDIT"] {
        args.extend(["--unsetenv".to_string(), var.to_string()]);
    }

    // Set CWD if writable roots are available (preserves symlink aliases)
    if let Some(first_root) = config.writable_roots.first() {
        let cwd_str = first_root.path.display().to_string();
        args.extend_from_slice(&["--chdir".into(), cwd_str]);
    }

    args
}

/// Find symlinks within protected subpaths that could be used for escape attacks.
///
/// A sandboxed process could replace a file in a writable area with a symlink
/// pointing into a read-only protected subpath. By detecting existing symlinks
/// within those subpaths, we can mask them with `/dev/null` to prevent abuse.
///
/// Returns paths that should be masked with /dev/null.
fn find_attack_symlinks(root: &WritableRoot) -> Vec<PathBuf> {
    let mut symlinks = Vec::new();
    for sub in &root.readonly_subpaths {
        let sub_path = root.path.join(sub);
        if sub_path.is_symlink() {
            symlinks.push(sub_path.clone());
        }
        // Also check immediate children if the subpath is a directory
        if sub_path.is_dir()
            && let Ok(entries) = std::fs::read_dir(&sub_path)
        {
            for entry in entries.flatten() {
                if entry.path().is_symlink() {
                    symlinks.push(entry.path());
                }
            }
        }
    }
    symlinks
}

#[cfg(test)]
#[path = "linux.test.rs"]
mod tests;
