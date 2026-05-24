//! IDE lockfile discovery and parsing.
//!
//! IDE extensions write lockfiles to `~/.claude/ide/<PORT>.lock` containing
//! connection metadata. This module scans, validates, and selects the best
//! matching lockfile for the current workspace.

use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use tracing::debug;
use tracing::warn;
use unicode_normalization::UnicodeNormalization;

use crate::detection::IdeType;
use crate::detection::ide_for_key;

/// Parsed contents of an IDE lockfile.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IdeLockfile {
    /// Workspace folders the IDE has open.
    #[serde(default)]
    pub workspace_folders: Vec<String>,
    /// PID of the IDE process.
    pub pid: i64,
    /// IDE key matching the detection registry (e.g. "vscode", "cursor").
    pub ide_name: String,
    /// Transport type: "ws" for WebSocket, anything else for SSE.
    #[serde(default)]
    pub transport: String,
    /// Whether the IDE is running on Windows (relevant for WSL path translation).
    #[serde(default)]
    #[allow(dead_code)] // Used when WSL path translation is wired to diff handler
    pub running_in_windows: bool,
    /// Auth token for MCP connection.
    #[serde(default)]
    pub auth_token: String,
}

/// A validated lockfile with its resolved metadata.
#[derive(Debug, Clone)]
pub(crate) struct ResolvedLockfile {
    /// The parsed lockfile contents.
    pub lockfile: IdeLockfile,
    /// The IDE type from the registry.
    pub ide_type: &'static IdeType,
    /// The port number (from filename).
    pub port: i32,
    /// Host address for connection.
    pub host: String,
}

impl ResolvedLockfile {
    /// Build the MCP server URL for this lockfile.
    pub fn mcp_url(&self) -> String {
        if self.is_websocket() {
            format!("ws://{}:{}", self.host, self.port)
        } else {
            format!("http://{}:{}/sse", self.host, self.port)
        }
    }

    /// Whether this uses WebSocket transport.
    pub fn is_websocket(&self) -> bool {
        self.lockfile.transport == "ws"
    }
}

/// Discover and return the best matching IDE lockfile for the given workspace.
///
/// Scans `~/.claude/ide/` for `.lock` files, validates workspace match,
/// and checks process liveness. On WSL, also scans the Windows host user's
/// `.claude/ide/` directory.
pub(crate) async fn discover_ide_lockfile(cwd: &Path) -> Option<ResolvedLockfile> {
    // When an explicit URL override is set, the caller should connect
    // directly instead of using lockfile-based discovery.
    if std::env::var("COCODE_IDE_MCP_URL").is_ok() {
        debug!("COCODE_IDE_MCP_URL is set, skipping lockfile discovery");
        return None;
    }

    // Collect all candidate directories to scan
    let mut dirs_to_scan = vec![ide_lockfile_dir()];

    // On WSL, also scan the Windows host user's .claude/ide/ directory
    if crate::wsl::is_wsl()
        && let Some(win_ide_dir) = wsl_windows_ide_dir().await
    {
        dirs_to_scan.push(win_ide_dir);
    }

    // Remove non-existent directories (async to avoid blocking the executor)
    let mut existing_dirs = Vec::new();
    for dir in dirs_to_scan {
        if tokio::fs::try_exists(&dir).await.unwrap_or(false) {
            existing_dirs.push(dir);
        }
    }
    let dirs_to_scan = existing_dirs;

    if dirs_to_scan.is_empty() {
        debug!("No IDE lockfile directories found");
        return None;
    }

    let skip_valid_check =
        std::env::var("COCODE_IDE_SKIP_VALID_CHECK").is_ok_and(|v| v == "1" || v == "true");

    let host = std::env::var("COCODE_IDE_HOST_OVERRIDE").unwrap_or_else(|_| "127.0.0.1".into());

    let mut candidates = Vec::new();

    for ide_dir in &dirs_to_scan {
        let mut entries = match tokio::fs::read_dir(ide_dir).await {
            Ok(e) => e,
            Err(e) => {
                debug!("Failed to read IDE dir {}: {e}", ide_dir.display());
                continue;
            }
        };

        loop {
            let entry = match entries.next_entry().await {
                Ok(Some(e)) => e,
                Ok(None) => break,
                Err(e) => {
                    debug!("Skipping unreadable lockfile entry: {e}");
                    continue;
                }
            };
            let path = entry.path();

            let ext = path.extension().and_then(|e| e.to_str());
            if ext != Some("lock") {
                continue;
            }

            let port = match path
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.parse::<i32>().ok())
            {
                Some(p) => p,
                None => {
                    debug!("Skipping non-numeric lockfile: {}", path.display());
                    continue;
                }
            };

            let content = match tokio::fs::read_to_string(&path).await {
                Ok(c) => c,
                Err(e) => {
                    warn!("Failed to read lockfile {}: {e}", path.display());
                    continue;
                }
            };

            let lockfile: IdeLockfile = match serde_json::from_str(&content) {
                Ok(lf) => lf,
                Err(e) => {
                    warn!("Failed to parse lockfile {}: {e}", path.display());
                    continue;
                }
            };

            let ide_type = match ide_for_key(&lockfile.ide_name) {
                Some(t) => t,
                None => {
                    debug!(
                        "Unknown IDE name '{}' in lockfile {}",
                        lockfile.ide_name,
                        path.display()
                    );
                    continue;
                }
            };

            // Validate workspace match (unless skipped)
            if !skip_valid_check && !workspace_matches(&lockfile.workspace_folders, cwd) {
                debug!(
                    "Lockfile {} workspace mismatch for {}",
                    path.display(),
                    cwd.display()
                );
                continue;
            }

            // Skip lockfiles whose process is no longer alive (best effort cleanup)
            if !is_process_alive(lockfile.pid).await {
                debug!(
                    "Stale lockfile {} (PID {} dead)",
                    path.display(),
                    lockfile.pid
                );
                // Best-effort cleanup; ignore errors (file may already be gone)
                let _ = tokio::fs::remove_file(&path).await;
                continue;
            }

            candidates.push(ResolvedLockfile {
                lockfile,
                ide_type,
                port,
                host: host.clone(),
            });
        }
    } // end for ide_dir

    candidates.into_iter().next()
}

