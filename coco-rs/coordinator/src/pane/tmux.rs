//! Tmux pane backend for teammate execution.
//!
//! TS: utils/swarm/backends/TmuxBackend.ts
//!
//! Manages tmux panes for teammates — creating splits, setting border colors
//! and titles, hiding/showing panes, and rebalancing layouts.

use async_trait::async_trait;
use tokio::sync::Mutex;

use super::CreatePaneResult;
use super::PaneBackend;
use super::PaneId;
use crate::constants::AgentColorName;
use crate::constants::HIDDEN_SESSION_NAME;
use crate::constants::SWARM_SESSION_NAME;
use crate::constants::SWARM_VIEW_WINDOW_NAME;
use crate::constants::TMUX_COMMAND;
use crate::types::BackendType;

/// Delay for shell initialization after creating a pane (ms).
///
/// TS: `PANE_SHELL_INIT_DELAY_MS = 200`
const PANE_SHELL_INIT_DELAY_MS: u64 = 200;

/// Tmux pane backend.
///
/// TS: `class TmuxBackend implements PaneBackend`
pub struct TmuxBackend {
    /// Whether we're inside tmux (leader's pane exists).
    is_native: bool,
    /// Lock for sequential pane creation (avoids race conditions).
    pane_creation_lock: Mutex<()>,
    /// Cached leader window target (used for rebalancing).
    _cached_leader_window: Mutex<Option<String>>,
    /// Whether the first pane was used for external session.
    first_pane_used: Mutex<bool>,
}

impl TmuxBackend {
    pub fn new(is_native: bool) -> Self {
        Self {
            is_native,
            pane_creation_lock: Mutex::new(()),
            _cached_leader_window: Mutex::new(None),
            first_pane_used: Mutex::new(false),
        }
    }
}

#[async_trait]
impl PaneBackend for TmuxBackend {
    fn backend_type(&self) -> BackendType {
        BackendType::Tmux
    }

    fn display_name(&self) -> &str {
        "tmux"
    }

    fn supports_hide_show(&self) -> bool {
        true
    }

    async fn is_available(&self) -> bool {
        super::is_tmux_available().await
    }

    async fn is_running_inside(&self) -> bool {
        self.is_native
    }

    async fn create_teammate_pane(
        &self,
        name: &str,
        color: AgentColorName,
    ) -> crate::Result<CreatePaneResult> {
        let _lock = self.pane_creation_lock.lock().await;

        let is_first = {
            let mut first = self.first_pane_used.lock().await;
            let was_first = !*first;
            *first = true;
            was_first
        };

        if self.is_native {
            self.create_teammate_pane_with_leader(name, color, is_first)
                .await
        } else {
            self.create_teammate_pane_external(name, color, is_first)
                .await
        }
    }

    async fn send_command_to_pane(&self, pane_id: &PaneId, command: &str) -> crate::Result<()> {
        run_tmux(&["send-keys", "-t", pane_id, command, "Enter"]).await?;
        Ok(())
    }

    async fn set_pane_border_color(
        &self,
        pane_id: &PaneId,
        color: AgentColorName,
    ) -> crate::Result<()> {
        let tmux_color = agent_color_to_tmux(color);
        // Three-step sequence mirroring TS `TmuxBackend.ts:178-202`. Step 1
        // sets the pane's foreground colour for the border; steps 2-3 set
        // the per-pane `pane-border-style` and `pane-active-border-style`
        // options so the border keeps its colour whether the pane is
        // active or inactive (requires tmux 3.2+).
        run_tmux(&[
            "select-pane",
            "-t",
            pane_id,
            "-P",
            &format!("bg=default,fg={tmux_color}"),
        ])
        .await?;
        run_tmux(&[
            "set-option",
            "-p",
            "-t",
            pane_id,
            "pane-border-style",
            &format!("fg={tmux_color}"),
        ])
        .await?;
        run_tmux(&[
            "set-option",
            "-p",
            "-t",
            pane_id,
            "pane-active-border-style",
            &format!("fg={tmux_color}"),
        ])
        .await?;
        Ok(())
    }

    async fn set_pane_title(
        &self,
        pane_id: &PaneId,
        name: &str,
        _color: AgentColorName,
    ) -> crate::Result<()> {
        run_tmux(&["select-pane", "-t", pane_id, "-T", name]).await?;
        Ok(())
    }

    async fn enable_pane_border_status(&self, _window_target: Option<&str>) -> crate::Result<()> {
        run_tmux(&["set-option", "-g", "pane-border-status", "top"]).await?;
        Ok(())
    }

    async fn rebalance_panes(&self, window_target: &str, has_leader: bool) -> crate::Result<()> {
        if has_leader {
            self.rebalance_panes_with_leader(window_target).await
        } else {
            self.rebalance_panes_tiled(window_target).await
        }
    }

    async fn kill_pane(&self, pane_id: &PaneId) -> crate::Result<bool> {
        let output = run_tmux(&["kill-pane", "-t", pane_id]).await;
        Ok(output.is_ok())
    }

    async fn hide_pane(&self, pane_id: &PaneId) -> crate::Result<bool> {
        // Ensure hidden session exists
        let has_hidden = run_tmux(&["has-session", "-t", HIDDEN_SESSION_NAME])
            .await
            .is_ok();

        if !has_hidden {
            run_tmux(&["new-session", "-d", "-s", HIDDEN_SESSION_NAME]).await?;
        }

        // Move pane to hidden session
        let result = run_tmux(&[
            "join-pane",
            "-d",
            "-t",
            &format!("{HIDDEN_SESSION_NAME}:"),
            "-s",
            pane_id,
        ])
        .await;

        Ok(result.is_ok())
    }

