//! Shared state for team/collaboration tools.

use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// A team of named agents for coordinated multi-agent work.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    /// Unique team name/ID.
    pub name: String,
    /// Description of the team's purpose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Agent type for members of this team.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    /// The agent that created this team.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leader_agent_id: Option<String>,
    /// Members of the team with rich metadata.
    #[serde(default)]
    pub members: Vec<TeamMember>,
    /// Creation timestamp (Unix seconds).
    pub created_at: i64,
}

/// A member of a team with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMember {
    /// Agent ID.
    pub agent_id: String,
    /// Display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Agent type (e.g., "general-purpose", "Explore").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    /// Model being used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// When this member joined (Unix seconds).
    pub joined_at: i64,
    /// Working directory of this member.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

/// A message in the inter-agent message store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    /// Sender agent ID.
    pub from: String,
    /// Recipient agent ID or "all" for broadcast.
    pub to: String,
    /// Message content.
    pub content: String,
    /// Timestamp (Unix seconds).
    pub timestamp: i64,
    /// Whether this message has been read.
    #[serde(default)]
    pub read: bool,
}

/// Message store for inter-agent communication (in-memory per team).
pub type MessageStore = Arc<Mutex<Vec<AgentMessage>>>;

/// Create a new empty message store.
pub fn new_message_store() -> MessageStore {
    Arc::new(Mutex::new(Vec::new()))
}

impl Team {
    /// Get member agent IDs (for backward compatibility).
    pub fn agent_ids(&self) -> Vec<String> {
        self.members.iter().map(|m| m.agent_id.clone()).collect()
    }

    /// Check if an agent is a member.
    pub fn has_member(&self, agent_id: &str) -> bool {
        self.members.iter().any(|m| m.agent_id == agent_id)
    }
}

/// Shared team store.
pub type TeamStore = Arc<Mutex<BTreeMap<String, Team>>>;

/// Create a new empty team store.
pub fn new_team_store() -> TeamStore {
    Arc::new(Mutex::new(BTreeMap::new()))
}

/// Format teams as human-readable summary.
pub fn format_team_summary(teams: &BTreeMap<String, Team>) -> String {
    if teams.is_empty() {
        return "No teams.".to_string();
    }
    let mut output = String::new();
    for team in teams.values() {
        output.push_str(&format!("Team: {}\n", team.name));
        if let Some(desc) = &team.description {
            output.push_str(&format!("  Description: {desc}\n"));
        }
        if let Some(agent_type) = &team.agent_type {
            output.push_str(&format!("  Agent type: {agent_type}\n"));
        }
        if let Some(ref leader) = team.leader_agent_id {
            output.push_str(&format!("  Leader: {leader}\n"));
        }
        output.push_str(&format!("  Members: {}\n", team.members.len()));
        for member in &team.members {
            let name = member.name.as_deref().unwrap_or(&member.agent_id);
            output.push_str(&format!("    - {name} ({})\n", member.agent_id));
        }
    }
    output
}
