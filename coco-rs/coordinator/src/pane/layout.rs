//! Teammate color assignment and layout management.
//!
//! TS: utils/swarm/teammateLayoutManager.ts
//!
//! Assigns colors in round-robin from a fixed palette. Tracks assignments
//! per-session in a global map.

use std::collections::HashMap;
use std::sync::RwLock;

use crate::constants::AgentColorName;

/// Color palette for teammate assignment (round-robin order).
///
/// TS: `AGENT_COLORS` array in teammateLayoutManager.ts
const AGENT_COLORS: &[AgentColorName] = &[
    AgentColorName::Blue,
    AgentColorName::Green,
    AgentColorName::Yellow,
    AgentColorName::Purple,
    AgentColorName::Orange,
    AgentColorName::Pink,
    AgentColorName::Cyan,
    AgentColorName::Red,
];

/// Global state for color assignment.
static COLOR_STATE: RwLock<Option<ColorAssignmentState>> = RwLock::new(None);

struct ColorAssignmentState {
    assignments: HashMap<String, AgentColorName>,
    next_index: usize,
}

fn with_state<F, T>(f: F) -> T
where
    F: FnOnce(&mut ColorAssignmentState) -> T,
{
    let mut guard = COLOR_STATE
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let state = guard.get_or_insert_with(|| ColorAssignmentState {
        assignments: HashMap::new(),
        next_index: 0,
    });
    f(state)
}

/// Assign a color to a teammate (round-robin from palette).
///
/// TS: `assignTeammateColor(teammateId)`
///
/// Returns the assigned color. If already assigned, returns the existing one.
pub fn assign_teammate_color(teammate_id: &str) -> AgentColorName {
    with_state(|state| {
        if let Some(&color) = state.assignments.get(teammate_id) {
            return color;
        }
        let color = AGENT_COLORS[state.next_index % AGENT_COLORS.len()];
        state.next_index += 1;
        state.assignments.insert(teammate_id.to_string(), color);
        color
    })
}

/// Get the color assigned to a teammate (if any).
///
/// TS: `getTeammateColor(teammateId?)`
pub fn get_teammate_color(teammate_id: &str) -> Option<AgentColorName> {
    let guard = COLOR_STATE
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard
        .as_ref()
        .and_then(|s| s.assignments.get(teammate_id).copied())
}

/// Clear all color assignments (for testing or session reset).
///
/// TS: `clearTeammateColors()`
pub fn clear_teammate_colors() {
    let mut guard = COLOR_STATE
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *guard = None;
    let mut agent_guard = AGENT_TYPE_COLORS
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *agent_guard = None;
}

/// Per-`AgentTypeId` color cache. TS: `tools/AgentTool/agentColorManager.ts`
/// keeps this stable across spawns so the `Explore` agent always renders
/// in the same color regardless of how many copies are running.
///
/// Distinct from the per-teammate cache above (`COLOR_STATE`), which keys
/// on `name@team` for long-lived teammates spawned via `TeamCreate`.
static AGENT_TYPE_COLORS: RwLock<Option<HashMap<coco_types::AgentTypeId, AgentColorName>>> =
    RwLock::new(None);

/// Assign a color to an agent type, reusing the prior assignment when one
/// exists. Mirrors TS `setAgentColor` / `getAgentColor`. The first spawn
/// of `AgentTypeId::Builtin(SubagentType::Explore)` rotates a fresh color
/// off the palette; subsequent spawns hit the cache.
pub fn assign_agent_type_color(agent_type: &coco_types::AgentTypeId) -> AgentColorName {
    let mut guard = AGENT_TYPE_COLORS
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let cache = guard.get_or_insert_with(HashMap::new);
    if let Some(&color) = cache.get(agent_type) {
        return color;
    }
    // Use the same round-robin counter as teammates so coordinators that
    // mix teammates + standalone subagents avoid awkward color collisions
    // on the first few assignments.
    let color = with_state(|state| {
        let color = AGENT_COLORS[state.next_index % AGENT_COLORS.len()];
        state.next_index += 1;
        color
    });
    cache.insert(agent_type.clone(), color);
    color
}

/// Look up the cached color for an agent type without assigning. Useful
/// for renderers that want to avoid mutating the cache when the type
/// hasn't been spawned yet.
pub fn get_agent_type_color(agent_type: &coco_types::AgentTypeId) -> Option<AgentColorName> {
    let guard = AGENT_TYPE_COLORS
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard
        .as_ref()
        .and_then(|cache| cache.get(agent_type).copied())
}

#[cfg(test)]
#[path = "layout.test.rs"]
mod tests;