    async fn show_pane(
        &self,
        pane_id: &PaneId,
        target_window_or_pane: &str,
    ) -> crate::Result<bool> {
        let result = run_tmux(&[
            "join-pane",
            "-d",
            "-t",
            target_window_or_pane,
            "-s",
            &format!("{HIDDEN_SESSION_NAME}:{pane_id}"),
        ])
        .await;

        Ok(result.is_ok())
    }
}

impl TmuxBackend {
    /// Create a pane when the leader is inside tmux.
    ///
    /// TS: `createTeammatePaneWithLeader(name, color)`
    /// Layout: 30% leader (left), 70% teammates (right, tiled).
    async fn create_teammate_pane_with_leader(
        &self,
        name: &str,
        color: AgentColorName,
        is_first: bool,
    ) -> crate::Result<CreatePaneResult> {
        let split_args = if is_first {
            // First teammate: horizontal split, 70% right
            vec!["split-window", "-h", "-p", "70", "-P", "-F", "#{pane_id}"]
        } else {
            // Subsequent: vertical split in the right region
            vec!["split-window", "-v", "-P", "-F", "#{pane_id}"]
        };

        let output = run_tmux(&split_args).await?;
        let pane_id = output.trim().to_string();

        tokio::time::sleep(std::time::Duration::from_millis(PANE_SHELL_INIT_DELAY_MS)).await;

        // Set border color and title
        let _ = self.set_pane_border_color(&pane_id, color).await;
        let _ = self.set_pane_title(&pane_id, name, color).await;

        // Enable pane border titles
        let _ = self.enable_pane_border_status(None).await;

        Ok(CreatePaneResult {
            pane_id,
            is_first_teammate: is_first,
        })
    }

    /// Create a pane in an external swarm session.
    ///
    /// TS: `createTeammatePaneExternal(name, color)`
    async fn create_teammate_pane_external(
        &self,
        name: &str,
        _color: AgentColorName,
        is_first: bool,
    ) -> crate::Result<CreatePaneResult> {
        let socket_name = crate::constants::swarm_socket_name();

        if is_first {
            // Create the swarm session
            run_tmux_with_socket(
                &socket_name,
                &[
                    "new-session",
                    "-d",
                    "-s",
                    SWARM_SESSION_NAME,
                    "-n",
                    SWARM_VIEW_WINDOW_NAME,
                    "-P",
                    "-F",
                    "#{pane_id}",
                ],
            )
            .await?;
        }

        let output = run_tmux_with_socket(
            &socket_name,
            &[
                "split-window",
                "-t",
                &format!("{SWARM_SESSION_NAME}:{SWARM_VIEW_WINDOW_NAME}"),
                "-P",
                "-F",
                "#{pane_id}",
            ],
        )
        .await?;

        let pane_id = output.trim().to_string();

        tokio::time::sleep(std::time::Duration::from_millis(PANE_SHELL_INIT_DELAY_MS)).await;

        // Set title
        let _ =
            run_tmux_with_socket(&socket_name, &["select-pane", "-t", &pane_id, "-T", name]).await;

        Ok(CreatePaneResult {
            pane_id,
            is_first_teammate: is_first,
        })
    }

    /// Rebalance panes with leader (30% leader, 70% teammates).
    async fn rebalance_panes_with_leader(&self, window_target: &str) -> crate::Result<()> {
        run_tmux(&["select-layout", "-t", window_target, "main-vertical"]).await?;
        // Set leader pane width to 30%
        run_tmux(&["set-option", "-t", window_target, "main-pane-width", "30%"]).await?;
        Ok(())
    }

    /// Rebalance panes without leader (tiled layout).
    async fn rebalance_panes_tiled(&self, window_target: &str) -> crate::Result<()> {
        run_tmux(&["select-layout", "-t", window_target, "tiled"]).await?;
        Ok(())
    }
}

// ── Tmux Helpers ──

/// Run a tmux command and return stdout.
async fn run_tmux(args: &[&str]) -> crate::Result<String> {
    let output = tokio::process::Command::new(TMUX_COMMAND)
        .args(args)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(crate::CoordinatorError::generic(format!(
            "tmux command failed: {stderr}"
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Run a tmux command with a specific socket.
async fn run_tmux_with_socket(socket: &str, args: &[&str]) -> crate::Result<String> {
    let output = tokio::process::Command::new(TMUX_COMMAND)
        .arg("-L")
        .arg(socket)
        .args(args)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(crate::CoordinatorError::generic(format!(
            "tmux command failed: {stderr}"
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Map AgentColorName to tmux color strings.
///
/// TS: `getTmuxColorName(color)`
fn agent_color_to_tmux(color: AgentColorName) -> &'static str {
    match color {
        AgentColorName::Red => "red",
        AgentColorName::Blue => "blue",
        AgentColorName::Green => "green",
        AgentColorName::Yellow => "yellow",
        AgentColorName::Purple => "magenta",
        AgentColorName::Orange => "colour208",
        AgentColorName::Pink => "colour205",
        AgentColorName::Cyan => "cyan",
    }
}

#[cfg(test)]
#[path = "tmux.test.rs"]
mod tests;
