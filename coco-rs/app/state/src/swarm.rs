//! Swarm multi-agent orchestration.
//!
//! TS: utils/swarm/ (7.5K LOC) — in-process runner, permission sync, team helpers.

use std::collections::HashMap;
use std::sync::Arc;

use coco_types::ModelInheritance;
use coco_types::PermissionMode;
use serde::Deserialize;
use serde::Serialize;
use tokio::sync::RwLock;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use super::AgentMessage;
use super::SubAgentState;
use super::SubAgentStatus;
use super::swarm_constants::AgentColorName;

// AgentMessageContent is used in tests via `use super::*`.
#[cfg(test)]
use super::AgentMessageContent;

// ── Teammate Identity ──

/// Lightweight identity for a teammate (display/routing only).
///
/// TS: TeammateIdentity in utils/swarm/backends/types.ts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeammateIdentity {
    /// Unique agent ID (format: "name@team").
    pub agent_id: String,
    /// Display name.
    pub agent_name: String,
    /// Team this agent belongs to.
    pub team_name: String,
    /// Optional UI color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<AgentColorName>,
    /// Whether plan mode is required before implementing.
    #[serde(default)]
    pub plan_mode_required: bool,
}

// ── Spawn Result ──

/// Result of attempting to spawn an agent within the swarm.
///
/// TS: AgentSpawnResult in utils/swarm/spawnInProcess.ts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpawnResult {
    pub agent_id: String,
    pub name: String,
    pub status: SubAgentStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl AgentSpawnResult {
    pub fn success(agent_id: String, name: String) -> Self {
        Self {
            agent_id,
            name,
            status: SubAgentStatus::Running,
            error: None,
        }
    }

    pub fn failure(agent_id: String, name: String, error: String) -> Self {
        Self {
            agent_id,
            name,
            status: SubAgentStatus::Failed,
            error: Some(error),
        }
    }
}

// ── Handoff Classifier ──

/// Post-execution gate that determines whether to auto-continue or pause.
///
/// TS: HandoffClassifier in auto-mode post-execution logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffDecision {
    /// Continue autonomously — no user intervention needed.
    #[default]
    Continue,
    /// Pause and surface to the user for review.
    Pause,
    /// Escalate to the parent/leader agent.
    Escalate,
}

// ── Permission Sync Types ──

/// Status of a swarm permission request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionRequestStatus {
    Pending,
    Approved,
    Rejected,
}

/// Who resolved a permission request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionResolver {
    Leader,
    User,
    Policy,
}

/// Full permission request forwarded from a worker to the leader.
///
/// TS: SwarmPermissionRequest in utils/swarm/permissionSync.ts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmPermissionRequest {
    pub id: String,
    pub worker_id: String,
    pub worker_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_color: Option<String>,
    pub team_name: String,
    pub tool_name: String,
    pub tool_use_id: String,
    pub description: String,
    pub input: serde_json::Value,
    pub status: PermissionRequestStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_by: Option<PermissionResolver>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
    pub created_at: i64,
}

/// Resolution of a swarm permission request.
#[derive(Debug, Clone)]
pub struct PermissionResolution {
    pub decision: PermissionRequestStatus,
    pub resolved_by: PermissionResolver,
    pub feedback: Option<String>,
    pub updated_input: Option<serde_json::Value>,
}

/// Bridge for synchronizing permission requests between workers and the leader.
///
/// Workers call `request_permission()` which blocks until the leader resolves.
pub struct PermissionSyncBridge {
    leader_tx: mpsc::Sender<SwarmPermissionRequest>,
    pending: Arc<RwLock<HashMap<String, oneshot::Sender<PermissionResolution>>>>,
}

