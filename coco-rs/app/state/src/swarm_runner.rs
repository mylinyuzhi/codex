//! In-process agent runner for teammate spawning and lifecycle management.
//!
//! TS: utils/swarm/inProcessRunner.ts, utils/swarm/spawnInProcess.ts
//!
//! Provides `InProcessAgentRunner` which creates isolated agent contexts,
//! runs queries, and manages completion/cancellation. Permission forwarding
//! between agents uses an mpsc channel to the leader.

use std::collections::HashMap;
use std::sync::Arc;

use coco_types::AgentIsolation;
use coco_types::MemoryScope;
use serde::Deserialize;
use serde::Serialize;
use tokio::sync::RwLock;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

// ── Permission Types ──

/// A permission request forwarded from a spawned agent to the leader.
///
/// TS: utils/swarm/permissionSync.ts — SwarmPermissionRequest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequest {
    /// Unique request ID.
    pub id: String,
    /// Agent that is requesting permission.
    pub agent_id: String,
    /// Tool that needs permission.
    pub tool_name: String,
    /// Human-readable description of the action.
    pub description: String,
    /// Tool input as JSON.
    pub input: serde_json::Value,
}

/// Resolution of a permission request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    Approved,
    Rejected,
}

/// Result of a permission resolution.
#[derive(Debug, Clone)]
pub struct PermissionResolution {
    pub decision: PermissionDecision,
    pub feedback: Option<String>,
}

/// Bridge for forwarding permission requests from agents to the leader.
///
/// Agents call `request_permission()` which sends the request to the leader
/// via an mpsc channel. The leader resolves it by calling `resolve()`,
/// which completes the oneshot channel the agent is awaiting.
pub struct PermissionBridge {
    /// Channel to send requests to the leader.
    leader_tx: mpsc::Sender<PermissionRequest>,
    /// Pending requests awaiting resolution (keyed by request ID).
    pending: Arc<RwLock<HashMap<String, oneshot::Sender<PermissionResolution>>>>,
}

