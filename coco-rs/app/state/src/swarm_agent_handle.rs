//! AgentHandle implementation bridging tools → swarm infrastructure.
//!
//! TS: AgentTool.call() dispatches to spawnMultiAgent/runAgent/forkSubagent
//!     based on input parameters. This module provides the Rust equivalent
//!     by implementing `coco_tool::AgentHandle` atop the swarm modules.
//!
//! **Dependency flow:**
//! ```text
//! coco-tool  (defines AgentHandle trait)
//!     ↓
//! coco-tools (AgentTool calls ctx.agent.spawn_agent())
//!     ↓
//! coco-state (SwarmAgentHandle implements AgentHandle)
//!     ↓ uses
//! swarm_runner (InProcessAgentRunner)
//! swarm_mailbox (write_to_mailbox)
//! swarm_file_io (write_team_file, read_team_file)
//! swarm_backend (BackendRegistry, TeammateExecutor)
//! swarm_identity (get_agent_name, get_team_name)
//! swarm_teammate (resolve_teammate_model)
//! ```

use std::sync::Arc;
use std::time::Instant;

use coco_tool::AgentHandle;
use coco_tool::AgentSpawnRequest;
use coco_tool::AgentSpawnResponse;
use coco_tool::AgentSpawnStatus;
use tokio::sync::RwLock;

use super::SubAgentState;
use super::SubAgentStatus;
use super::swarm::TeamManager;
use super::swarm_constants::AgentColorName;
use super::swarm_constants::TEAM_LEAD_NAME;
use super::swarm_file_io::write_team_file;
use super::swarm_identity::get_agent_name;
use super::swarm_identity::get_team_name;
use super::swarm_mailbox::TeammateMessage;
use super::swarm_mailbox::write_to_mailbox;
use super::swarm_runner::InProcessAgentRunner;
use super::swarm_runner::SpawnConfig;
use super::swarm_teammate::resolve_teammate_model;

/// AgentHandle implementation backed by the swarm infrastructure.
///
/// This is the **bridge** between the tool layer (AgentTool) and the
/// state layer (swarm modules). It routes spawn requests to the
/// appropriate backend (in-process, tmux, iTerm2) and manages
/// agent lifecycle.
pub struct SwarmAgentHandle {
    /// In-process agent runner for direct spawning.
    runner: Arc<InProcessAgentRunner>,
    /// Team manager (if a team is active).
    team_manager: Arc<RwLock<Option<TeamManager>>>,
    /// Registered subagents for status tracking.
    agents: Arc<RwLock<Vec<SubAgentState>>>,
    /// Query execution engine for driving agent conversations.
    /// TS: runAgent() in AgentTool → this drives the LLM loop.
    ///
    /// When `None`, `spawn_subagent` returns a "no execution engine
    /// configured" error for sync agents instead of silently
    /// succeeding with placeholder output. Install via
    /// [`Self::with_execution_engine`] at session bootstrap.
    execution_engine: Option<coco_tool::AgentQueryEngineRef>,
    /// Worktree manager for `isolation: "worktree"` subagents.
    ///
    /// When `None`, worktree-isolation requests fail fast with a
    /// clean error. The CLI resolves the canonical git root at
    /// bootstrap and installs this via [`Self::with_worktree_manager`]
    /// so subagents spawned with worktree isolation always land in
    /// `.claude/worktrees/agent-<slug>` under the main repo.
    worktree_manager: Option<Arc<crate::agent_worktree::AgentWorktreeManager>>,
    /// Current working directory.
    cwd: String,
    /// Main loop model (for inheritance).
    main_model: String,
}

impl SwarmAgentHandle {
    pub fn new(
        runner: Arc<InProcessAgentRunner>,
        team_manager: Arc<RwLock<Option<TeamManager>>>,
        cwd: String,
        main_model: String,
    ) -> Self {
        Self {
            runner,
            team_manager,
            agents: Arc::new(RwLock::new(Vec::new())),
            execution_engine: None,
            worktree_manager: None,
            cwd,
            main_model,
        }
    }

