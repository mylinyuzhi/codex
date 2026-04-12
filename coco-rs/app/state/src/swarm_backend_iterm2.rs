//! iTerm2 pane backend for teammate execution.
//!
//! TS: utils/swarm/backends/ITermBackend.ts
//!
//! Uses the `it2` CLI to manage native iTerm2 split panes.
//! Most cosmetic methods (color, title, rebalance) are no-ops
//! due to it2 CLI performance overhead (each call spawns Python).
//!
//! Layout: vertical split for first teammate, horizontal for subsequent.
//! Includes dead session recovery loop.

use async_trait::async_trait;
use tokio::sync::Mutex;

use super::swarm::BackendType;
use super::swarm_backend::CreatePaneResult;
use super::swarm_backend::PaneBackend;
use super::swarm_backend::PaneId;
use super::swarm_constants::AgentColorName;

/// it2 CLI command name.
const IT2_COMMAND: &str = "it2";

/// iTerm2 pane backend.
///
/// TS: `class ITermBackend implements PaneBackend`
pub struct ITermBackend {
    /// Session IDs of created teammate panes (for recovery).
    teammate_session_ids: Mutex<Vec<String>>,
    /// Whether the first pane has been created.
    first_pane_used: Mutex<bool>,
    /// Lock for sequential pane creation.
    pane_creation_lock: Mutex<()>,
}

impl ITermBackend {
    pub fn new() -> Self {
        Self {
            teammate_session_ids: Mutex::new(Vec::new()),
            first_pane_used: Mutex::new(false),
            pane_creation_lock: Mutex::new(()),
        }
    }
}

impl Default for ITermBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PaneBackend for ITermBackend {
    fn backend_type(&self) -> BackendType {
        BackendType::Iterm2
    }

    fn display_name(&self) -> &str {
        "iTerm2"
    }

    fn supports_hide_show(&self) -> bool {
        false
    }

    async fn is_available(&self) -> bool {
        super::swarm_backend::is_in_iterm2() && super::swarm_backend::is_it2_cli_available().await
    }

    async fn is_running_inside(&self) -> bool {
        super::swarm_backend::is_in_iterm2()
    }

    async fn create_teammate_pane(
        &self,
        _name: &str,
        _color: AgentColorName,
    ) -> anyhow::Result<CreatePaneResult> {
        let _lock = self.pane_creation_lock.lock().await;

        let is_first = {
            let first = self.first_pane_used.lock().await;
            !*first
        };

        // Determine target session for split
        let target_session = if is_first {
            // Split from leader
            get_leader_session_id()
        } else {
            // Split from last teammate
            let ids = self.teammate_session_ids.lock().await;
            ids.last().cloned()
        };

        // Dead session recovery loop
        // Bounded at O(N+1) where N = number of dead sessions
        let mut attempt_target = target_session;

        loop {
            // Build split command
            let split_dir = if is_first { "-v" } else { "-h" };
            let mut args = vec!["session", "split", split_dir];
            if let Some(ref target) = attempt_target {
                args.push("-s");
                args.push(target);
            }

            let output = run_it2(&args).await;

            match output {
                Ok(stdout) => {
                    let pane_id = parse_split_output(&stdout);
                    if pane_id.is_empty() {
                        anyhow::bail!("Failed to parse pane ID from it2 split output");
                    }

                    // Track the new session
                    self.teammate_session_ids.lock().await.push(pane_id.clone());
                    *self.first_pane_used.lock().await = true;

                    return Ok(CreatePaneResult {
                        pane_id,
                        is_first_teammate: is_first,
                    });
                }
                Err(e) => {
                    // Dead session recovery: check if target exists
                    if let Some(ref target) = attempt_target {
                        let session_alive = check_session_exists(target).await;
                        if !session_alive {
                            // Prune dead session and retry
                            let mut ids = self.teammate_session_ids.lock().await;
                            ids.retain(|id| id != target);
                            if ids.is_empty() {
                                *self.first_pane_used.lock().await = false;
                            }
                            // Try next target
                            attempt_target = ids.last().cloned().or_else(get_leader_session_id);
                            continue;
                        }
                    }
                    // Session alive but split failed — surface error
                    anyhow::bail!("iTerm2 split failed: {e}");
                }
            }
        }
    }

    async fn send_command_to_pane(&self, pane_id: &PaneId, command: &str) -> anyhow::Result<()> {
        let args = if pane_id.is_empty() {
            vec!["session", "run", command]
        } else {
            vec!["session", "run", "-s", pane_id, command]
        };
        run_it2(&args).await?;
        Ok(())
    }

    /// No-op: skipped for performance (each it2 call spawns Python).
    async fn set_pane_border_color(
        &self,
        _pane_id: &PaneId,
        _color: AgentColorName,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    /// No-op: skipped for performance.
    async fn set_pane_title(
        &self,
        _pane_id: &PaneId,
        _name: &str,
        _color: AgentColorName,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    /// No-op: iTerm2 shows titles in tabs automatically.
    async fn enable_pane_border_status(&self, _window_target: Option<&str>) -> anyhow::Result<()> {
        Ok(())
    }

    /// No-op: iTerm2 handles pane balancing automatically.
    async fn rebalance_panes(&self, _window_target: &str, _has_leader: bool) -> anyhow::Result<()> {
        Ok(())
    }

    async fn kill_pane(&self, pane_id: &PaneId) -> anyhow::Result<bool> {
        // -f flag required: bypasses "Confirm before closing" preference
        let result = run_it2(&["session", "close", "-f", "-s", pane_id]).await;

        // Clean up tracking regardless of result
        let mut ids = self.teammate_session_ids.lock().await;
        ids.retain(|id| id != pane_id);
        if ids.is_empty() {
            *self.first_pane_used.lock().await = false;
        }

        Ok(result.is_ok())
    }

    /// Not supported by iTerm2.
    async fn hide_pane(&self, _pane_id: &PaneId) -> anyhow::Result<bool> {
        Ok(false)
    }

    /// Not supported by iTerm2.
    async fn show_pane(
        &self,
        _pane_id: &PaneId,
        _target_window_or_pane: &str,
    ) -> anyhow::Result<bool> {
        Ok(false)
    }
}

// ── Helpers ──

/// Run an it2 CLI command and return stdout.
async fn run_it2(args: &[&str]) -> anyhow::Result<String> {
    let output = tokio::process::Command::new(IT2_COMMAND)
        .args(args)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("it2 command failed: {stderr}");
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Parse pane ID from `it2 session split` output.
///
/// Format: "Created new pane: <session-id>"
fn parse_split_output(output: &str) -> String {
    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("Created new pane:") {
            return rest.trim().to_string();
        }
    }
    String::new()
}

/// Get the leader's iTerm2 session ID from ITERM_SESSION_ID env var.
///
/// Format: `wXtYpZ:UUID`
fn get_leader_session_id() -> Option<String> {
    std::env::var("ITERM_SESSION_ID")
        .ok()
        .and_then(|s| s.split(':').nth(1).map(String::from))
}

/// Check if an iTerm2 session exists by querying `it2 session list`.
async fn check_session_exists(session_id: &str) -> bool {
    let Ok(output) = run_it2(&["session", "list"]).await else {
        return true; // Can't tell — assume alive
    };
    output.contains(session_id)
}

#[cfg(test)]
#[path = "swarm_backend_iterm2.test.rs"]
mod tests;