impl PermissionBridge {
    /// Create a new bridge with the given channel to the leader.
    pub fn new(leader_tx: mpsc::Sender<PermissionRequest>) -> Self {
        Self {
            leader_tx,
            pending: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Send a permission request and wait for the leader's response.
    pub async fn request_permission(
        &self,
        request: PermissionRequest,
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
    pub async fn resolve(&self, request_id: &str, resolution: PermissionResolution) -> bool {
        if let Some(tx) = self.pending.write().await.remove(request_id) {
            tx.send(resolution).is_ok()
        } else {
            false
        }
    }

    /// Get the number of pending permission requests.
    pub async fn pending_count(&self) -> usize {
        self.pending.read().await.len()
    }
}

// ── Agent Context ──

/// Isolated context for an in-process agent.
///
/// Each spawned agent gets its own context with an independent cancellation
/// signal and working directory. Mirrors TS `TeammateContext` from
/// `utils/teammateContext.ts`.
#[derive(Debug, Clone)]
pub struct AgentContext {
    /// Unique identifier for this agent (format: "name@team").
    pub agent_id: String,
    /// Display name.
    pub agent_name: String,
    /// Team this agent belongs to.
    pub team_name: String,
    /// Optional UI color.
    pub color: Option<String>,
    /// Working directory for this agent.
    pub working_dir: String,
    /// Model override for this agent.
    pub model: Option<String>,
    /// System prompt override.
    pub system_prompt: Option<String>,
    /// Whether plan mode is required before implementing.
    pub plan_mode_required: bool,
    /// Whether this agent has been cancelled.
    pub cancelled: Arc<std::sync::atomic::AtomicBool>,
    /// Tools explicitly allowed without permission prompts.
    pub allowed_tools: Vec<String>,
    /// Whether unlisted tools can show permission prompts.
    pub allow_permission_prompts: bool,
    /// Thinking/effort level override.
    pub effort: Option<String>,
    /// Cache-identical tool schema prefixes.
    pub use_exact_tools: bool,
    /// Isolation mode for the execution environment.
    pub isolation: AgentIsolation,
    /// Memory persistence scope.
    pub memory_scope: Option<MemoryScope>,
    /// Per-agent MCP server names.
    pub mcp_servers: Vec<String>,
    /// Tools this agent is not allowed to use.
    pub disallowed_tools: Vec<String>,
    /// Maximum turns before the agent should stop.
    pub max_turns: Option<i32>,
}

impl AgentContext {
    /// Check if this agent has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Cancel this agent.
    pub fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

// ── Spawn Configuration ──

/// Configuration for spawning an in-process teammate.
///
/// TS: InProcessSpawnConfig in spawnInProcess.ts
#[derive(Debug, Clone)]
pub struct SpawnConfig {
    /// Display name for the teammate.
    pub name: String,
    /// Team this teammate belongs to.
    pub team_name: String,
    /// Initial prompt/task for the teammate.
    pub prompt: String,
    /// Optional UI color.
    pub color: Option<String>,
    /// Whether teammate must enter plan mode before implementing.
    pub plan_mode_required: bool,
    /// Optional model override.
    pub model: Option<String>,
    /// Working directory (defaults to parent's CWD).
    pub working_dir: Option<String>,
    /// Optional system prompt override.
    pub system_prompt: Option<String>,
    /// Tools explicitly allowed without permission prompts.
    pub allowed_tools: Vec<String>,
    /// Whether unlisted tools can trigger permission prompts.
    pub allow_permission_prompts: bool,
    /// Thinking/effort level override.
    pub effort: Option<String>,
    /// Cache-identical tool schema prefixes.
    pub use_exact_tools: bool,
    /// Isolation mode for the execution environment.
    pub isolation: AgentIsolation,
    /// Memory persistence scope.
    pub memory_scope: Option<MemoryScope>,
    /// Per-agent MCP server names.
    pub mcp_servers: Vec<String>,
    /// Tools this agent is not allowed to use.
    pub disallowed_tools: Vec<String>,
    /// Maximum turns before the agent should stop.
    pub max_turns: Option<i32>,
}

// ── Spawn Result ──

/// Result from spawning an in-process teammate.
///
/// TS: InProcessSpawnOutput in spawnInProcess.ts
#[derive(Debug)]
pub struct SpawnResult {
    /// Whether spawn was successful.
    pub success: bool,
    /// Full agent ID (format: "name@team").
    pub agent_id: String,
    /// Error message if spawn failed.
    pub error: Option<String>,
}

// ── Runner Result ──

/// Result from running an in-process teammate to completion.
///
/// TS: InProcessRunnerResult in inProcessRunner.ts
#[derive(Debug, Clone)]
pub struct RunnerResult {
    /// Whether the run completed successfully.
    pub success: bool,
    /// Error message if failed.
    pub error: Option<String>,
    /// Output text produced by the agent.
    pub output: Option<String>,
    /// Number of turns the agent executed.
    pub turns: i32,
}

// ── Agent Entry ──

/// Internal tracking entry for a spawned agent.
struct AgentEntry {
    context: AgentContext,
    /// Channel to receive the agent's result.
    result_rx: Option<oneshot::Receiver<RunnerResult>>,
}

// ── InProcessAgentRunner ──

/// In-process agent runner — spawns, tracks, and manages teammate agents.
///
/// Unlike process-based teammates (tmux/iTerm2), in-process teammates share
/// the same Tokio runtime but use independent cancellation flags and contexts
/// for isolation.
///
/// TS: combines logic from inProcessRunner.ts and spawnInProcess.ts
pub struct InProcessAgentRunner {
    /// Active agents keyed by agent_id.
    agents: Arc<RwLock<HashMap<String, AgentEntry>>>,
    /// Permission bridge for forwarding permission requests to the leader.
    permission_bridge: Arc<PermissionBridge>,
    /// Default working directory for spawned agents.
    default_working_dir: String,
    /// Maximum concurrent agents.
    max_agents: i32,
}

impl InProcessAgentRunner {
    /// Create a new runner.
    pub fn new(
        permission_bridge: Arc<PermissionBridge>,
        default_working_dir: String,
        max_agents: i32,
    ) -> Self {
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
            permission_bridge,
            default_working_dir,
            max_agents,
        }
    }

    /// Format an agent ID from name and team.
    fn format_agent_id(name: &str, team: &str) -> String {
        format!("{name}@{team}")
    }

    /// Spawn a new in-process agent.
    ///
    /// Creates an isolated `AgentContext` with its own cancellation flag
    /// and registers the agent. Returns a `SpawnResult` indicating success
    /// or failure.
    pub async fn spawn_agent(&self, config: SpawnConfig) -> SpawnResult {
        let agent_id = Self::format_agent_id(&config.name, &config.team_name);

        // Check capacity
        {
            let agents = self.agents.read().await;
            if agents.len() >= self.max_agents as usize {
                return SpawnResult {
                    success: false,
                    agent_id,
                    error: Some(format!(
                        "Max agents ({}) reached, cannot spawn '{}'",
                        self.max_agents, config.name
                    )),
                };
            }

            // Check for duplicate
            if agents.contains_key(&agent_id) {
                let msg = format!("Agent '{agent_id}' already exists");
                return SpawnResult {
                    success: false,
                    agent_id,
                    error: Some(msg),
                };
            }
        }

        let context = AgentContext {
            agent_id: agent_id.clone(),
            agent_name: config.name,
            team_name: config.team_name,
            color: config.color,
            working_dir: config
                .working_dir
                .unwrap_or_else(|| self.default_working_dir.clone()),
            model: config.model,
            system_prompt: config.system_prompt,
            plan_mode_required: config.plan_mode_required,
            cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            allowed_tools: config.allowed_tools,
            allow_permission_prompts: config.allow_permission_prompts,
            effort: config.effort,
            use_exact_tools: config.use_exact_tools,
            isolation: config.isolation,
            memory_scope: config.memory_scope,
            mcp_servers: config.mcp_servers,
            disallowed_tools: config.disallowed_tools,
            max_turns: config.max_turns,
        };

        let entry = AgentEntry {
            context,
            result_rx: None,
        };

        self.agents.write().await.insert(agent_id.clone(), entry);

        SpawnResult {
            success: true,
            agent_id,
            error: None,
        }
    }

    /// Wait for an agent to complete.
    ///
    /// Blocks until the agent finishes via its result channel. Returns `None`
    /// if the agent does not exist or has no result channel.
    pub async fn wait_for_completion(&self, agent_id: &str) -> Option<RunnerResult> {
        let rx = {
            let mut agents = self.agents.write().await;
            agents
                .get_mut(agent_id)
                .and_then(|entry| entry.result_rx.take())
        };

        match rx {
            Some(rx) => match rx.await {
                Ok(result) => Some(result),
                Err(_) => Some(RunnerResult {
                    success: false,
                    error: Some("Agent result channel closed unexpectedly".into()),
                    output: None,
                    turns: 0,
                }),
            },
            None => None,
        }
    }

    /// Cancel a running agent.
    ///
    /// Sets the agent's cancellation flag and removes it from the active
    /// agents map. Returns `true` if the agent was found and cancelled.
    pub async fn cancel_agent(&self, agent_id: &str) -> bool {
        let entry = self.agents.write().await.remove(agent_id);
        match entry {
            Some(entry) => {
                entry.context.cancel();
                true
            }
            None => false,
        }
    }

    /// Collect the result for an agent that has completed.
    ///
    /// Removes the agent from tracking and returns its result. Returns
    /// `None` if the agent doesn't exist.
    pub async fn collect_result(&self, agent_id: &str) -> Option<RunnerResult> {
        let entry = self.agents.write().await.remove(agent_id);
        match entry {
            Some(entry) => {
                if let Some(rx) = entry.result_rx {
                    match rx.await {
                        Ok(result) => Some(result),
                        Err(_) => Some(RunnerResult {
                            success: false,
                            error: Some("Agent result channel closed".into()),
                            output: None,
                            turns: 0,
                        }),
                    }
                } else {
                    Some(RunnerResult {
                        success: false,
                        error: Some("Agent did not produce a result".into()),
                        output: None,
                        turns: 0,
                    })
                }
            }
            None => None,
        }
    }

    /// Set a result channel for an agent (used by the execution loop).
    pub async fn set_result_channel(
        &self,
        agent_id: &str,
        result_rx: oneshot::Receiver<RunnerResult>,
    ) -> bool {
        let mut agents = self.agents.write().await;
        if let Some(entry) = agents.get_mut(agent_id) {
            entry.result_rx = Some(result_rx);
            true
        } else {
            false
        }
    }

    /// Get the agent context for an agent.
    pub async fn get_context(&self, agent_id: &str) -> Option<AgentContext> {
        self.agents
            .read()
            .await
            .get(agent_id)
            .map(|e| e.context.clone())
    }

    /// Forward a permission request from a spawned agent to the leader.
    pub async fn forward_permission(
        &self,
        request: PermissionRequest,
    ) -> Result<PermissionResolution, String> {
        self.permission_bridge.request_permission(request).await
    }

    /// Get a list of all active (non-cancelled) agents.
    pub async fn active_agents(&self) -> Vec<AgentContext> {
        self.agents
            .read()
            .await
            .values()
            .filter(|e| !e.context.is_cancelled())
            .map(|e| e.context.clone())
            .collect()
    }

    /// Get the number of active agents.
    pub async fn active_count(&self) -> usize {
        self.agents
            .read()
            .await
            .values()
            .filter(|e| !e.context.is_cancelled())
            .count()
    }

    /// Cancel all active agents.
    pub async fn cancel_all(&self) {
        let agents = self.agents.write().await;
        for entry in agents.values() {
            entry.context.cancel();
        }
        drop(agents);
        self.agents.write().await.clear();
    }

    /// Get the permission bridge (for external callers that need direct access).
    pub fn permission_bridge(&self) -> &Arc<PermissionBridge> {
        &self.permission_bridge
    }
}

#[cfg(test)]
#[path = "swarm_runner.test.rs"]
mod tests;
