//! Team/swarm configuration types.
//!
//! TS: configConstants.ts `TEAMMATE_MODES`, config.ts `ConfigOptions`.

use super::swarm_constants::AGENT_TEAMS_ENV_VAR;

/// How teammates are spawned.
///
/// TS: `TEAMMATE_MODES = ['auto', 'tmux', 'in-process']`
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum TeammateMode {
    /// Auto-detect: try tmux/iTerm2 first, fall back to in-process.
    #[default]
    Auto,
    /// Force tmux backend.
    Tmux,
    /// Force iTerm2 backend.
    Iterm2,
    /// Force in-process backend.
    InProcess,
}

impl TeammateMode {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Tmux => "tmux",
            Self::Iterm2 => "iterm2",
            Self::InProcess => "in-process",
        }
    }
}

/// Team/swarm configuration.
///
/// TS: `ConfigOptions.showSpinnerTree`, `teammateMode`, `teammateDefaultModel`
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TeamConfig {
    /// Whether agent teams are enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// How to spawn teammates.
    #[serde(default)]
    pub teammate_mode: TeammateMode,

    /// Default model for new teammates (None = inherit from leader).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub teammate_default_model: Option<String>,

    /// Show spinner tree instead of pills.
    #[serde(default = "default_true")]
    pub show_spinner_tree: bool,

    /// Maximum concurrent in-process agents.
    #[serde(default = "default_max_agents")]
    pub max_agents: i32,
}

fn default_true() -> bool {
    true
}

fn default_max_agents() -> i32 {
    8
}

impl Default for TeamConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            teammate_mode: TeammateMode::Auto,
            teammate_default_model: None,
            show_spinner_tree: true,
            max_agents: default_max_agents(),
        }
    }
}

/// Check whether agent teams/swarm feature is enabled.
///
/// TS: `isAgentSwarmsEnabled()` in `utils/agentSwarmsEnabled.ts`.
///
/// Checks:
/// 1. `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS` env var
/// 2. `--agent-teams` CLI flag (via config)
/// 3. GrowthBook gate `tengu_amber_flint` (not yet implemented, always passes)
pub fn is_agent_teams_enabled(config: &TeamConfig, cli_flag: bool) -> bool {
    if !config.enabled {
        return false;
    }

    // Env var override
    if std::env::var(AGENT_TEAMS_ENV_VAR)
        .ok()
        .is_some_and(|v| !v.is_empty() && v != "0" && v != "false")
    {
        return true;
    }

    // CLI flag
    if cli_flag {
        return true;
    }

    // Default: disabled for external builds, enabled for internal.
    // GrowthBook gate `tengu_amber_flint` would be checked here.
    false
}

#[cfg(test)]
#[path = "swarm_config.test.rs"]
mod tests;
