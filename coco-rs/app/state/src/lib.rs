//! Application state tree (Arc<RwLock<AppState>>).
//!
//! TS: state/AppState.ts + AppStateStore.ts (Zustand-like pattern)

pub mod swarm;
pub mod swarm_agent_handle;
pub mod swarm_backend;
pub mod swarm_backend_iterm2;
pub mod swarm_backend_pane;
pub mod swarm_backend_tmux;
pub mod swarm_config;
pub mod swarm_constants;
pub mod swarm_discovery;
pub mod swarm_file_io;
pub mod swarm_identity;
pub mod swarm_it2_setup;
pub mod swarm_layout;
pub mod swarm_mailbox;
pub mod swarm_prompt;
pub mod swarm_reconnect;
pub mod swarm_runner;
pub mod swarm_runner_loop;
pub mod swarm_spawn_utils;
pub mod swarm_task;
pub mod swarm_teammate;

use coco_types::PermissionMode;
use coco_types::ThinkingLevel;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// The central application state.
///
/// TS: AppState has 80+ fields spanning model, session, agent, token tracking,
/// tasks, MCP, plugins, notifications, speculation, remote, and feature flags.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppState {
    // ── Model & Config ──
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<ThinkingLevel>,
    #[serde(default)]
    pub fast_mode: bool,
    pub permission_mode: PermissionMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort_value: Option<String>,
    #[serde(default)]
    pub thinking_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advisor_model: Option<String>,

    // ── Session ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default)]
    pub working_dir: String,
    #[serde(default)]
    pub is_git_repo: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entrypoint: Option<String>,

    // ── Agent ──
    #[serde(default)]
    pub is_busy: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_tool: Option<String>,
    #[serde(default)]
    pub turn_count: i32,

    // ── Token Tracking ──
    #[serde(default)]
    pub total_input_tokens: i64,
    #[serde(default)]
    pub total_output_tokens: i64,
    #[serde(default)]
    pub total_cost_usd: f64,

    // ── Tasks & Agents ──
    #[serde(default)]
    pub tasks: HashMap<String, TaskEntry>,
    pub agent_names: HashMap<String, String>,
    #[serde(default)]
    pub sub_agents: HashMap<String, SubAgentState>,

    // ── File History ──
    #[serde(default)]
    pub file_history_enabled: bool,
    #[serde(default)]
    pub file_history_snapshot_count: i32,

    // ── Denial Tracking ──
    #[serde(default)]
    pub denial_consecutive: i32,
    #[serde(default)]
    pub denial_total: i32,
    #[serde(default)]
    pub denial_circuit_breaker_tripped: bool,

    // ── Feature Flags ──
    #[serde(default)]
    pub has_compacted: bool,
    #[serde(default)]
    pub plan_mode: bool,
    #[serde(default)]
    pub mcp_connected: bool,

    // ── MCP ──
    #[serde(default)]
    pub mcp_clients: HashMap<String, McpClientState>,
    #[serde(default)]
    pub mcp_tools: Vec<String>,
    #[serde(default)]
    pub mcp_commands: Vec<String>,
    #[serde(default)]
    pub mcp_resources: Vec<String>,

    // ── Plugins ──
    #[serde(default)]
    pub plugins: PluginState,

    // ── Notifications ──
    #[serde(default)]
    pub notifications: NotificationState,

    // ── Speculation/Pipelining ──
    #[serde(default)]
    pub speculation_enabled: bool,
    #[serde(default)]
    pub speculation_session_time_saved_ms: i64,

    // ── Bridge / IDE ──
    #[serde(default)]
    pub repl_bridge_enabled: bool,
    #[serde(default)]
    pub repl_bridge_connected: bool,
    #[serde(default)]
    pub repl_bridge_session_active: bool,

    // ── Remote ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_connection_status: Option<String>,
    #[serde(default)]
    pub remote_agent_task_suggestions: Vec<String>,

    // ── Inbox / Teammates ──
    #[serde(default)]
    pub inbox_messages: Vec<InboxEntry>,

    // ── Team Context ──
    /// Active team context (set when running as part of a team).
    ///
    /// TS: `AppState.teamContext` in state/AppStateStore.ts
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_context: Option<TeamContext>,

    /// Standalone agent context (non-team agent identity).
    ///
    /// TS: `AppState.standaloneAgentContext`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standalone_agent_context: Option<StandaloneAgentContext>,

    /// Task ID of the teammate currently being viewed.
    ///
    /// TS: `AppState.viewingAgentTaskId`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewing_agent_task_id: Option<String>,

    /// Selection index in the in-process agent list.
    #[serde(default)]
    pub selected_ip_agent_index: i32,

    /// Pending permission request on worker side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_worker_request: Option<PendingWorkerRequest>,

    /// Pending sandbox permission request on worker side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_sandbox_request: Option<PendingSandboxRequest>,

    // ── Coordinator Mode ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coordinator_task_index: Option<i32>,
    #[serde(default)]
    pub view_selection_mode: bool,

    // ── Elicitation ──
    #[serde(default)]
    pub elicitation_queue: Vec<ElicitationEntry>,

    // ── Sandbox ──
    /// Worker sandbox permission queue (leader side).
    ///
    /// TS: `AppState.workerSandboxPermissions` — queue + selectedIndex.
    #[serde(default)]
    pub worker_sandbox_permissions: WorkerSandboxPermissions,

    // ── Prompt Suggestion ──
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_suggestion: Option<String>,

    // ── Onboarding ──
    #[serde(default)]
    pub onboarding_completed: bool,
    #[serde(default)]
    pub onboarding_step: i32,

    // ── Session Hooks ──
    #[serde(default)]
    pub session_hooks_loaded: bool,

    // ── Bootstrap ──
    #[serde(default)]
    pub bootstrap_loaded: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bootstrap_data: Option<serde_json::Value>,
}

