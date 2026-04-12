//! Shell type detection and configuration.
//!
//! Provides types and functions for detecting the user's default shell,
//! resolving shell paths, and configuring shell execution with optional
//! environment snapshot support.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;
use serde::Serialize;
use tokio::sync::watch;

use crate::snapshot::ShellSnapshot;

/// Supported shell types.
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellType {
    Zsh,
    Bash,
    PowerShell,
    Sh,
    Cmd,
}

/// Shell configuration with path and optional environment snapshot.
#[derive(Debug, Clone)]
pub struct Shell {
    shell_type: ShellType,
    shell_path: PathBuf,
    shell_snapshot: watch::Receiver<Option<Arc<ShellSnapshot>>>,
}

impl Shell {
    /// Returns the shell type.
    pub fn shell_type(&self) -> &ShellType {
        &self.shell_type
    }

    /// Returns the shell binary path.
    pub fn shell_path(&self) -> &Path {
        &self.shell_path
    }

    /// Returns the short name of the shell.
    pub fn name(&self) -> &'static str {
        match self.shell_type {
            ShellType::Zsh => "zsh",
            ShellType::Bash => "bash",
            ShellType::PowerShell => "powershell",
            ShellType::Sh => "sh",
            ShellType::Cmd => "cmd",
        }
    }

    /// Derives the command arguments for executing a shell script.
    pub fn derive_exec_args(&self, command: &str, use_login_shell: bool) -> Vec<String> {
        let shell = self.shell_path.to_string_lossy().to_string();
        match self.shell_type {
            ShellType::Zsh | ShellType::Bash | ShellType::Sh => {
                let flag = if use_login_shell { "-lc" } else { "-c" };
                vec![shell, flag.to_string(), command.to_string()]
            }
            ShellType::PowerShell => {
                let mut args = vec![shell];
                if !use_login_shell {
                    args.push("-NoProfile".to_string());
                }
                args.push("-Command".to_string());
                args.push(command.to_string());
                args
            }
            ShellType::Cmd => {
                vec![shell, "/c".to_string(), command.to_string()]
            }
        }
    }

    /// Returns the current shell snapshot if available.
    pub fn shell_snapshot(&self) -> Option<Arc<ShellSnapshot>> {
        self.shell_snapshot.borrow().clone()
    }

    /// Sets the shell snapshot receiver for async snapshot updates.
    pub fn set_shell_snapshot_receiver(
        &mut self,
        receiver: watch::Receiver<Option<Arc<ShellSnapshot>>>,
    ) {
        self.shell_snapshot = receiver;
    }
}

impl PartialEq for Shell {
    fn eq(&self, other: &Self) -> bool {
        self.shell_type == other.shell_type && self.shell_path == other.shell_path
    }
}

impl Eq for Shell {}

/// Creates a watch receiver that always returns `None`.
pub fn empty_shell_snapshot_receiver() -> watch::Receiver<Option<Arc<ShellSnapshot>>> {
    let (_tx, rx) = watch::channel(None);
    rx
}

/// Detects the shell type from a binary path.
pub fn detect_shell_type(shell_path: &Path) -> Option<ShellType> {
    let name = shell_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();

    match name {
        "zsh" => Some(ShellType::Zsh),
        "bash" => Some(ShellType::Bash),
        "sh" => Some(ShellType::Sh),
        "pwsh" | "powershell" => Some(ShellType::PowerShell),
        "cmd" => Some(ShellType::Cmd),
        _ => None,
    }
}

/// Returns the user's default shell.
///
/// Priority (TS alignment — `Shell.ts:findSuitableShell`):
/// 1. `$COCO_SHELL` override (must be bash or zsh, must be executable)
/// 2. `$SHELL` environment variable
/// 3. Platform-specific discovery (which + fallback paths)
pub fn default_user_shell() -> Shell {
    // TS: CLAUDE_CODE_SHELL override, only bash/zsh accepted
    if let Ok(override_path) = std::env::var("COCO_SHELL") {
        let path = PathBuf::from(&override_path);
        if let Some(st) = detect_shell_type(&path)
            && matches!(st, ShellType::Bash | ShellType::Zsh)
            && let Some(shell) = get_shell(st, Some(&path))
        {
            return shell;
        }
        tracing::warn!("COCO_SHELL={override_path} is not a supported shell (bash/zsh), ignoring");
    }

    let user_shell = std::env::var("SHELL").ok().map(PathBuf::from);
    default_user_shell_from_path(user_shell)
}

fn default_user_shell_from_path(user_shell_path: Option<PathBuf>) -> Shell {
    if cfg!(windows) {
        return get_shell(ShellType::PowerShell, None).unwrap_or_else(ultimate_fallback_shell);
    }

    let user_default = user_shell_path
        .as_deref()
        .and_then(detect_shell_type)
        .and_then(|st| get_shell(st, user_shell_path.as_deref()));

    let with_fallback = if cfg!(target_os = "macos") {
        user_default
            .or_else(|| get_shell(ShellType::Zsh, None))
            .or_else(|| get_shell(ShellType::Bash, None))
    } else {
        user_default
            .or_else(|| get_shell(ShellType::Bash, None))
            .or_else(|| get_shell(ShellType::Zsh, None))
    };

    with_fallback.unwrap_or_else(ultimate_fallback_shell)
}

/// Gets a shell of the specified type, optionally at a specific path.
pub fn get_shell(shell_type: ShellType, path: Option<&Path>) -> Option<Shell> {
    let binary_name = match shell_type {
        ShellType::Zsh => "zsh",
        ShellType::Bash => "bash",
        ShellType::Sh => "sh",
        ShellType::PowerShell => "pwsh",
        ShellType::Cmd => "cmd",
    };

    let fallbacks: &[&str] = match shell_type {
        ShellType::Zsh => &["/bin/zsh"],
        ShellType::Bash => &["/bin/bash"],
        ShellType::Sh => &["/bin/sh"],
        ShellType::PowerShell => &["/usr/local/bin/pwsh"],
        ShellType::Cmd => &[],
    };

    let resolved = resolve_shell_path(path, binary_name, fallbacks)?;

    Some(Shell {
        shell_type,
        shell_path: resolved,
        shell_snapshot: empty_shell_snapshot_receiver(),
    })
}

/// Gets a shell by its binary path, detecting the type automatically.
pub fn get_shell_by_path(shell_path: &Path) -> Shell {
    detect_shell_type(shell_path)
        .and_then(|st| get_shell(st, Some(shell_path)))
        .unwrap_or_else(ultimate_fallback_shell)
}

fn resolve_shell_path(
    provided: Option<&Path>,
    binary_name: &str,
    fallbacks: &[&str],
) -> Option<PathBuf> {
    // Exact provided path
    if let Some(path) = provided
        && path.is_file()
    {
        return Some(path.to_path_buf());
    }

    // Try `which`
    if let Ok(path) = which::which(binary_name) {
        return Some(path);
    }

    // Try fallback paths
    for path in fallbacks {
        let p = Path::new(path);
        if p.is_file() {
            return Some(p.to_path_buf());
        }
    }

    None
}

fn ultimate_fallback_shell() -> Shell {
    if cfg!(windows) {
        Shell {
            shell_type: ShellType::Cmd,
            shell_path: PathBuf::from("cmd.exe"),
            shell_snapshot: empty_shell_snapshot_receiver(),
        }
    } else {
        Shell {
            shell_type: ShellType::Sh,
            shell_path: PathBuf::from("/bin/sh"),
            shell_snapshot: empty_shell_snapshot_receiver(),
        }
    }
}

#[cfg(test)]
#[path = "shell_types.test.rs"]
mod tests;
