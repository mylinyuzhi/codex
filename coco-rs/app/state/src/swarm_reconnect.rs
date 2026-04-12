//! Swarm reconnection — restore team context from resumed sessions.
//!
//! TS: utils/swarm/reconnection.ts

use super::TeamContext;
use super::TeammateEntry;
use super::swarm_constants::TEAM_LEAD_NAME;
use super::swarm_file_io;

/// Compute the initial team context for AppState (called at startup).
///
/// TS: `computeInitialTeamContext()`
///
/// Reads from dynamic context and team file. Returns `None` if not a
/// teammate session.
pub fn compute_initial_team_context(
    team_name: Option<&str>,
    agent_name: Option<&str>,
) -> Option<TeamContext> {
    let team_name = team_name?;
    let agent_name = agent_name.unwrap_or(TEAM_LEAD_NAME);

    let team_file = swarm_file_io::read_team_file(team_name).ok()??;
    let team_file_path = swarm_file_io::get_team_file_path(team_name)
        .to_string_lossy()
        .to_string();

    let is_leader = agent_name == TEAM_LEAD_NAME
        || format!("{agent_name}@{team_name}") == team_file.lead_agent_id;

    let self_agent_id = if is_leader {
        Some(team_file.lead_agent_id.clone())
    } else {
        Some(format!("{agent_name}@{team_name}"))
    };

    let mut teammates = std::collections::HashMap::new();
    for member in &team_file.members {
        // Skip self
        if member.name == agent_name {
            continue;
        }
        teammates.insert(
            member.agent_id.clone(),
            TeammateEntry {
                name: member.name.clone(),
                agent_type: member.agent_type.clone(),
                color: member.color.clone(),
                tmux_session_name: String::new(),
                tmux_pane_id: member.tmux_pane_id.clone(),
                cwd: member.cwd.clone(),
                worktree_path: member.worktree_path.clone(),
                spawned_at: member.joined_at,
            },
        );
    }

    Some(TeamContext {
        team_name: team_name.to_string(),
        team_file_path,
        lead_agent_id: team_file.lead_agent_id,
        self_agent_id,
        self_agent_name: Some(agent_name.to_string()),
        is_leader,
        self_agent_color: None,
        teammates,
    })
}

/// Initialize teammate context from a resumed session.
///
/// TS: `initializeTeammateContextFromSession(setAppState, teamName, agentName)`
///
/// Called when resuming a session that has team metadata in the transcript.
pub fn initialize_from_session(team_name: &str, agent_name: &str) -> Option<TeamContext> {
    compute_initial_team_context(Some(team_name), Some(agent_name))
}

/// Extract team metadata from transcript messages.
///
/// Looks for `agent_id`, `team_name`, `agent_name` fields in transcript
/// message metadata.
pub fn extract_team_metadata(messages: &[serde_json::Value]) -> Option<(String, String)> {
    for msg in messages {
        let team_name = msg.get("team_name").and_then(|v| v.as_str());
        let agent_name = msg.get("agent_name").and_then(|v| v.as_str());
        if let (Some(tn), Some(an)) = (team_name, agent_name) {
            return Some((tn.to_string(), an.to_string()));
        }
    }
    None
}

#[cfg(test)]
#[path = "swarm_reconnect.test.rs"]
mod tests;