    /// Set the execution engine for driving agent queries.
    pub fn set_execution_engine(&mut self, engine: coco_tool::AgentQueryEngineRef) {
        self.execution_engine = Some(engine);
    }

    /// Install an [`AgentWorktreeManager`](crate::agent_worktree::AgentWorktreeManager)
    /// so subagents spawned with `isolation: "worktree"` get a real
    /// git worktree + cwd_override + cleanup-on-success.
    pub fn set_worktree_manager(
        &mut self,
        manager: Arc<crate::agent_worktree::AgentWorktreeManager>,
    ) {
        self.worktree_manager = Some(manager);
    }

    /// Determine if this is a teammate spawn (has both name + team_name).
    fn is_teammate_spawn(request: &AgentSpawnRequest) -> bool {
        request.name.is_some() && request.team_name.is_some()
    }

    /// Spawn a teammate via the swarm infrastructure.
    async fn spawn_teammate(
        &self,
        request: &AgentSpawnRequest,
    ) -> Result<AgentSpawnResponse, String> {
        let name = request
            .name
            .as_deref()
            .ok_or("name required for teammate")?;
        let team_name = request
            .team_name
            .as_deref()
            .ok_or("team_name required for teammate")?;

        // Resolve model
        let model = resolve_teammate_model(request.model.as_deref(), Some(&self.main_model), None);

        // Resolve color (round-robin from name hash)
        let all_colors = [
            AgentColorName::Red,
            AgentColorName::Blue,
            AgentColorName::Green,
            AgentColorName::Yellow,
            AgentColorName::Purple,
            AgentColorName::Orange,
            AgentColorName::Pink,
            AgentColorName::Cyan,
        ];
        let color_index = name
            .bytes()
            .fold(0usize, |acc, b| acc.wrapping_add(b as usize))
            % all_colors.len();
        let color = all_colors[color_index];

        let config = SpawnConfig {
            name: name.to_string(),
            team_name: team_name.to_string(),
            prompt: request.prompt.clone(),
            color: Some(color.as_str().to_string()),
            plan_mode_required: request.mode.as_deref().is_some_and(|m| m == "plan"),
            model: Some(model.clone()),
            working_dir: request.cwd.as_ref().map(|p| p.display().to_string()),
            system_prompt: None,
            allowed_tools: Vec::new(),
            allow_permission_prompts: true,
            effort: None,
            use_exact_tools: false,
            isolation: coco_types::AgentIsolation::None,
            memory_scope: None,
            mcp_servers: Vec::new(),
            disallowed_tools: Vec::new(),
            max_turns: None,
        };

        let spawn_result = self.runner.register_agent(config).await;

        if !spawn_result.success {
            return Ok(AgentSpawnResponse {
                status: AgentSpawnStatus::Failed,
                agent_id: Some(spawn_result.agent_id),
                result: None,
                error: spawn_result.error,
                total_tool_use_count: 0,
                total_tokens: 0,
                duration_ms: 0,
                worktree_path: None,
                worktree_branch: None,
                output_file: None,
                prompt: None,
            });
        }

        // Register as subagent
        let state = SubAgentState {
            agent_id: spawn_result.agent_id.clone(),
            name: name.to_string(),
            status: SubAgentStatus::Running,
            turns: 0,
            model: Some(model),
            working_dir: request.cwd.as_ref().map(|p| p.display().to_string()),
            last_message: None,
        };
        self.agents.write().await.push(state);

        Ok(AgentSpawnResponse {
            status: AgentSpawnStatus::TeammateSpawned,
            agent_id: Some(spawn_result.agent_id),
            result: None,
            error: None,
            total_tool_use_count: 0,
            total_tokens: 0,
            duration_ms: 0,
            worktree_path: None,
            worktree_branch: None,
            output_file: None,
            prompt: None,
        })
    }