/// Get the IDE lockfile directory path.
fn ide_lockfile_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".claude")
        .join("ide")
}

/// Check if any workspace folder is a prefix of (or matches) the current CWD.
///
/// Uses NFC normalization and path-separator boundary checking to avoid
/// false positives (e.g. `/home/user/project` must not match `/home/user/project2`).
/// Matches Claude Code's `path === workspace || path.startsWith(workspace + "/")` logic.
fn workspace_matches(workspace_folders: &[String], cwd: &Path) -> bool {
    let cwd_normalized = normalize_path_str(&cwd.to_string_lossy());

    workspace_folders.iter().any(|folder| {
        let folder_normalized = normalize_path_str(folder);
        // Strip trailing slash for consistent comparison
        let folder_trimmed = folder_normalized.trim_end_matches('/');
        // Exact match or CWD is a subdirectory (with path separator boundary)
        cwd_normalized == folder_trimmed
            || cwd_normalized.starts_with(&format!("{folder_trimmed}/"))
    })
}

/// NFC-normalize a path string for consistent comparison.
fn normalize_path_str(path: &str) -> String {
    path.nfc().collect::<String>()
}

/// Attempt to find the Windows host user's `.claude/ide/` directory from WSL.
///
/// Scans `/mnt/c/Users/` for user home directories and checks for `.claude/ide/`.
async fn wsl_windows_ide_dir() -> Option<PathBuf> {
    let users_dir = PathBuf::from("/mnt/c/Users");
    if !tokio::fs::try_exists(&users_dir).await.unwrap_or(false) {
        return None;
    }

    let mut entries = tokio::fs::read_dir(&users_dir).await.ok()?;

    // Skip system-level user directories
    const SKIP_DIRS: &[&str] = &["Public", "Default", "Default User", "All Users"];

    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if SKIP_DIRS.iter().any(|&s| s == name_str.as_ref()) {
            continue;
        }

        let ide_dir = entry.path().join(".claude").join("ide");
        if tokio::fs::try_exists(&ide_dir).await.unwrap_or(false) {
            debug!("Found WSL Windows IDE dir: {}", ide_dir.display());
            return Some(ide_dir);
        }
    }

    None
}

/// Best-effort check if a process is alive by PID.
///
/// Uses `/proc/<pid>` on Linux and `kill -0` via shell on macOS.
/// Falls back to assuming alive on unsupported platforms.
async fn is_process_alive(pid: i64) -> bool {
    #[cfg(target_os = "linux")]
    {
        // On Linux, /proc/<pid> exists iff the process is alive.
        tokio::fs::metadata(format!("/proc/{pid}")).await.is_ok()
    }

    #[cfg(target_os = "macos")]
    {
        // On macOS, use `kill -0 <pid>` which checks existence without
        // sending a signal. Exit code 0 = alive.
        tokio::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .is_ok_and(|s| s.success())
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        // On other platforms, assume alive. Stale lockfiles will be cleaned
        // up when TCP connectivity fails.
        let _ = pid;
        true
    }
}

#[cfg(test)]
#[path = "lockfile.test.rs"]
mod tests;
