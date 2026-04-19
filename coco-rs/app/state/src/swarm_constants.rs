//! Swarm constants matching TS `utils/swarm/constants.ts`.

/// Name used for the team leader agent.
///
/// TS: `TEAM_LEAD_NAME = 'team-lead'`
pub const TEAM_LEAD_NAME: &str = "team-lead";

/// Tmux session name for the swarm coordinator.
///
/// TS: `SWARM_SESSION_NAME = 'claude-swarm'`
pub const SWARM_SESSION_NAME: &str = "claude-swarm";

/// Tmux window name for the swarm view layout.
///
/// TS: `SWARM_VIEW_WINDOW_NAME = 'swarm-view'`
pub const SWARM_VIEW_WINDOW_NAME: &str = "swarm-view";

/// Tmux session name for hidden panes.
///
/// TS: `HIDDEN_SESSION_NAME = 'claude-hidden'`
pub const HIDDEN_SESSION_NAME: &str = "claude-hidden";

/// Tmux command name.
///
/// TS: `TMUX_COMMAND = 'tmux'`
pub const TMUX_COMMAND: &str = "tmux";

// Swarm + plan-mode env vars use the `COCO_` prefix (coco-rs native).
// TS equivalents (`CLAUDE_CODE_*`) are Anthropic-owned and not forwarded
// verbatim — coco-rs owns its own env namespace. Provider-specific flags
// like `CLAUDE_CODE_USE_BEDROCK` in the SDK env passthrough stay as-is
// because they belong to the vercel-ai provider crates.

/// Env var: override command used to spawn teammates.
///
/// TS analogue: `CLAUDE_CODE_TEAMMATE_COMMAND`.
pub const TEAMMATE_COMMAND_ENV_VAR: &str = "COCO_TEAMMATE_COMMAND";

/// Env var: teammate's assigned UI color.
///
/// TS analogue: `CLAUDE_CODE_AGENT_COLOR`.
pub const TEAMMATE_COLOR_ENV_VAR: &str = "COCO_AGENT_COLOR";

/// Env var: force plan mode for teammates.
///
/// TS analogue: `CLAUDE_CODE_PLAN_MODE_REQUIRED`.
pub const PLAN_MODE_REQUIRED_ENV_VAR: &str = "COCO_PLAN_MODE_REQUIRED";

/// Env var: enable experimental agent teams feature.
///
/// TS analogue: `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS`.
pub const AGENT_TEAMS_ENV_VAR: &str = "COCO_EXPERIMENTAL_AGENT_TEAMS";

/// Env var: teammate's agent ID (cross-process identity fallback).
///
/// TS analogue: `CLAUDE_CODE_AGENT_ID`.
pub const AGENT_ID_ENV_VAR: &str = "COCO_AGENT_ID";

/// Env var: teammate's human-facing agent name.
///
/// TS analogue: `CLAUDE_CODE_AGENT_NAME`.
pub const AGENT_NAME_ENV_VAR: &str = "COCO_AGENT_NAME";

/// Env var: teammate's team name.
///
/// TS analogue: `CLAUDE_CODE_TEAM_NAME`.
pub const TEAM_NAME_ENV_VAR: &str = "COCO_TEAM_NAME";

/// Env var: parent session ID, piped from the leader so teammates can
/// correlate cross-process logs + replay.
///
/// TS analogue: `CLAUDE_CODE_PARENT_SESSION_ID`.
pub const PARENT_SESSION_ID_ENV_VAR: &str = "COCO_PARENT_SESSION_ID";

/// Env var: opt into the VerifyPlanExecution PostToolUse hook that
/// compares plan-file mtime vs `plan_mode_entry_ms`.
///
/// TS analogue: `CLAUDE_CODE_VERIFY_PLAN`.
pub const VERIFY_PLAN_ENV_VAR: &str = "COCO_VERIFY_PLAN";

/// Generate a swarm socket name based on the current PID.
///
/// TS: `getSwarmSocketName()` → `'claude-swarm-{pid}'`
pub fn swarm_socket_name() -> String {
    format!("claude-swarm-{}", std::process::id())
}

/// Agent color names for UI differentiation.
///
/// TS: `AgentColorName` type in swarm/backends/types.ts
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentColorName {
    Red,
    Blue,
    Green,
    Yellow,
    Purple,
    Orange,
    Pink,
    Cyan,
}

impl AgentColorName {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Red => "red",
            Self::Blue => "blue",
            Self::Green => "green",
            Self::Yellow => "yellow",
            Self::Purple => "purple",
            Self::Orange => "orange",
            Self::Pink => "pink",
            Self::Cyan => "cyan",
        }
    }
}

impl std::fmt::Display for AgentColorName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
#[path = "swarm_constants.test.rs"]
mod tests;