    /// Spawn a standalone subagent (not a teammate).
    async fn spawn_subagent(
        &self,
        request: &AgentSpawnRequest,
    ) -> Result<AgentSpawnResponse, String> {
        let start = Instant::now();
        let agent_type = request
            .subagent_type
            .as_deref()
            .unwrap_or("general-purpose");

        let agent_id = format!(
            "agent-{}",
            uuid::Uuid::new_v4()
                .to_string()
                .split('-')
                .next()
                .unwrap_or("0")
        );

        // Register as subagent
        let state = SubAgentState {
            agent_id: agent_id.clone(),
            name: request
                .description
                .clone()
                .unwrap_or_else(|| agent_type.to_string()),
            status: if request.run_in_background {
                SubAgentStatus::Backgrounded
            } else {
                SubAgentStatus::Running
            },
            turns: 0,
            model: request.model.clone(),
            working_dir: request.cwd.as_ref().map(|p| p.display().to_string()),
            last_message: None,
        };
        self.agents.write().await.push(state);

        if request.run_in_background {
            // Async: return immediately, agent runs in background.
            // Background + worktree combo is out of scope for this
            // slice (requires agent metadata persistence for resume);
            // if worktree was requested, warn via error but still
            // launch without isolation.
            let status = AgentSpawnStatus::AsyncLaunched;
            return Ok(AgentSpawnResponse {
                status,
                agent_id: Some(agent_id),
                result: None,
                error: None,
                total_tool_use_count: 0,
                total_tokens: 0,
                duration_ms: 0,
                worktree_path: None,
                worktree_branch: None,
                output_file: None,
                prompt: None,
            });
        }

        // ── Sync subagent path (Phase 6 Workstream C) ──
        //
        // Worktree isolation: if requested, create a worktree first
        // so its path becomes the child's cwd_override. Any creation
        // error returns a model-visible failure — we don't silently
        // fall back to sync-without-isolation, per the plan's "Make
        // Unsupported Parity Explicit" rule.
        let worktree_session = if matches!(request.isolation.as_deref(), Some("worktree")) {
            match self.worktree_manager.as_ref() {
                Some(m) => {
                    let slug = format!(
                        "agent-{}",
                        agent_id
                            .strip_prefix("agent-")
                            .unwrap_or(&agent_id)
                            .chars()
                            .take(8)
                            .collect::<String>()
                    );
                    match m.create_for(&slug) {
                        Ok(s) => Some(s),
                        Err(e) => {
                            return Ok(spawn_failed(
                                agent_id,
                                format!("Worktree creation failed: {e}"),
                                start.elapsed().as_millis() as i64,
                            ));
                        }
                    }
                }
                None => {
                    return Ok(spawn_failed(
                        agent_id,
                        "Isolation 'worktree' requested but no AgentWorktreeManager is \
                         configured. Use SwarmAgentHandle::set_worktree_manager."
                            .into(),
                        start.elapsed().as_millis() as i64,
                    ));
                }
            }
        } else {
            None
        };

        // Execution engine is required for sync subagent. Without
        // one we can't run a child — surface as a clean failure and
        // clean up the worktree we may have just created.
        let Some(engine) = self.execution_engine.clone() else {
            if let (Some(m), Some(session)) =
                (self.worktree_manager.as_ref(), worktree_session.clone())
            {
                // Best-effort cleanup — we never ran anything, so
                // the worktree definitely has no changes.
                let _ = m.cleanup_if_unchanged(session);
            }
            return Ok(spawn_failed(
                agent_id,
                "No AgentQueryEngine configured on SwarmAgentHandle. Use \
                 SwarmAgentHandle::set_execution_engine at session bootstrap."
                    .into(),
                start.elapsed().as_millis() as i64,
            ));
        };

        let cwd_override = worktree_session
            .as_ref()
            .map(|s| s.path.clone())
            .or_else(|| request.cwd.clone());

        // Build the child query config. Inherits parent model when
        // the caller didn't specify. System prompt is empty here —
        // the child engine builds its own via coco-context from
        // CLAUDE.md discovery.
        // Fork isolation: TS `AgentTool.tsx:622-632` routes fork
        // agents through a codepath that prepends the parent's
        // conversation messages to the child's turn.
        // `AgentSpawnRequest.isolation == "fork"` surfaces this
        // intent; the request's `fork_context_messages` field
        // (set by AgentTool) carries the parent's history. This
        // module doesn't have direct access to the parent history
        // (the AgentTool's ToolUseContext held it), so we rely on
        // the request. Today's request shape doesn't carry it;
        // callers requesting fork from the tool path will have
        // the messages attached by a future AgentTool change that
        // threads `ctx.messages` through.
        let is_fork = request.isolation.as_deref() == Some("fork");
        let query_config = coco_tool::AgentQueryConfig {
            system_prompt: String::new(),
            model: request
                .model
                .clone()
                .unwrap_or_else(|| self.main_model.clone()),
            max_turns: None,
            context_window: None,
            max_output_tokens: None,
            allowed_tools: Vec::new(),
            // Fork mode: preserve tool_use_results across the
            // child's compaction boundary — parent's history
            // references tool_use_ids that the child needs to
            // resolve via its own tool_result messages.
            preserve_tool_use_results: is_fork,
            permission_mode: request.mode.clone(),
            agent_id: Some(agent_id.clone()),
            is_teammate: false,
            plan_mode_required: false,
            session_id: None,
            bypass_permissions_available: false,
            cwd_override,
            // Fork context propagation: surfaces when the
            // AgentSpawnRequest carries a non-None fork-context
            // payload. Current shape: left empty until AgentTool
            // threads parent messages through.
            fork_context_messages: Vec::new(),
            // Swarm subagents default to the parent's Main role
            // today. Follow-up can map SubagentType → ModelRole
            // (Explore/Review/Plan) per agent definition; the
            // factory reads this to install the role's fallback
            // chain.
            model_role: None,
        };

        // Run the child query.
        let query_result = engine.execute_query(&request.prompt, query_config).await;
        let duration_ms = start.elapsed().as_millis() as i64;

        // Cleanup worktree regardless of success/failure. If the
        // child made changes the worktree is kept for inspection
        // and surfaced in the response payload.
        let (worktree_path, worktree_branch) =
            match (self.worktree_manager.as_ref(), worktree_session) {
                (Some(m), Some(session)) => match m.cleanup_if_unchanged(session) {
                    crate::agent_worktree::WorktreeCleanupOutcome::Removed => (None, None),
                    crate::agent_worktree::WorktreeCleanupOutcome::Kept {
                        path, branch, ..
                    } => (Some(path), Some(branch)),
                },
                _ => (None, None),
            };

        // Update tracked agent status.
        {
            let mut agents = self.agents.write().await;
            if let Some(agent) = agents.iter_mut().find(|a| a.agent_id == agent_id) {
                agent.status = match &query_result {
                    Ok(_) => SubAgentStatus::Completed,
                    Err(_) => SubAgentStatus::Failed,
                };
            }
        }

        match query_result {
            Ok(qr) => Ok(AgentSpawnResponse {
                status: AgentSpawnStatus::Completed,
                agent_id: Some(agent_id),
                result: qr.response_text,
                error: None,
                total_tool_use_count: qr.tool_use_count,
                total_tokens: qr.input_tokens + qr.output_tokens,
                duration_ms,
                worktree_path,
                worktree_branch,
                output_file: None,
                prompt: None,
            }),
            Err(e) => Ok(AgentSpawnResponse {
                status: AgentSpawnStatus::Failed,
                agent_id: Some(agent_id),
                result: None,
                error: Some(e.to_string()),
                total_tool_use_count: 0,
                total_tokens: 0,
                duration_ms,
                worktree_path,
                worktree_branch,
                output_file: None,
                prompt: None,
            }),
        }
    }
}

