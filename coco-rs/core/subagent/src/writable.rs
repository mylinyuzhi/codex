//! Writable-source helpers used by the `/agents` create wizard.
//!
//! The wizard needs two pure-logic decisions that depend on the
//! agent-catalog model:
//!
//! 1. **Where does a new agent file live for a given `AgentSource`?**
//!    Only `UserSettings` and `ProjectSettings` are coco-rs-writable —
//!    Built-in, Plugin, Flag, and Policy come from filesystem roots
//!    coco-rs doesn't own. Centralising the directory resolution here
//!    keeps the TUI and CLI in lock-step on what "writable" means.
//!
//! 2. **What color should a freshly-created agent default to?**
//!    TS picks the next unused entry from the eight-colour palette so
//!    a new agent has visual distinctness in the Library list. Mirror
//!    that behaviour by scanning the active snapshot for occupied
//!    colours and returning the first unoccupied one.

use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;

use coco_types::AgentColorName;
use coco_types::AgentSource;

use crate::snapshot::AgentCatalogSnapshot;

/// Resolve the on-disk directory an agent definition would land in
/// for a given source. Returns `None` for sources coco-rs does not
/// itself write to (Built-in, Plugin, Flag, Policy) so callers can
/// surface a typed error rather than silently picking a wrong path.
///
/// `config_home` is normally `coco_config::global_config::config_home()`
/// (i.e. `~/.coco/`), `cwd` is the active worktree root.
///
/// TS parity: maps `AGENT_PATHS.FOLDER_NAME = '.claude'` + scope
/// resolution from `components/agents/agentFileUtils.ts:20-55`.
/// coco-rs serves user agents from `~/.coco/agents/` (not `.claude/`)
/// because the `~/.coco/` config home is the multi-provider canonical
/// root; the `<cwd>/.coco/agents/` project path keeps project-level
/// config uniformly under `.coco/`.
pub fn resolve_writable_agent_dir(
    source: AgentSource,
    config_home: &Path,
    cwd: &Path,
) -> Option<PathBuf> {
    match source {
        AgentSource::UserSettings => Some(config_home.join("agents")),
        AgentSource::ProjectSettings => Some(cwd.join(".coco").join("agents")),
        AgentSource::BuiltIn
        | AgentSource::Plugin
        | AgentSource::FlagSettings
        | AgentSource::PolicySettings => None,
    }
}

/// Pick the colour to assign to a freshly-created agent.
///
/// Preference order:
/// 1. The first palette entry not currently used by any active
///    agent (TS `getNextUnusedColor` parity).
/// 2. When every palette entry is taken, cycle by active-agent
///    count so the new agent still picks up a colour — visually
///    distinct rotation beats greyscale fallback.
///
/// Returns `None` only when the palette is empty (impossible — the
/// type guarantees eight entries) so callers can `.unwrap()` if they
/// want. Returning `Option` keeps the door open to a future
/// "no-auto-color" feature flag.
pub fn next_unused_color(snapshot: &AgentCatalogSnapshot) -> Option<AgentColorName> {
    let palette = AgentColorName::ALL;
    if palette.is_empty() {
        return None;
    }
    let used: BTreeSet<AgentColorName> = snapshot.active().filter_map(|d| d.color).collect();
    if let Some(c) = palette.iter().copied().find(|c| !used.contains(c)) {
        return Some(c);
    }
    // Full palette — cycle by active-agent count for a deterministic,
    // visually-spread fallback.
    let idx = snapshot.active_count() % palette.len();
    Some(palette[idx])
}

#[cfg(test)]
#[path = "writable.test.rs"]
mod tests;
