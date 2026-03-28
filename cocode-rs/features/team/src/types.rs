//! Core team types.
//!
//! These types model the agent team system: teams, members, messages,
//! and their associated enums. Moved from `core/tools/src/builtin/team_state.rs`
//! and extended with message types, member status, and shutdown tracking.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde::Serialize;

// ============================================================================
// Message Types
// ============================================================================

/// Type of inter-agent message.
///
/// Matches Claude Code's message type system for team communication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    /// Regular text message between agents.
    #[default]
    Message,
    /// Broadcast to all team members.
    Broadcast,
    /// Request for agent to shut down gracefully.
    ShutdownRequest,
    /// Response to shutdown request.
    ShutdownResponse,
    /// Request for plan approval from another agent.
    PlanApprovalRequest,
    /// Response to a plan approval request.
    PlanApprovalResponse,
    /// Notification that an agent has become idle.
    IdleNotification,
    /// Worker requests sandbox bypass permission from the leader.
    /// Content is JSON-serialized [`SandboxPermissionRequest`].
    SandboxPermissionRequest,
    /// Leader responds to a worker's sandbox bypass request.
    /// Content is JSON-serialized [`SandboxPermissionResponse`].
    SandboxPermissionResponse,
}

impl MessageType {
    /// Returns the string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Message => "message",
            Self::Broadcast => "broadcast",
            Self::ShutdownRequest => "shutdown_request",
            Self::ShutdownResponse => "shutdown_response",
            Self::PlanApprovalRequest => "plan_approval_request",
            Self::PlanApprovalResponse => "plan_approval_response",
            Self::IdleNotification => "idle_notification",
            Self::SandboxPermissionRequest => "sandbox_permission_request",
            Self::SandboxPermissionResponse => "sandbox_permission_response",
        }
    }
}

impl std::fmt::Display for MessageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ============================================================================
// Member Status
// ============================================================================

/// Runtime status of a team member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MemberStatus {
    /// Agent is actively processing work.
    #[default]
    Active,
    /// Agent has finished current work and is awaiting new tasks.
    Idle,
    /// Shutdown has been requested but not yet completed.
    ShuttingDown,
    /// Agent has stopped executing.
    Stopped,
}

impl MemberStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Idle => "idle",
            Self::ShuttingDown => "shutting_down",
            Self::Stopped => "stopped",
        }
    }
}

impl std::fmt::Display for MemberStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ============================================================================
// Agent Message
// ============================================================================

/// A message in the inter-agent mailbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    /// Unique message identifier.
    pub id: String,
    /// Sender agent ID or name.
    pub from: String,
    /// Recipient agent ID, name, or "all" for broadcast.
    pub to: String,
    /// Message content.
    pub content: String,
    /// Type of this message.
    #[serde(default)]
    pub message_type: MessageType,
    /// Timestamp (Unix seconds).
    pub timestamp: i64,
    /// Whether this message has been read by the recipient.
    #[serde(default)]
    pub read: bool,
    /// Optional team scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_name: Option<String>,
}

impl AgentMessage {
    /// Create a new message with a generated ID.
    pub fn new(
        from: impl Into<String>,
        to: impl Into<String>,
        content: impl Into<String>,
        message_type: MessageType,
    ) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        Self {
            id: uuid::Uuid::new_v4().to_string(),
            from: from.into(),
            to: to.into(),
            content: content.into(),
            message_type,
            timestamp: now,
            read: false,
            team_name: None,
        }
    }

    /// Set the team scope.
    pub fn with_team(mut self, team_name: impl Into<String>) -> Self {
        self.team_name = Some(team_name.into());
        self
    }
}

// ============================================================================
// Team Member
// ============================================================================

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
    /// Current runtime status.
    #[serde(default)]
    pub status: MemberStatus,
    /// Whether this member runs in the background.
    #[serde(default)]
    pub background: bool,
}

// ============================================================================
// Team
// ============================================================================

/// A team of named agents for coordinated multi-agent work.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    /// Unique team name/ID.
    pub name: String,
    /// Description of the team's purpose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Default agent type for members of this team.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    /// The agent that created this team (team lead).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leader_agent_id: Option<String>,
    /// Members of the team with rich metadata.
    #[serde(default)]
    pub members: Vec<TeamMember>,
    /// Creation timestamp (Unix seconds).
    pub created_at: i64,
}

impl Team {
    /// Get member agent IDs.
    pub fn agent_ids(&self) -> Vec<String> {
        self.members.iter().map(|m| m.agent_id.clone()).collect()
    }

    /// Check if an agent is a member (by ID or name).
    pub fn has_member(&self, id_or_name: &str) -> bool {
        self.find_member(id_or_name).is_some()
    }

    /// Find a member by ID or name.
    pub fn find_member(&self, id_or_name: &str) -> Option<&TeamMember> {
        self.members.iter().find(|m| {
            m.agent_id == id_or_name || m.name.as_deref().is_some_and(|n| n == id_or_name)
        })
    }