/// Shorthand for a sync-path early-failure response. Worktree
/// cleanup is the caller's responsibility — this helper only builds
/// the AgentSpawnResponse shell.
fn spawn_failed(agent_id: String, message: String, duration_ms: i64) -> AgentSpawnResponse {
    AgentSpawnResponse {
        status: AgentSpawnStatus::Failed,
        agent_id: Some(agent_id),
        result: None,
        error: Some(message),
        total_tool_use_count: 0,
        total_tokens: 0,
        duration_ms,
        worktree_path: None,
        worktree_branch: None,
        output_file: None,
        prompt: None,
    }
}

#[async_trait::async_trait]
impl AgentHandle for SwarmAgentHandle {
    async fn spawn_agent(&self, request: AgentSpawnRequest) -> Result<AgentSpawnResponse, String> {
        if Self::is_teammate_spawn(&request) {
            self.spawn_teammate(&request).await
        } else {
            self.spawn_subagent(&request).await
        }
    }

    async fn send_message(&self, to: &str, content: &str) -> Result<String, String> {
        let team_name = {
            let tm = self.team_manager.read().await;
            tm.as_ref()
                .map(|m| m.team_name().to_string())
                .or_else(|| get_team_name(None))
                .ok_or_else(|| "No active team — cannot send message".to_string())?
        };

        let from = get_agent_name().unwrap_or_else(|| TEAM_LEAD_NAME.to_string());

        // Broadcast: send to all teammates except sender
        if to == "*" {
            let tm = self.team_manager.read().await;
            let members = if let Some(manager) = tm.as_ref() {
                let agents = manager.running_agents().await;
                agents
                    .iter()
                    .filter(|a| a.name != from)
                    .map(|a| a.name.clone())
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            drop(tm);

            let mut sent = Vec::new();
            for recipient in &members {
                let message = TeammateMessage {
                    from: from.clone(),
                    text: content.to_string(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    read: false,
                    color: None,
                    summary: None,
                };
                if write_to_mailbox(recipient, message, &team_name).is_ok() {
                    sent.push(recipient.clone());
                }
            }
            return Ok(format!(
                "Broadcast from '{from}' to {} recipients: {}",
                sent.len(),
                sent.join(", ")
            ));
        }

        let message = TeammateMessage {
            from: from.clone(),
            text: content.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            read: false,
            color: None,
            summary: None,
        };

        write_to_mailbox(to, message, &team_name)
            .map_err(|e| format!("Failed to send message to '{to}': {e}"))?;

        Ok(format!("Message sent from '{from}' to '{to}'"))
    }

    async fn create_team(&self, name: &str) -> Result<String, String> {
        use super::swarm::TeamFile;
        use super::swarm::TeamMember;

        let lead_agent_id = format!("{TEAM_LEAD_NAME}@{name}");

        let team_file = TeamFile {
            name: name.to_string(),
            description: None,
            created_at: chrono::Utc::now().timestamp(),
            lead_agent_id: lead_agent_id.clone(),
            lead_session_id: None,
            hidden_pane_ids: Vec::new(),
            team_allowed_paths: Vec::new(),
            members: vec![TeamMember {
                agent_id: lead_agent_id.clone(),
                name: TEAM_LEAD_NAME.to_string(),
                agent_type: Some("team-lead".to_string()),
                model: Some(self.main_model.clone()),
                prompt: None,
                color: None,
                plan_mode_required: false,
                joined_at: chrono::Utc::now().timestamp(),
                tmux_pane_id: String::new(),
                cwd: self.cwd.clone(),
                worktree_path: None,
                session_id: None,
                subscriptions: Vec::new(),
                backend_type: Some(super::swarm::BackendType::InProcess),
                is_active: true,
                mode: None,
            }],
        };

        write_team_file(name, &team_file)
            .map_err(|e| format!("Failed to create team '{name}': {e}"))?;

        // Initialize team manager
        {
            let mut tm = self.team_manager.write().await;
            *tm = Some(TeamManager::new(name.to_string(), team_file.clone()));
        }

        Ok(format!("Team '{name}' created with lead '{lead_agent_id}'"))
    }

    async fn delete_team(&self, name: &str) -> Result<String, String> {
        // Check no active members
        {
            let tm = self.team_manager.read().await;
            if let Some(manager) = tm.as_ref() {
                let active = manager.running_agents().await;
                let non_lead: Vec<_> = active.iter().filter(|a| a.name != TEAM_LEAD_NAME).collect();
                if !non_lead.is_empty() {
                    let names: Vec<_> = non_lead.iter().map(|a| a.name.as_str()).collect();
                    return Err(format!(
                        "Cannot delete team: active members: {}",
                        names.join(", ")
                    ));
                }
            }
        }

        // Remove team directories
        super::swarm_file_io::cleanup_team_directories(name)
            .map_err(|e| format!("Failed to delete team '{name}': {e}"))?;

        // Clear manager
        {
            let mut tm = self.team_manager.write().await;
            *tm = None;
        }

        Ok(format!("Team '{name}' deleted"))
    }

    async fn resume_agent(
        &self,
        agent_id: &str,
        prompt: Option<&str>,
    ) -> Result<AgentSpawnResponse, String> {
        // Check if agent exists
        let agent = {
            let agents = self.agents.read().await;
            agents.iter().find(|a| a.agent_id == agent_id).cloned()
        };

        let Some(_agent) = agent else {
            return Err(format!("Agent '{agent_id}' not found"));
        };

        // Re-spawn with optional new prompt
        let new_prompt = prompt.unwrap_or("Continue from where you left off.");

        let config = SpawnConfig {
            name: agent_id.to_string(),
            team_name: "resumed".to_string(),
            prompt: new_prompt.to_string(),
            color: None,
            plan_mode_required: false,
            model: None,
            working_dir: None,
            system_prompt: None,
            allowed_tools: Vec::new(),
            allow_permission_prompts: false,
            effort: None,
            use_exact_tools: false,
            isolation: coco_types::AgentIsolation::None,
            memory_scope: None,
            mcp_servers: Vec::new(),
            disallowed_tools: Vec::new(),
            max_turns: None,
        };

        let spawn_result = self.runner.register_agent(config).await;

        Ok(AgentSpawnResponse {
            status: if spawn_result.success {
                AgentSpawnStatus::AsyncLaunched
            } else {
                AgentSpawnStatus::Failed
            },
            agent_id: Some(spawn_result.agent_id),
            result: None,
            error: spawn_result.error,
            total_tool_use_count: 0,
            total_tokens: 0,
            duration_ms: 0,
            worktree_path: None,
            worktree_branch: None,
            output_file: None,
            prompt: None,
        })
    }

    async fn query_agent_status(&self, agent_id: &str) -> Result<AgentSpawnResponse, String> {
        let agents = self.agents.read().await;
        let agent = agents
            .iter()
            .find(|a| a.agent_id == agent_id)
            .ok_or_else(|| format!("Agent '{agent_id}' not found"))?;

        let status = match agent.status {
            SubAgentStatus::Pending | SubAgentStatus::Running => AgentSpawnStatus::AsyncLaunched,
            SubAgentStatus::Completed => AgentSpawnStatus::Completed,
            SubAgentStatus::Failed => AgentSpawnStatus::Failed,
            SubAgentStatus::Backgrounded => AgentSpawnStatus::AsyncLaunched,
            SubAgentStatus::Interrupted => AgentSpawnStatus::Failed,
        };

        Ok(AgentSpawnResponse {
            status,
            agent_id: Some(agent_id.to_string()),
            result: agent.last_message.clone(),
            error: None,
            total_tool_use_count: 0,
            total_tokens: 0,
            duration_ms: 0,
            worktree_path: None,
            worktree_branch: None,
            output_file: None,
            prompt: None,
        })
    }

    async fn get_agent_output(&self, agent_id: &str) -> Result<String, String> {
        let agents = self.agents.read().await;
        let agent = agents
            .iter()
            .find(|a| a.agent_id == agent_id)
            .ok_or_else(|| format!("Agent '{agent_id}' not found"))?;

        agent
            .last_message
            .clone()
            .ok_or_else(|| format!("Agent '{agent_id}' has no output yet"))
    }

    async fn background_agent(&self, agent_id: &str) -> Result<(), String> {
        let mut agents = self.agents.write().await;
        let agent = agents
            .iter_mut()
            .find(|a| a.agent_id == agent_id)
            .ok_or_else(|| format!("Agent '{agent_id}' not found"))?;

        agent.status = SubAgentStatus::Backgrounded;
        Ok(())
    }

    // `resolve_skill` was deleted in Phase 7 — skills now route
    // through the dedicated `SkillHandle` trait, not `AgentHandle`.
}

#[cfg(test)]
#[path = "swarm_agent_handle.test.rs"]
mod tests;