/// MCP client connection state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpClientState {
    pub server_name: String,
    pub status: String,
    #[serde(default)]
    pub tool_count: i32,
    #[serde(default)]
    pub error: Option<String>,
}

/// Plugin subsystem state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginState {
    #[serde(default)]
    pub enabled: Vec<String>,
    #[serde(default)]
    pub disabled: Vec<String>,
    #[serde(default)]
    pub errors: HashMap<String, String>,
    #[serde(default)]
    pub installation_status: HashMap<String, String>,
}

/// Notification subsystem state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NotificationState {
    #[serde(default)]
    pub current: Option<NotificationEntry>,
    #[serde(default)]
    pub queue: Vec<NotificationEntry>,
}

/// A single notification entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationEntry {
    pub id: String,
    pub message: String,
    #[serde(default)]
    pub level: NotificationLevel,
    pub timestamp: i64,
}

/// Notification severity level.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NotificationLevel {
    #[default]
    Info,
    Warning,
    Error,
}

/// Task entry in the AppState task map.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskEntry {
    pub subject: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_form: Option<String>,
    #[serde(default)]
    pub blocks: Vec<String>,
    #[serde(default)]
    pub blocked_by: Vec<String>,
}

/// Inbox entry from a teammate agent.
///
/// TS: `AppState.inbox.messages[]` — richer than original stub.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxEntry {
    /// Unique message ID.
    #[serde(default)]
    pub id: String,
    pub from_agent: String,
    pub content: String,
    /// Processing status.
    #[serde(default)]
    pub status: InboxMessageStatus,
    #[serde(default)]
    pub consumed: bool,
    pub timestamp: i64,
    /// Sender's UI color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Brief summary (5-10 words).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

/// Inbox message processing status.
///
/// TS: `status: 'pending' | 'processing' | 'processed'`
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InboxMessageStatus {
    #[default]
    Pending,
    Processing,
    Processed,
}

