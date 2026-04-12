//! Teammate lifecycle — model fallback, init hooks, mode snapshot,
//! leader permission bridge, and spawn orchestration helpers.
//!
//! TS: teammateModel.ts, teammateInit.ts, teammateModeSnapshot.ts,
//!     leaderPermissionBridge.ts, shared/spawnMultiAgent.ts

use std::sync::RwLock;

use super::swarm_config::TeammateMode;

// ── Teammate Model Fallback (teammateModel.ts) ──

/// Get the hardcoded default model for teammates.
///
/// TS: `getHardcodedTeammateModelFallback()`
/// Returns the default model to use when no model is explicitly specified
/// for a teammate. Provider-aware in TS; simplified here.
pub fn get_default_teammate_model() -> &'static str {
    "claude-sonnet-4-6-20250514"
}

/// Resolve the model for a teammate.
///
/// TS: `resolveTeammateModel(inputModel, leaderModel)`
///
/// Priority: explicit model > config default > leader model > hardcoded fallback.
pub fn resolve_teammate_model(
    input_model: Option<&str>,
    leader_model: Option<&str>,
    config_default: Option<&str>,
) -> String {
    // "inherit" is an alias for the leader's model
    if input_model.is_some_and(|m| m == "inherit") {
        return leader_model
            .unwrap_or_else(|| get_default_teammate_model())
            .to_string();
    }

    if let Some(model) = input_model {
        return model.to_string();
    }

    if let Some(default) = config_default {
        return default.to_string();
    }

    leader_model
        .unwrap_or_else(|| get_default_teammate_model())
        .to_string()
}

// ── Teammate Mode Snapshot (teammateModeSnapshot.ts) ──

/// Captured teammate mode snapshot (frozen for session duration).
static MODE_SNAPSHOT: RwLock<Option<TeammateMode>> = RwLock::new(None);
/// CLI override for teammate mode.
static CLI_MODE_OVERRIDE: RwLock<Option<TeammateMode>> = RwLock::new(None);

/// Set a CLI override for teammate mode.
///
/// TS: `setCliTeammateModeOverride(mode)`
pub fn set_cli_teammate_mode_override(mode: TeammateMode) {
    if let Ok(mut guard) = CLI_MODE_OVERRIDE.write() {
        *guard = Some(mode);
    }
}

/// Get the CLI override for teammate mode.
///
/// TS: `getCliTeammateModeOverride()`
pub fn get_cli_teammate_mode_override() -> Option<TeammateMode> {
    CLI_MODE_OVERRIDE.read().ok().and_then(|g| *g)
}

/// Capture the teammate mode at startup (frozen for session).
///
/// TS: `captureTeammateModeSnapshot()`
pub fn capture_teammate_mode_snapshot(config_mode: TeammateMode) {
    let mode = get_cli_teammate_mode_override().unwrap_or(config_mode);
    if let Ok(mut guard) = MODE_SNAPSHOT.write() {
        *guard = Some(mode);
    }
}

/// Get the captured teammate mode.
///
/// TS: `getTeammateModeFromSnapshot()`
pub fn get_teammate_mode_from_snapshot() -> TeammateMode {
    MODE_SNAPSHOT
        .read()
        .ok()
        .and_then(|g| *g)
        .unwrap_or(TeammateMode::Auto)
}

// ── Leader Permission Bridge (leaderPermissionBridge.ts) ──

// In TS, this module stores callbacks for in-process teammates to access
// the leader's ToolUseConfirm dialog. In Rust, this is modeled as
// trait objects stored in a global registry.

use std::sync::Arc;
use tokio::sync::Mutex;

/// Callback for the leader's permission UI queue.
pub type PermissionQueueSetter = Arc<dyn Fn(serde_json::Value) + Send + Sync>;

static LEADER_PERMISSION_QUEUE: Mutex<Option<PermissionQueueSetter>> = Mutex::const_new(None);

/// Register the leader's tool use confirm queue setter.
///
/// TS: `registerLeaderToolUseConfirmQueue(setter)`
pub async fn register_leader_permission_queue(setter: PermissionQueueSetter) {
    *LEADER_PERMISSION_QUEUE.lock().await = Some(setter);
}

/// Get the registered leader permission queue setter.
///
/// TS: `getLeaderToolUseConfirmQueue()`
pub async fn get_leader_permission_queue() -> Option<PermissionQueueSetter> {
    LEADER_PERMISSION_QUEUE.lock().await.clone()
}

