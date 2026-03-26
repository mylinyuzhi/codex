//! macOS sandbox violation monitor.
//!
//! Spawns a `log stream` process to capture Seatbelt deny events in real-time,
//! parses each line into a `Violation`, and pushes it into a shared
//! `ViolationStore`.
//!
//! Uses a session-unique tag (`_<random>_SBX`) to filter log events and
//! base64-encoded command tags (`CMD64_<b64>_END_<tag>`) for correlation.

use std::sync::Arc;

use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

#[cfg(any(target_os = "macos", test))]
use crate::violation::Violation;
use crate::violation::ViolationStore;

/// Monitors macOS system log for Seatbelt sandbox violations.
///
/// Spawns a `log stream` child process with a predicate that matches
/// Seatbelt deny events tagged with this session's unique tag.
pub struct ViolationMonitor {
    cancel_token: CancellationToken,
    task_handle: Option<JoinHandle<()>>,
    /// Stored for API access (e.g., generating command tags via the monitor).
    #[allow(dead_code)]
    session_tag: String,
}

impl ViolationMonitor {
    /// Start monitoring macOS sandbox violations.
    ///
    /// Only works on macOS; returns `None` on other platforms.
    /// The monitor runs on a background tokio task and writes parsed
    /// violations into the provided store.
    #[cfg(target_os = "macos")]
    pub fn start(
        violations: Arc<Mutex<ViolationStore>>,
        cancel_token: CancellationToken,
        session_tag: String,
    ) -> Option<Self> {
        let child_token = cancel_token.child_token();
        let task_token = child_token.clone();
        let tag_for_task = session_tag.clone();

        let task_handle = tokio::spawn(async move {
            if let Err(e) = run_monitor(violations, task_token, &tag_for_task).await {
                tracing::debug!("Sandbox violation monitor stopped: {e}");
            }
        });

        Some(Self {
            cancel_token: child_token,
            task_handle: Some(task_handle),
            session_tag,
        })
    }

    /// On non-macOS platforms, always returns `None`.
    #[cfg(not(target_os = "macos"))]
    pub fn start(
        _violations: Arc<Mutex<ViolationStore>>,
        _cancel_token: CancellationToken,
        session_tag: String,
    ) -> Option<Self> {
        // Store the tag even on non-macOS for API consistency
        let _ = session_tag;
        None
    }

    /// Stop the monitor gracefully.
    ///
    /// Cancels the background task and waits for it to finish.
    /// The child process is killed automatically (`kill_on_drop`).
    pub async fn stop(&mut self) {
        self.cancel_token.cancel();
        if let Some(handle) = self.task_handle.take() {
            let _ = handle.await;
        }
    }
}

/// Generate a session-unique tag for filtering macOS log events.
///
/// Format: `_<8 hex chars>_SBX` (e.g., `_a1b2c3d4_SBX`).
/// Unique per session to avoid cross-session interference.
pub fn generate_session_tag() -> String {
    use rand::Rng;
    let hex: String = rand::rng()
        .sample_iter(rand::distr::Alphanumeric)
        .take(8)
        .map(char::from)
        .collect();
    format!("_{hex}_SBX")
}

/// Maximum command length before base64 encoding (matches Claude Code's T21).
///
/// Long commands are truncated to avoid oversized SBPL message strings.
const MAX_COMMAND_TAG_INPUT: usize = 100;

/// Generate a base64-encoded command tag for violation correlation.
///
/// Format: `CMD64_<base64(command[:100])>_END<session_tag>`
/// Embedded in the sandboxed command so log violations can be correlated
/// back to the specific command that triggered them.
///
/// Command is truncated to [`MAX_COMMAND_TAG_INPUT`] chars (at a valid
/// UTF-8 boundary) before encoding, matching Claude Code's behavior.
pub fn generate_command_tag(command: &str, session_tag: &str) -> String {
    use base64::Engine;
    let truncated =
        cocode_utils_string::take_bytes_at_char_boundary(command, MAX_COMMAND_TAG_INPUT);
    let encoded = base64::engine::general_purpose::STANDARD.encode(truncated);
    format!("CMD64_{encoded}_END{session_tag}")
}

/// Decode a command from a command tag.
///
/// Returns `None` if the tag format is invalid or decoding fails.
pub fn decode_command_tag(tag: &str) -> Option<String> {
    use base64::Engine;
    let rest = tag.strip_prefix("CMD64_")?;
    let b64_end = rest.find("_END")?;
    let b64 = &rest[..b64_end];
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    String::from_utf8(bytes).ok()
}