/// Team context — set when running as part of a multi-agent team.
///
/// TS: `AppState.teamContext` in state/AppStateStore.ts
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TeamContext {
    pub team_name: String,
    pub team_file_path: String,
    pub lead_agent_id: String,
    /// Own agent ID (same as lead_agent_id for leaders).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_agent_id: Option<String>,
    /// Own display name ('team-lead' for leaders).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_agent_name: Option<String>,
    /// True if this session is the team leader.
    #[serde(default)]
    pub is_leader: bool,
    /// Assigned UI color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_agent_color: Option<String>,
    /// Active teammates keyed by agent ID.
    #[serde(default)]
    pub teammates: HashMap<String, TeammateEntry>,
}

/// Entry for a teammate in the team context.
///
/// TS: `AppState.teamContext.teammates[id]`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeammateEntry {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default)]
    pub tmux_session_name: String,
    #[serde(default)]
    pub tmux_pane_id: String,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    pub spawned_at: i64,
}

/// Standalone agent context (non-team agent identity).
///
/// TS: `AppState.standaloneAgentContext`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StandaloneAgentContext {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// Pending permission request on worker side (waiting for leader response).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingWorkerRequest {
    pub tool_name: String,
    pub tool_use_id: String,
    pub description: String,
}

/// Pending sandbox permission request on worker side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingSandboxRequest {
    pub request_id: String,
    pub host: String,
}

/// Worker sandbox permission queue (leader side).
///
/// TS: `AppState.workerSandboxPermissions`
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkerSandboxPermissions {
    #[serde(default)]
    pub queue: Vec<SandboxQueueEntry>,
    #[serde(default)]
    pub selected_index: i32,
}

/// Entry in the sandbox permission queue.
///
/// TS: `workerSandboxPermissions.queue[]`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxQueueEntry {
    pub request_id: String,
    pub worker_id: String,
    pub worker_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_color: Option<String>,
    pub host: String,
    pub created_at: i64,
}

/// Elicitation queue entry (pending user input request from MCP).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitationEntry {
    pub server_name: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub timestamp: i64,
}

/// Sub-agent state tracking.
///
/// TS: utils/swarm/ — AgentStatus, inter-agent messaging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentState {
    pub agent_id: String,
    pub name: String,
    pub status: SubAgentStatus,
    /// Number of turns completed.
    #[serde(default)]
    pub turns: i32,
    /// Model used by this agent.
    #[serde(default)]
    pub model: Option<String>,
    /// Working directory for this agent.
    #[serde(default)]
    pub working_dir: Option<String>,
    /// Last message from this agent.
    #[serde(default)]
    pub last_message: Option<String>,
}

/// Sub-agent execution status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubAgentStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Backgrounded,
    Interrupted,
}

/// Inter-agent message for the mailbox system.
///
/// TS: utils/swarm/permissionSync.ts — SwarmPermissionRequest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    pub from_agent: String,
    pub to_agent: String,
    pub content: AgentMessageContent,
    pub timestamp: i64,
}

