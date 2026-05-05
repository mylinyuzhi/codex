//! Path helpers shared by binary subcommand handlers and library
//! bootstrap code.
//!
//! Centralizes path construction that was previously duplicated across
//! `main.rs`, `tui_runner.rs`, and `run_sdk_mode`: the sessions
//! directory, the agent search paths, and the output-style directories.

use std::path::Path;
use std::path::PathBuf;

use coco_config::global_config;

/// `~/.coco/sessions` — disk root for `SessionManager`.
pub fn sessions_dir() -> PathBuf {
    global_config::config_home().join("sessions")
}

/// `~/.coco/output-styles` — single user dir for custom output-style
/// markdown files. A future iteration can add `~/.claude/output-styles`
/// for TS compatibility.
pub fn output_style_dirs() -> Vec<PathBuf> {
    vec![global_config::config_home().join("output-styles")]
}

/// Standard CLI agent search paths: `~/.coco/agents` (user) plus
/// `<cwd>/.claude/agents` (project). Mirrors TS `agentDirs` from
/// `tools/AgentTool/loadAgentsDir.ts` discovery roots and the legacy
/// `agent_spawn::get_agent_dirs` shape we replaced.
pub fn standard_agent_search_paths(
    config_home: &Path,
    cwd: &Path,
) -> coco_subagent::definition_store::AgentSearchPaths {
    coco_subagent::definition_store::AgentSearchPaths {
        user_dir: Some(config_home.join("agents")),
        project_dirs: vec![cwd.join(".claude").join("agents")],
        ..coco_subagent::definition_store::AgentSearchPaths::empty()
    }
}