/// Build the `log stream` predicate for this session's tag.
pub fn build_log_predicate(session_tag: &str) -> String {
    format!("eventMessage ENDSWITH \"{session_tag}\"")
}

/// Run the log stream process and feed violations into the store.
#[cfg(target_os = "macos")]
async fn run_monitor(
    violations: Arc<Mutex<ViolationStore>>,
    cancel_token: CancellationToken,
    session_tag: &str,
) -> Result<(), std::io::Error> {
    use tokio::io::AsyncBufReadExt;
    use tokio::io::BufReader;

    let predicate = build_log_predicate(session_tag);

    let mut child = tokio::process::Command::new("log")
        .args([
            "stream",
            "--level",
            "debug",
            "--predicate",
            &predicate,
            "--style",
            "compact",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()?;

    let stdout = child
        .stdout
        .take()
        .expect("stdout was piped but not available");
    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();

    loop {
        tokio::select! {
            () = cancel_token.cancelled() => {
                break;
            }
            line_result = lines.next_line() => {
                match line_result {
                    Ok(Some(line)) => {
                        if let Some(violation) = parse_violation_line(&line) {
                            violations.lock().await.push(violation);
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        tracing::debug!("Error reading log stream: {e}");
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

/// Known benign process patterns that generate expected sandbox violations.
#[cfg(any(target_os = "macos", test))]
const BENIGN_PROCESSES: &[&str] = &[
    "mDNSResponder",
    "diagnosticd",
    "analyticsd",
    "com.apple.trustd",
];

/// Parse a single log stream line into a `Violation`, if it contains a deny event.
///
/// Example log lines:
/// ```text
/// 2024-01-15 10:30:45.123 Df sandbox[1234] Sandbox: bash(5678) deny(1) file-write-data /tmp/foo
/// 2024-01-15 10:30:45.456 Df sandbox[1234] Sandbox: bash(5678) deny(1) network-outbound
/// ```
#[cfg(any(target_os = "macos", test))]
pub(crate) fn parse_violation_line(line: &str) -> Option<Violation> {
    if !line.contains("deny") {
        return None;
    }

    let operation = extract_operation(line)?;
    let path = extract_path(line, &operation);
    let command_tag = extract_command_tag(line);
    let benign = BENIGN_PROCESSES.iter().any(|p| line.contains(p));

    Some(Violation {
        timestamp: std::time::SystemTime::now(),
        operation,
        path,
        command_tag,
        benign,
    })
}

/// Extract the sandbox operation from a deny line.
///
/// Looks for `deny(<digits>)` or `deny` followed by the operation name
/// (e.g., "file-write-data", "network-outbound").
#[cfg(any(target_os = "macos", test))]
pub(crate) fn extract_operation(line: &str) -> Option<String> {
    let deny_idx = line.find("deny")?;
    let after_deny = &line[deny_idx + 4..];

    // Skip optional "(digits)"
    let rest = if after_deny.starts_with('(') {
        after_deny
            .find(')')
            .map(|i| &after_deny[i + 1..])
            .unwrap_or(after_deny)
    } else {
        after_deny
    };

    // Skip whitespace, then grab the next word.
    let trimmed = rest.trim_start();
    let end = trimmed
        .find(|c: char| c.is_whitespace())
        .unwrap_or(trimmed.len());

    if end == 0 {
        return None;
    }

    Some(trimmed[..end].to_string())
}

/// Extract a file path from the violation line, if present.
///
/// The path typically appears after the operation name and starts with '/'.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn extract_path(line: &str, operation: &str) -> Option<String> {
    let op_idx = line.find(operation)?;
    let after_op = &line[op_idx + operation.len()..];
    let trimmed = after_op.trim_start();

    if trimmed.starts_with('/') {
        let end = trimmed
            .find(|c: char| c.is_whitespace())
            .unwrap_or(trimmed.len());
        Some(trimmed[..end].to_string())
    } else {
        None
    }
}

/// Extract a command tag from a log line, if present.
///
/// Looks for the `CMD64_<base64>_END_<tag>` pattern in the line.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn extract_command_tag(line: &str) -> Option<String> {
    let start = line.find("CMD64_")?;
    let rest = &line[start..];
    // Find the end of the tag (next whitespace or end of line)
    let end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
    Some(rest[..end].to_string())
}

#[cfg(test)]
#[path = "monitor.test.rs"]
mod tests;