/// Content of an inter-agent message.
///
/// TS: teammateMailbox.ts defines 15+ structured protocol message types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentMessageContent {
    /// Text message from one agent to another.
    Text { text: String },

    // ── Permission ──
    /// Permission request forwarded from sub-agent to leader.
    PermissionRequest {
        request_id: String,
        tool_name: String,
        tool_use_id: String,
        description: String,
        #[serde(default)]
        input: serde_json::Value,
        #[serde(default)]
        permission_suggestions: Vec<serde_json::Value>,
    },
    /// Permission response from leader to sub-agent.
    PermissionResponse {
        request_id: String,
        #[serde(default)]
        approved: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        feedback: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        updated_input: Option<serde_json::Value>,
    },

    // ── Sandbox Permission ──
    /// Sandbox permission request from worker to leader.
    SandboxPermissionRequest {
        request_id: String,
        worker_id: String,
        worker_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        worker_color: Option<String>,
        host: String,
        created_at: i64,
    },
    /// Sandbox permission response from leader to worker.
    SandboxPermissionResponse {
        request_id: String,
        host: String,
        allow: bool,
    },

    // ── Task lifecycle ──
    /// Agent completed its task.
    TaskComplete { result: String },
    /// Agent encountered an error.
    TaskError { error: String },
    /// Task assignment from leader to teammate.
    TaskAssignment {
        task_id: String,
        subject: String,
        description: String,
        assigned_by: String,
    },

    // ── Idle notification ──
    /// Agent has become idle.
    IdleNotification {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        idle_reason: Option<IdleReason>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        completed_task_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        completed_status: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        failure_reason: Option<String>,
    },

    // ── Shutdown lifecycle ──
    /// Request to shut down a teammate.
    ShutdownRequest {
        request_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// Shutdown approved by teammate.
    ShutdownApproved {
        request_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pane_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        backend_type: Option<String>,
    },
    /// Shutdown rejected by teammate.
    ShutdownRejected { request_id: String, reason: String },

    // ── Plan approval ──
    /// Plan approval request from teammate to leader.
    PlanApprovalRequest {
        request_id: String,
        plan_file_path: String,
        plan_content: String,
    },
    /// Plan approval response from leader to teammate.
    PlanApprovalResponse {
        request_id: String,
        approved: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        feedback: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        permission_mode: Option<PermissionMode>,
    },

    // ── Mode & permission updates ──
    /// Request to change permission mode.
    ModeSetRequest { mode: PermissionMode },
    /// Team-wide permission update.
    TeamPermissionUpdate {
        tool_name: String,
        directory_path: String,
        #[serde(default)]
        rules: Vec<serde_json::Value>,
        #[serde(default)]
        behavior: String,
    },
}

/// Reason why an agent became idle.
///
/// TS: `IdleNotificationMessage.idleReason`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdleReason {
    Available,
    Interrupted,
    Failed,
}

/// Thread-safe app state store.
pub type AppStateStore = Arc<RwLock<AppState>>;

/// Create a new app state store with defaults.
pub fn create_app_state() -> AppStateStore {
    Arc::new(RwLock::new(AppState::default()))
}

/// Create app state with initial configuration.
pub fn create_app_state_with(
    model: &str,
    cwd: &str,
    permission_mode: PermissionMode,
) -> AppStateStore {
    Arc::new(RwLock::new(AppState {
        model: model.to_string(),
        working_dir: cwd.to_string(),
        permission_mode,
        ..Default::default()
    }))
}

/// Helper functions on the store.
pub async fn update_token_usage(store: &AppStateStore, input: i64, output: i64) {
    let mut state = store.write().await;
    state.total_input_tokens += input;
    state.total_output_tokens += output;
}

pub async fn set_busy(store: &AppStateStore, busy: bool, tool: Option<&str>) {
    let mut state = store.write().await;
    state.is_busy = busy;
    state.current_tool = tool.map(String::from);
}

pub async fn increment_turn(store: &AppStateStore) {
    let mut state = store.write().await;
    state.turn_count += 1;
}

/// Register a new sub-agent.
pub async fn register_sub_agent(store: &AppStateStore, agent: SubAgentState) {
    let mut state = store.write().await;
    state
        .agent_names
        .insert(agent.agent_id.clone(), agent.name.clone());
    state.sub_agents.insert(agent.agent_id.clone(), agent);
}

/// Update a sub-agent's status.
pub async fn update_sub_agent_status(
    store: &AppStateStore,
    agent_id: &str,
    status: SubAgentStatus,
    last_message: Option<String>,
) {
    let mut state = store.write().await;
    if let Some(agent) = state.sub_agents.get_mut(agent_id) {
        agent.status = status;
        if let Some(msg) = last_message {
            agent.last_message = Some(msg);
        }
    }
}

/// Get all running sub-agents.
pub async fn running_sub_agents(store: &AppStateStore) -> Vec<SubAgentState> {
    let state = store.read().await;
    state
        .sub_agents
        .values()
        .filter(|a| a.status == SubAgentStatus::Running)
        .cloned()
        .collect()
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