impl PermissionSyncBridge {
    pub fn new(leader_tx: mpsc::Sender<SwarmPermissionRequest>) -> Self {
        Self {
            leader_tx,
            pending: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Send a permission request and wait for the leader's resolution.
    pub async fn request_permission(
        &self,
        request: SwarmPermissionRequest,
    ) -> Result<PermissionResolution, String> {
        let (tx, rx) = oneshot::channel();
        self.pending.write().await.insert(request.id.clone(), tx);

        self.leader_tx
            .send(request)
            .await
            .map_err(|e| format!("Failed to send permission request: {e}"))?;

        rx.await
            .map_err(|_| "Permission response channel closed".to_string())
    }

    /// Resolve a pending permission request (called by the leader).
    pub async fn resolve_permission(
        &self,
        request_id: &str,
        resolution: PermissionResolution,
    ) -> bool {
        if let Some(tx) = self.pending.write().await.remove(request_id) {
            tx.send(resolution).is_ok()
        } else {
            false
        }
    }

    pub async fn pending_count(&self) -> usize {
        self.pending.read().await.len()
    }
}

// ── Team File Types ──

/// Backend type for teammate execution.
///
/// TS: `BackendType = 'tmux' | 'iterm2' | 'in-process'`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackendType {
    Tmux,
    Iterm2,
    InProcess,
}

impl BackendType {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Tmux => "tmux",
            Self::Iterm2 => "iterm2",
            Self::InProcess => "in-process",
        }
    }

    /// Whether this backend uses terminal panes.
    pub const fn is_pane_backend(&self) -> bool {
        matches!(self, Self::Tmux | Self::Iterm2)
    }
}

/// Persistent team member record.
///
/// TS: TeamFile.members[] in utils/swarm/teamHelpers.ts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMember {
    pub agent_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Initial prompt/task for this teammate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Whether plan mode is required before implementing.
    #[serde(default)]
    pub plan_mode_required: bool,
    pub joined_at: i64,
    /// Tmux pane ID (empty for in-process).
    #[serde(default)]
    pub tmux_pane_id: String,
    pub cwd: String,
    /// Git worktree path if isolated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Topic subscriptions for inter-agent messaging.
    #[serde(default)]
    pub subscriptions: Vec<String>,
    /// Execution backend.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_type: Option<BackendType>,
    #[serde(default)]
    pub is_active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<PermissionMode>,
}

/// A path that all teammates in the team are allowed to edit.
///
/// TS: `TeamAllowedPath` in utils/swarm/teamHelpers.ts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamAllowedPath {
    /// Directory path (absolute).
    pub path: String,
    /// Tool name (e.g. "Edit", "Write").
    pub tool_name: String,
    /// Agent name who added this rule.
    pub added_by: String,
    /// Timestamp when added.
    pub added_at: i64,
}

/// On-disk team file (persisted as JSON at ~/.claude/teams/{name}/config.json).
///
/// TS: TeamFile in utils/swarm/teamHelpers.ts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamFile {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub created_at: i64,
    pub lead_agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead_session_id: Option<String>,
    /// Pane IDs currently hidden from UI.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden_pane_ids: Vec<String>,
    /// Shared paths all teammates can edit.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub team_allowed_paths: Vec<TeamAllowedPath>,
    #[serde(default)]
    pub members: Vec<TeamMember>,
}

// ── Team Manager ──

/// Team manager — tracks active teammates, their state, and inter-agent mailbox.
pub struct TeamManager {
    team_name: String,
    team_file: Arc<RwLock<TeamFile>>,
    agents: Arc<RwLock<HashMap<String, SubAgentState>>>,
    /// Per-agent mailboxes for inter-agent messaging.
    mailboxes: Arc<RwLock<HashMap<String, Vec<AgentMessage>>>>,
    /// Per-agent handoff classifier decisions.
    handoff_classifiers: Arc<RwLock<HashMap<String, HandoffDecision>>>,
    /// Per-agent model inheritance tracking (for debugging).
    model_sources: Arc<RwLock<HashMap<String, ModelInheritance>>>,
}