    /// Get active (non-stopped) members excluding the leader.
    pub fn active_non_leader_members(&self) -> Vec<&TeamMember> {
        self.members
            .iter()
            .filter(|m| {
                m.status != MemberStatus::Stopped
                    && self
                        .leader_agent_id
                        .as_ref()
                        .is_none_or(|lid| m.agent_id != *lid)
            })
            .collect()
    }
}

// ============================================================================
// Sandbox Permission Sync (Worker-Leader Protocol)
// ============================================================================

/// What kind of sandbox restriction triggered the permission request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxRestrictionKind {
    /// Network domain blocked by sandbox proxy.
    Network,
    /// Filesystem path blocked by sandbox enforcement.
    Filesystem,
    /// Unix socket blocked by sandbox enforcement.
    UnixSocket,
}

/// Worker → Leader: request sandbox bypass permission.
///
/// Sent via the mailbox when a worker agent's command is blocked by
/// sandbox restrictions and the worker needs the leader (which has
/// the TUI) to prompt the user for approval.
///
/// Matches Claude Code's `sendSandboxPermissionRequest` (xb4) protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxPermissionRequest {
    /// Unique request ID: `sandbox-{timestamp_ms}-{random_hex}`.
    pub request_id: String,
    /// The command that was blocked.
    pub command: String,
    /// What kind of restriction was hit.
    pub restriction_kind: SandboxRestrictionKind,
    /// Detail about the restriction (domain, path, or socket path).
    pub detail: String,
    /// Display name of the worker agent (for the leader's permission dialog).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_name: Option<String>,
    /// Display color of the worker agent (for TUI rendering).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_color: Option<String>,
    /// When the request was created (Unix milliseconds).
    pub created_at: i64,
}

impl SandboxPermissionRequest {
    /// Create a new request with a generated ID and current timestamp.
    pub fn new(
        command: impl Into<String>,
        restriction_kind: SandboxRestrictionKind,
        detail: impl Into<String>,
    ) -> Self {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let random = uuid::Uuid::new_v4().as_fields().0;
        Self {
            request_id: format!("sandbox-{now_ms}-{random:08x}"),
            command: command.into(),
            restriction_kind,
            detail: detail.into(),
            worker_name: None,
            worker_color: None,
            created_at: now_ms,
        }
    }

    /// Set the worker display name.
    pub fn with_worker_name(mut self, name: impl Into<String>) -> Self {
        self.worker_name = Some(name.into());
        self
    }

    /// Set the worker display color.
    pub fn with_worker_color(mut self, color: impl Into<String>) -> Self {
        self.worker_color = Some(color.into());
        self
    }

    /// Wrap this request into an [`AgentMessage`] for mailbox delivery.
    ///
    /// Serialization of this struct is infallible in practice (no maps with
    /// non-string keys, no recursive structures) so the error path is purely
    /// for type safety.
    pub fn into_message(self, from: &str, to: &str) -> Result<AgentMessage, serde_json::Error> {
        let content = serde_json::to_string(&self)?;
        Ok(AgentMessage::new(
            from,
            to,
            content,
            MessageType::SandboxPermissionRequest,
        ))
    }

    /// Parse from an [`AgentMessage`] content field.
    pub fn from_message(msg: &AgentMessage) -> Option<Self> {
        if msg.message_type != MessageType::SandboxPermissionRequest {
            return None;
        }
        serde_json::from_str(&msg.content).ok()
    }
}

/// Leader → Worker: sandbox bypass decision.
///
/// Matches Claude Code's `sendSandboxPermissionResponse` (bb4) protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxPermissionResponse {
    /// Must match the `request_id` from the corresponding request.
    pub request_id: String,
    /// Whether the user approved the bypass.
    pub approved: bool,
    /// The host/path that was approved or denied (echoed from the request detail).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// When the decision was made (Unix milliseconds, same as `SandboxPermissionRequest.created_at`).
    pub timestamp: i64,
}

impl SandboxPermissionResponse {
    /// Wrap this response into an [`AgentMessage`] for mailbox delivery.
    ///
    /// Serialization is infallible in practice; error return is for type safety.
    pub fn into_message(self, from: &str, to: &str) -> Result<AgentMessage, serde_json::Error> {
        let content = serde_json::to_string(&self)?;
        Ok(AgentMessage::new(
            from,
            to,
            content,
            MessageType::SandboxPermissionResponse,
        ))
    }

    /// Parse from an [`AgentMessage`] content field.
    pub fn from_message(msg: &AgentMessage) -> Option<Self> {
        if msg.message_type != MessageType::SandboxPermissionResponse {
            return None;
        }
        serde_json::from_str(&msg.content).ok()
    }
}

// ============================================================================
// Formatting
// ============================================================================

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
            output.push_str(&format!(
                "    - {name} ({}) [{}]\n",
                member.agent_id, member.status
            ));
        }
    }
    output
}

#[cfg(test)]
#[path = "types.test.rs"]
mod tests;
