//! Team configuration.
//!
//! Configurable settings for the agent team system. These are loaded
//! from the config layer (not hardcoded) and flow through `ConfigManager`.

use serde::Deserialize;
use serde::Serialize;

/// Configuration for the agent team system.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TeamConfig {
    /// Maximum number of members allowed per team.
    pub max_members_per_team: usize,

    /// Polling interval in milliseconds for checking the mailbox.
    ///
    /// Used by in-process teammate runners to check for new messages.
    pub mailbox_poll_interval_ms: u64,

    /// Seconds before an idle agent triggers a timeout notification.
    pub idle_timeout_secs: u64,

    /// Seconds to wait for a shutdown acknowledgement before timeout.
    pub shutdown_timeout_secs: u64,

    /// Whether to persist team state to the filesystem.
    ///
    /// When `true`, team configs are written to `~/.cocode/teams/{name}/config.json`
    /// and mailbox messages to `~/.cocode/teams/{name}/mailbox/{agent}.jsonl`.
    /// When `false`, all state is in-memory only (useful for tests).
    pub persist_to_disk: bool,

    /// Default agent type for new team members when not specified.
    pub default_agent_type: String,
}

impl Default for TeamConfig {
    fn default() -> Self {
        Self {
            max_members_per_team: 10,
            mailbox_poll_interval_ms: 500,
            idle_timeout_secs: 300,
            shutdown_timeout_secs: 60,
            persist_to_disk: true,
            default_agent_type: "general-purpose".to_string(),
        }
    }
}

#[cfg(test)]
#[path = "config.test.rs"]
mod tests;
