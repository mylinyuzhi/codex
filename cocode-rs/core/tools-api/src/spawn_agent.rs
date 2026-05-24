//! Subagent spawning types and callback definitions.
//!
//! Provides [`SpawnAgentInput`], [`SpawnAgentResult`], and the [`SpawnAgentFn`]
//! callback type used to decouple tool→subagent spawning from the executor.

use cocode_protocol::RoleSelections;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// Input for spawning a subagent.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpawnAgentInput {
    /// The agent type to spawn.
    pub agent_type: String,
    /// The task prompt for the agent.
    pub prompt: String,
    /// Optional model override.
    pub model: Option<String>,
    /// Optional turn limit override.
    pub max_turns: Option<i32>,
    /// Whether to run in background.
    ///
    /// - `Some(true/false)`: Explicitly set by the model.
    /// - `None`: Deferred to the agent definition's `background` default.
    pub run_in_background: Option<bool>,
    /// Optional tool filter override.
    pub allowed_tools: Option<Vec<String>>,
    /// Parent's role selections (snapshot at spawn time for isolation).
    ///
    /// When present, the spawned subagent will use these selections,
    /// ensuring it's unaffected by subsequent changes to the parent's settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_selections: Option<RoleSelections>,
    /// Permission mode override for the subagent.
    ///
    /// When set (from `AgentDefinition.permission_mode`), the subagent uses
    /// this mode instead of inheriting from the parent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<cocode_protocol::PermissionMode>,
    /// Agent ID to resume from a previous invocation.
    ///
    /// When set, the agent continues from the previous execution's output,
    /// prepending the prior context to the prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_from: Option<String>,

    /// Isolation mode for the spawned agent.
    ///
    /// When set to `"worktree"`, a temporary git worktree is created and the
    /// agent's CWD is set to the worktree path. Auto-cleanup on completion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolation: Option<String>,

    /// Display name for the spawned agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Team to auto-join the agent to after spawn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_name: Option<String>,

    /// Agent execution mode (normal, plan, auto).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,

    /// Working directory for the spawned agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,

    /// Short description of what the agent will do (for TUI display).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Result of spawning a subagent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnAgentResult {
    /// The unique agent ID.
    pub agent_id: String,
    /// The agent output (foreground only).
    pub output: Option<String>,
    /// Background agent output file path.
    pub output_file: Option<PathBuf>,
    /// Cancellation token for the spawned agent.
    ///
    /// Present for background agents so the caller can register it
    /// in `agent_cancel_tokens` for TaskStop to cancel by ID.
    #[serde(skip)]
    pub cancel_token: Option<CancellationToken>,
    /// Display color from agent definition (for TUI rendering).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// Input for a single-shot model call (no agent loop).
#[derive(Debug, Clone)]
pub struct ModelCallInput {
    /// The call options (messages + JSON response format).
    pub request: cocode_inference::LanguageModelCallOptions,
}

/// Result of a single-shot model call.
#[derive(Debug, Clone)]
pub struct ModelCallResult {
    /// The generate result.
    pub response: cocode_inference::LanguageModelGenerateResult,
}

/// Lightweight model call callback — single request/response, no agent loop.
/// Used by SmartEdit for LLM-assisted edit correction.
pub type ModelCallFn = Arc<
    dyn Fn(
            ModelCallInput,
        ) -> Pin<
            Box<
                dyn std::future::Future<
                        Output = std::result::Result<ModelCallResult, cocode_error::BoxedError>,
                    > + Send,
            >,
        > + Send
        + Sync,
>;

/// Shared registry of cancellation tokens for background agents.
///
/// When a subagent is spawned, its `CancellationToken` is registered here.
/// TaskStop can look up the token by agent ID and cancel it directly,
/// without needing a callback to the SubagentManager.
pub type AgentCancelTokens = Arc<Mutex<HashMap<String, CancellationToken>>>;

/// Shared set of agent IDs that have been explicitly killed via TaskStop.
///
/// When TaskStop cancels an agent, its ID is recorded here. The session
/// layer checks this set when building background task info so the agent's
/// status is reported as `Killed` rather than `Failed`.
pub type KilledAgents = Arc<Mutex<HashSet<String>>>;

/// Type alias for the agent spawn callback function.
///
/// This callback is provided by the executor layer to enable tools
/// to spawn subagents without creating circular dependencies.
pub type SpawnAgentFn = Arc<
    dyn Fn(
            SpawnAgentInput,
        ) -> Pin<
            Box<
                dyn std::future::Future<
                        Output = std::result::Result<SpawnAgentResult, cocode_error::BoxedError>,
                    > + Send,
            >,
        > + Send
        + Sync,
>;