/// Unregister the leader's permission queue.
///
/// TS: `unregisterLeaderToolUseConfirmQueue()`
pub async fn unregister_leader_permission_queue() {
    *LEADER_PERMISSION_QUEUE.lock().await = None;
}

// ── Spawn Orchestration Helpers (spawnMultiAgent.ts) ──

/// Configuration for spawning a teammate via the Agent tool.
///
/// TS: `SpawnTeammateConfig` in tools/shared/spawnMultiAgent.ts
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SpawnTeammateConfig {
    pub name: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default)]
    pub plan_mode_required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Result of spawning a teammate.
///
/// TS: `SpawnOutput` in tools/shared/spawnMultiAgent.ts
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SpawnOutput {
    pub teammate_id: String,
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub tmux_session_name: String,
    pub tmux_window_name: String,
    pub tmux_pane_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_name: Option<String>,
    #[serde(default)]
    pub plan_mode_required: bool,
}

/// Generate a unique teammate name (avoids collisions with -2, -3 suffixes).
///
/// TS: `generateUniqueTeammateName(baseName, teamName)`
pub fn generate_unique_teammate_name(base_name: &str, existing_names: &[String]) -> String {
    if !existing_names.contains(&base_name.to_string()) {
        return base_name.to_string();
    }

    for suffix in 2..100 {
        let candidate = format!("{base_name}-{suffix}");
        if !existing_names.contains(&candidate) {
            return candidate;
        }
    }

    // Fallback: append UUID fragment
    format!("{base_name}-{}", &uuid::Uuid::new_v4().to_string()[..8])
}

/// Format a message as a teammate XML message.
///
/// TS: `formatAsTeammateMessage(from, content, color?, summary?)`
pub fn format_as_teammate_message(
    from: &str,
    content: &str,
    color: Option<&str>,
    summary: Option<&str>,
) -> String {
    let color_attr = color.map(|c| format!(" color=\"{c}\"")).unwrap_or_default();
    let summary_attr = summary
        .map(|s| format!(" summary=\"{s}\""))
        .unwrap_or_default();
    format!(
        "<teammate_message teammate_id=\"{from}\"{color_attr}{summary_attr}>\n{content}\n</teammate_message>"
    )
}

// ── Teammate Init Hooks (teammateInit.ts) ──

/// Send an idle notification on teammate stop.
///
/// TS: `teammateInit.ts` registers a stop hook that sends idle notification
/// to the leader and marks the teammate as inactive in the team file.
pub fn on_teammate_stop(
    agent_name: &str,
    team_name: &str,
    color: Option<&str>,
    last_summary: Option<&str>,
) {
    // Send idle notification to leader
    let idle_text =
        super::swarm_mailbox::create_idle_notification(agent_name, Some("available"), last_summary);

    let message = super::swarm_mailbox::TeammateMessage {
        from: agent_name.to_string(),
        text: idle_text,
        timestamp: chrono::Utc::now().to_rfc3339(),
        read: false,
        color: color.map(String::from),
        summary: Some("idle notification".to_string()),
    };

    let _ = super::swarm_mailbox::write_to_mailbox(
        super::swarm_constants::TEAM_LEAD_NAME,
        message,
        team_name,
    );

    // Mark teammate as inactive in team file
    let _ = super::swarm_file_io::set_member_active(team_name, agent_name, false);
}

// ── Message Priority (inProcessRunner.ts) ──

/// Priority order for waiting for next prompt/shutdown.
///
/// TS: `waitForNextPromptOrShutdown()` polls with this priority:
/// 1. In-memory pending user messages (highest)
/// 2. Abort signal check
/// 3. Mailbox messages:
///    a. Shutdown requests (highest mailbox priority)
///    b. Team-lead messages (second — leader = user intent)
///    c. Peer messages (FIFO, third)
/// 4. Unclaimed tasks from task list (lowest)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MessagePriority {
    /// Pending user messages from transcript view.
    PendingUserMessage = 0,
    /// Shutdown request from leader.
    ShutdownRequest = 1,
    /// Message from team lead.
    LeaderMessage = 2,
    /// Message from a peer teammate.
    PeerMessage = 3,
    /// Unclaimed task from task list.
    UnclaimedTask = 4,
}

#[cfg(test)]
#[path = "swarm_teammate.test.rs"]
mod tests;