impl TeamManager {
    pub fn new(team_name: String, team_file: TeamFile) -> Self {
        Self {
            team_name,
            team_file: Arc::new(RwLock::new(team_file)),
            agents: Arc::new(RwLock::new(HashMap::new())),
            mailboxes: Arc::new(RwLock::new(HashMap::new())),
            handoff_classifiers: Arc::new(RwLock::new(HashMap::new())),
            model_sources: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn team_name(&self) -> &str {
        &self.team_name
    }

    pub async fn team_file(&self) -> TeamFile {
        self.team_file.read().await.clone()
    }

    /// Register a running agent.
    pub async fn register_agent(&self, agent: SubAgentState) {
        self.agents
            .write()
            .await
            .insert(agent.agent_id.clone(), agent);
    }

    /// Get all running agents.
    pub async fn running_agents(&self) -> Vec<SubAgentState> {
        self.agents
            .read()
            .await
            .values()
            .filter(|a| a.status == SubAgentStatus::Running)
            .cloned()
            .collect()
    }

    /// Remove a member from the team file.
    pub async fn remove_member(&self, agent_id: &str) -> bool {
        let mut tf = self.team_file.write().await;
        let before = tf.members.len();
        tf.members.retain(|m| m.agent_id != agent_id);
        self.agents.write().await.remove(agent_id);
        tf.members.len() < before
    }

    /// Get the member count from the team file.
    pub async fn member_count(&self) -> usize {
        self.team_file.read().await.members.len()
    }

    /// Check whether the given agent ID is the team leader.
    pub async fn is_leader(&self, agent_id: &str) -> bool {
        self.team_file.read().await.lead_agent_id == agent_id
    }

    /// Set the permission mode for a member (by name).
    pub async fn set_member_mode(&self, name: &str, mode: PermissionMode) -> bool {
        let mut tf = self.team_file.write().await;
        if let Some(member) = tf.members.iter_mut().find(|m| m.name == name) {
            member.mode = Some(mode);
            true
        } else {
            false
        }
    }

    /// Send a message to an agent's mailbox.
    pub async fn send_message(&self, to_agent: &str, msg: AgentMessage) {
        let mut mailboxes = self.mailboxes.write().await;
        mailboxes.entry(to_agent.to_string()).or_default().push(msg);
    }

    /// Read and drain messages from an agent's mailbox.
    pub async fn read_mailbox(&self, agent_id: &str) -> Vec<AgentMessage> {
        let mut mailboxes = self.mailboxes.write().await;
        mailboxes.remove(agent_id).unwrap_or_default()
    }

    /// Set the handoff classifier decision for an agent.
    pub async fn set_handoff_decision(&self, agent_id: &str, decision: HandoffDecision) {
        self.handoff_classifiers
            .write()
            .await
            .insert(agent_id.to_string(), decision);
    }

    /// Get the handoff classifier decision for an agent.
    pub async fn handoff_decision(&self, agent_id: &str) -> HandoffDecision {
        self.handoff_classifiers
            .read()
            .await
            .get(agent_id)
            .copied()
            .unwrap_or_default()
    }

    /// Record how a model was resolved for an agent (for debugging).
    pub async fn set_model_source(&self, agent_id: &str, inheritance: ModelInheritance) {
        self.model_sources
            .write()
            .await
            .insert(agent_id.to_string(), inheritance);
    }

    /// Get the model inheritance record for an agent.
    pub async fn model_source(&self, agent_id: &str) -> Option<ModelInheritance> {
        self.model_sources.read().await.get(agent_id).cloned()
    }
}

// ── Utility Functions ──

pub fn generate_request_id() -> String {
    format!("perm-{}", uuid::Uuid::new_v4())
}

pub fn generate_sandbox_request_id() -> String {
    format!("sandbox-{}", uuid::Uuid::new_v4())
}

/// Sanitize a name for use as a file/directory name.
pub fn sanitize_name(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Sanitize an agent name (replace @ with -).
pub fn sanitize_agent_name(name: &str) -> String {
    name.replace('@', "-")
}

#[cfg(test)]
#[path = "swarm.test.rs"]
mod tests;
