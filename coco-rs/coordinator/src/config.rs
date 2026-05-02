//! Team/swarm configuration types.
//!
//! TS: configConstants.ts `TEAMMATE_MODES`, config.ts `ConfigOptions`.
//!
//! Whether the swarm subsystem is **active** is gated upstream by
//! `Feature::AgentTeams`; this struct only carries internal parameters
//! (mode, max agents, default model, etc.).

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
            teammate_mode: TeammateMode::Auto,
            teammate_default_model: None,
            show_spinner_tree: true,
            max_agents: default_max_agents(),
        }
    }
}

#[cfg(test)]
#[path = "config.test.rs"]
mod tests;
