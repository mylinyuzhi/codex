//! `AgentHandle` implementation bridging the tool layer → coordinator
//! orchestration.
//!
//! TS: `AgentTool.call()` dispatches to `spawnMultiAgent` / `runAgent` /
//! `forkSubagent` based on input parameters. This module provides the
//! Rust equivalent by implementing
//! [`coco_tool_runtime::AgentHandle`] atop the coordinator's runner +
//! mailbox + team-file modules.
//!
//! Module layout (split from a single 854-LoC file):
//! - `mod.rs` (this file) — struct, accessors, setters, AgentHandle trait
//!   impl, teammate spawn.
//! - `spawn.rs` — sync + background subagent dispatch + worktree
//!   isolation + `AgentQueryConfig` construction.
//! - `handoff.rs` — 2-stage handoff safety classifier and post-spawn
//!   AgentSummary.
//! - `resume.rs` — TS-aligned background-spawn resume from JSONL
//!   transcript + sidecar metadata.

mod handoff;
mod resume;
mod spawn;
mod teammate_engine;

pub use teammate_engine::TeammateExecutionAdapter;
pub use teammate_engine::into_execution_engine;

use std::sync::Arc;

use coco_tool_runtime::AgentHandle;
use coco_tool_runtime::AgentSpawnRequest;
use coco_tool_runtime::AgentSpawnResponse;
use coco_tool_runtime::AgentSpawnStatus;
use tokio::sync::RwLock;

use coco_types::SubAgentState;
use coco_types::SubAgentStatus;

use crate::constants::TEAM_LEAD_NAME;
use crate::identity::get_agent_name;
use crate::identity::get_team_name;
use crate::mailbox::TeammateMessage;
use crate::mailbox::write_to_mailbox;
use crate::runner::InProcessAgentRunner;
use crate::runner::SpawnConfig;
use crate::team_file::write_team_file;
use crate::teammate::resolve_teammate_model;
use crate::types::TeamManager;

/// AgentHandle implementation backed by the swarm infrastructure.
///
/// The bridge between the tool layer (AgentTool) and the state layer
/// (swarm modules). Routes spawn requests to the appropriate backend
/// (in-process, tmux, iTerm2) and manages agent lifecycle.
pub struct SwarmAgentHandle {
    runner: Arc<InProcessAgentRunner>,
    team_manager: Arc<RwLock<Option<TeamManager>>>,
    agents: Arc<RwLock<Vec<SubAgentState>>>,
    /// Drives the LLM loop for sync subagents. `None` ⇒ sync spawn fails
    /// fast with a "no engine configured" error rather than silently
    /// succeeding with placeholder output. Install via
    /// [`Self::set_execution_engine`] at session bootstrap.
    execution_engine: Option<coco_tool_runtime::AgentQueryEngineRef>,
    /// `None` ⇒ worktree-isolation requests fail fast. The CLI resolves
    /// the canonical git root at bootstrap and installs this so subagents
    /// spawned with worktree isolation always land in
    /// `.claude/worktrees/agent-<slug>` under the main repo.
    worktree_manager: Option<Arc<crate::worktree::AgentWorktreeManager>>,
    /// Drives the 2-stage handoff safety classifier. `None` ⇒ classifier
    /// is a no-op (fail-open, matches TS).
    side_query: Option<coco_tool_runtime::SideQueryHandle>,
    /// Background-task registry (`AgentTaskRegistry`). Same `Arc` as the
    /// engine's `TaskHandle` slot so a bg AgentTool spawn registered here
    /// is addressable through `Task*` tools the model invokes later.
    /// `None` ⇒ bg spawns still run but aren't model-addressable.
    task_registry: Option<coco_tool_runtime::AgentTaskRegistryRef>,
    /// Per-agent transcript persistence. When installed, bg AgentTool
    /// spawns write `AgentSpawnMetadata` at registration and the full
    /// message history to `agent-<id>.jsonl` on completion. `resume_agent`
    /// reads both files to rehydrate a stopped spawn. `None` ⇒ resume is
    /// unsupported in this session.
    transcript_store: Option<coco_tool_runtime::AgentTranscriptStoreRef>,
    cwd: String,
    /// Atomic snapshot of resolved providers + role mappings. Reading the
    /// Main role through `runtime_config` means a parent that hot-reloaded
    /// into a new model picks up the updated value on the next subagent
    /// spawn. Each turn-boundary publishes a fresh `Arc<RuntimeConfig>`
    /// (`coco_config::SettingsWatcher`); callers update via
    /// [`Self::set_runtime_config`].
    runtime_config: Arc<coco_config::RuntimeConfig>,
    /// Drives the in-process teammate runner-loop after `spawn_teammate`
    /// registers a teammate. `None` ⇒ teammate spawns succeed at
    /// registration but never execute LLM turns (the prior behaviour
    /// before Gap C was fixed). Wire via
    /// [`Self::set_teammate_execution_engine`] at session bootstrap.
    teammate_engine: Option<Arc<dyn crate::runner_loop::AgentExecutionEngine>>,
    /// Auto-compact threshold (token count) used by the teammate
    /// runner-loop. Defaults to 100k; override via
    /// [`Self::set_teammate_auto_compact_threshold`] so the leader's
    /// resolved `CompactConfig` flows through to teammates.
    teammate_auto_compact_threshold: i64,
    /// In-process teammate task-state mirrors. One per registered
    /// teammate keyed by `agent_id`. The runner-loop writes its
    /// progress here; UI / panel widgets read snapshots through
    /// [`Self::teammate_task_state`].
    teammate_task_states: Arc<
        tokio::sync::RwLock<
            std::collections::HashMap<
                String,
                Arc<tokio::sync::RwLock<crate::task::InProcessTeammateTaskState>>,
            >,
        >,
    >,
    /// Base system prompt (the main agent's full system prompt) used
    /// as the teammate's base in `build_teammate_system_prompt`. The
    /// runner-loop appends `TEAMMATE_PROMPT_ADDENDUM` to whatever this
    /// holds. `None` ⇒ teammates see only the addendum (TS parity gap
    /// fixed by [`Self::set_teammate_base_system_prompt`]). TS:
    /// `inProcessRunner.ts` builds the teammate's prompt by composing
    /// the main `getSystemPrompt(...)` with the team addendum; the
    /// CLI mirrors that by passing `runtime.current_engine_config()
    /// .system_prompt` here at bootstrap.
    teammate_base_system_prompt: Arc<tokio::sync::RwLock<Option<String>>>,
    /// Hook registry used to fire `SubagentStart` / `SubagentStop`
    /// around subagent execution. `None` ⇒ hooks don't fire (the
    /// pre-fix behaviour). TS parity: `runAgent.ts:530-555` collects
    /// `SubagentStart.additionalContexts` and injects them as a
    /// hook_additional_context attachment into the child's first
    /// user message; the stop hook fires at completion / error /
    /// cancel.
    hook_registry: Option<Arc<coco_hooks::HookRegistry>>,
    /// Skill handle used to preload skill bodies declared in agent
    /// frontmatter (`skills: [foo, bar]`). At spawn time the handle's
    /// `read_skill_body(name)` is called for each entry and the
    /// concatenated bodies are prepended to the child's first user
    /// message. `None` ⇒ frontmatter skills are silently ignored
    /// (logged at debug). TS parity: `runAgent.ts:577-645`.
    skill_handle: Option<coco_tool_runtime::SkillHandleRef>,
    /// MCP handle used to register agent-specific MCP servers
    /// declared in frontmatter (`mcpServers: [github, {slack: {...}}]`).
    /// At spawn: inline entries get `add_dynamic_server`'d; at stop:
    /// they're `remove_dynamic_server`'d. String-ref entries are
    /// pre-registered on the handle (no spawn-time mutation). `None`
    /// ⇒ inline entries are silently dropped (logged at debug).
    /// TS parity: `runAgent.ts:95-218 initializeAgentMcpServers`.
    mcp_handle: Option<coco_tool_runtime::McpHandleRef>,
    /// Per-agent dynamic MCP server tracking. Populated when an
    /// inline server gets stood up at spawn time; consulted at
    /// SubagentStop to teardown only the agent's own servers
    /// (string-ref entries point at parent-shared connections that
    /// must NOT be torn down — TS `newlyCreatedClients` guard).
    dynamic_mcp_servers: Arc<tokio::sync::RwLock<std::collections::HashMap<String, Vec<String>>>>,
}

impl SwarmAgentHandle {
    pub fn new(
        runner: Arc<InProcessAgentRunner>,
        team_manager: Arc<RwLock<Option<TeamManager>>>,
        cwd: String,
        runtime_config: Arc<coco_config::RuntimeConfig>,
    ) -> Self {
        Self {
            runner,
            team_manager,
            agents: Arc::new(RwLock::new(Vec::new())),
            execution_engine: None,
            worktree_manager: None,
            side_query: None,
            task_registry: None,
            transcript_store: None,
            cwd,
            runtime_config,
            teammate_engine: None,
            teammate_auto_compact_threshold: 100_000,
            teammate_task_states: Arc::new(tokio::sync::RwLock::new(
                std::collections::HashMap::new(),
            )),
            teammate_base_system_prompt: Arc::new(tokio::sync::RwLock::new(None)),
            hook_registry: None,
            skill_handle: None,
            mcp_handle: None,
            dynamic_mcp_servers: Arc::new(tokio::sync::RwLock::new(
                std::collections::HashMap::new(),
            )),
        }
    }

    /// Install the MCP handle used for per-agent dynamic server
    /// registration (inline `mcpServers: [{name: config}]`).
    pub fn set_mcp_handle(&mut self, handle: coco_tool_runtime::McpHandleRef) {
        self.mcp_handle = Some(handle);
    }

    pub(crate) fn mcp_handle(&self) -> Option<&coco_tool_runtime::McpHandleRef> {
        self.mcp_handle.as_ref()
    }

    pub(crate) fn dynamic_mcp_servers(
        &self,
    ) -> &Arc<tokio::sync::RwLock<std::collections::HashMap<String, Vec<String>>>> {
        &self.dynamic_mcp_servers
    }

    /// Install a skill handle for frontmatter-skill preload at spawn
    /// time. The handle's `read_skill_body(name)` resolves each name
    /// declared in `definition.skills` to its prompt body, which then
    /// gets prepended to the child's first user message.
    pub fn set_skill_handle(&mut self, handle: coco_tool_runtime::SkillHandleRef) {
        self.skill_handle = Some(handle);
    }

    pub(crate) fn skill_handle(&self) -> Option<&coco_tool_runtime::SkillHandleRef> {
        self.skill_handle.as_ref()
    }

    /// Install the hook registry used to fire `SubagentStart` /
    /// `SubagentStop` around subagent execution. Without this,
    /// user-defined hooks for those events silently never run for
    /// spawned subagents (a TS parity gap pre-fix).
    pub fn set_hook_registry(&mut self, registry: Arc<coco_hooks::HookRegistry>) {
        self.hook_registry = Some(registry);
    }

    pub(crate) fn hook_registry(&self) -> Option<&Arc<coco_hooks::HookRegistry>> {
        self.hook_registry.as_ref()
    }

    /// Set the base system prompt teammates receive (composed with
    /// `TEAMMATE_PROMPT_ADDENDUM` by the runner-loop). The CLI wires
    /// this from the leader's resolved system prompt so teammates
    /// inherit CLAUDE.md + env-context + memory blocks the same way
    /// the leader sees them.
    pub async fn set_teammate_base_system_prompt(&self, prompt: String) {
        let mut slot = self.teammate_base_system_prompt.write().await;
        *slot = Some(prompt);
    }

    /// Install the engine that drives in-process teammate runner-loops.
    /// Without this, `spawn_teammate` registers the teammate but no
    /// LLM turn ever runs (Gap C). Production wiring lives in
    /// `app/cli/agent_handle_factory::install_agent_team`.
    pub fn set_teammate_execution_engine(
        &mut self,
        engine: Arc<dyn crate::runner_loop::AgentExecutionEngine>,
    ) {
        self.teammate_engine = Some(engine);
    }

    /// Override the teammate runner-loop's auto-compact threshold. The
    /// CLI wires this from `runtime_config.compact` so user-tuned
    /// thresholds flow through to teammates.
    pub fn set_teammate_auto_compact_threshold(&mut self, threshold: i64) {
        self.teammate_auto_compact_threshold = threshold;
    }

    /// Snapshot a teammate's task-state. UI panels use this to render
    /// per-teammate spinner verbs / message log without taking a write
    /// lock on the runner-loop's task_state.
    pub async fn teammate_task_state(
        &self,
        agent_id: &str,
    ) -> Option<Arc<tokio::sync::RwLock<crate::task::InProcessTeammateTaskState>>> {
        self.teammate_task_states
            .read()
            .await
            .get(agent_id)
            .cloned()
    }

    pub fn set_runtime_config(&mut self, runtime_config: Arc<coco_config::RuntimeConfig>) {
        self.runtime_config = runtime_config;
    }

    /// Resolve the Main role's `model_id` through the current
    /// `RuntimeConfig`. Empty string when the runtime has no Main —
    /// `RuntimeConfig::resolve_model_roles` shouldn't allow that, but
    /// defending the boundary is cheap.
    pub(crate) fn current_main_model_id(&self) -> String {
        self.runtime_config
            .model_roles
            .get(coco_types::ModelRole::Main)
            .map(|spec| spec.model_id.clone())
            .unwrap_or_default()
    }

    pub(crate) fn agents(&self) -> &Arc<RwLock<Vec<SubAgentState>>> {
        &self.agents
    }

    pub(crate) fn execution_engine(&self) -> Option<coco_tool_runtime::AgentQueryEngineRef> {
        self.execution_engine.clone()
    }

    pub(crate) fn worktree_manager(&self) -> Option<&Arc<crate::worktree::AgentWorktreeManager>> {
        self.worktree_manager.as_ref()
    }

    pub(crate) fn side_query(&self) -> Option<&coco_tool_runtime::SideQueryHandle> {
        self.side_query.as_ref()
    }

    pub(crate) fn task_registry(&self) -> Option<&coco_tool_runtime::AgentTaskRegistryRef> {
        self.task_registry.as_ref()
    }

    /// Install the AgentTaskRegistry that the bg AgentTool path uses to
    /// register spawns. Wire the same Arc that the engine's `TaskHandle`
    /// slot reads so model `Task*` calls and AgentTool spawns share one
    /// store.
    pub fn set_task_registry(&mut self, registry: coco_tool_runtime::AgentTaskRegistryRef) {
        self.task_registry = Some(registry);
    }

    /// Required for `resume_agent`; optional for fresh spawns (the bg path
    /// silently skips persistence when absent).
    pub fn set_transcript_store(&mut self, store: coco_tool_runtime::AgentTranscriptStoreRef) {
        self.transcript_store = Some(store);
    }

    pub(crate) fn transcript_store(&self) -> Option<&coco_tool_runtime::AgentTranscriptStoreRef> {
        self.transcript_store.as_ref()
    }

    pub fn set_execution_engine(&mut self, engine: coco_tool_runtime::AgentQueryEngineRef) {
        self.execution_engine = Some(engine);
    }

    /// Install an [`AgentWorktreeManager`](crate::worktree::AgentWorktreeManager)
    /// so subagents spawned with `isolation: "worktree"` get a real git
    /// worktree + cwd_override + cleanup-on-success.
    pub fn set_worktree_manager(&mut self, manager: Arc<crate::worktree::AgentWorktreeManager>) {
        self.worktree_manager = Some(manager);
    }

    /// Without a side-query bridge the classifier silently passes
    /// everything through (fail-open).
    pub fn set_side_query(&mut self, handle: coco_tool_runtime::SideQueryHandle) {
        self.side_query = Some(handle);
    }

    fn is_teammate_spawn(request: &AgentSpawnRequest) -> bool {
        request.name.is_some() && request.team_name.is_some()
    }

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

        let main_model_id = self.current_main_model_id();
        let model = resolve_teammate_model(request.model.as_deref(), Some(&main_model_id), None);

        // Prefer per-spawn `initial_prompt` override; otherwise fall back to
        // the leader's full system prompt so the teammate sees the same
        // CLAUDE.md + env-context + memory blocks the leader does. The
        // runner-loop then composes this with `TEAMMATE_PROMPT_ADDENDUM`.
        // Pre-fix: teammates ran with ONLY the addendum (the leader's
        // system prompt was discarded), which is a TS parity gap with
        // `inProcessRunner.ts`.
        let teammate_system_prompt = if request.initial_prompt.is_some() {
            request.initial_prompt.clone()
        } else {
            self.teammate_base_system_prompt.read().await.clone()
        };

        // Persistent round-robin assignment so the same teammate gets
        // the same color across spawns within a session (TS
        // `teammateLayoutManager.ts:assignTeammateColor`). The agent_id
        // namespacing keeps the assignment scoped per teammate identity.
        let agent_id_for_color = format!("{name}@{team_name}");
        let color = crate::pane::layout::assign_teammate_color(&agent_id_for_color);

        let config = SpawnConfig {
            name: name.to_string(),
            team_name: team_name.to_string(),
            prompt: request.prompt.clone(),
            color: Some(color.as_str().to_string()),
            plan_mode_required: request.mode.as_deref().is_some_and(|m| m == "plan"),
            model: Some(model.clone()),
            working_dir: request.cwd.as_ref().map(|p| p.display().to_string()),
            system_prompt: teammate_system_prompt,
            allowed_tools: Vec::new(),
            allow_permission_prompts: true,
            effort: request.effort.clone(),
            use_exact_tools: request.use_exact_tools,
            isolation: coco_types::AgentIsolation::None,
            memory_scope: None,
            mcp_servers: request.mcp_servers.clone(),
            disallowed_tools: request.disallowed_tools.clone(),
            max_turns: request.max_turns,
        };

        let spawn_result = self.runner.register_agent(config.clone()).await;

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
                ..Default::default()
            });
        }

        let state = SubAgentState {
            agent_id: spawn_result.agent_id.clone(),
            name: name.to_string(),
            status: SubAgentStatus::Running,
            turns: 0,
            model: Some(model.clone()),
            working_dir: request.cwd.as_ref().map(|p| p.display().to_string()),
            last_message: None,
        };
        self.agents.write().await.push(state);

        // ── Gap C fix — actually start the teammate's LLM loop ──
        //
        // Pre-fix: production code stopped at `register_agent`; the
        // teammate sat in the runner's `agents` map forever and never
        // ran a single turn (TS `InProcessBackend.spawn` calls
        // `startInProcessTeammate` after `spawnInProcessTeammate`
        // succeeds — `utils/swarm/backends/InProcessBackend.ts:99-129`).
        //
        // Now: when a teammate execution engine is installed, build the
        // runner config + task-state mirror, kick off the runner_loop
        // in a detached task, and wire its JoinHandle into the runner
        // so `wait_for_completion` / `cancel_agent` work.
        if let Some(engine) = self.teammate_engine.clone() {
            let cancelled = self
                .runner
                .get_context(&spawn_result.agent_id)
                .await
                .map(|ctx| ctx.cancelled)
                .unwrap_or_else(|| Arc::new(std::sync::atomic::AtomicBool::new(false)));

            let identity = crate::types::TeammateIdentity {
                agent_id: spawn_result.agent_id.clone(),
                agent_name: name.to_string(),
                team_name: team_name.to_string(),
                color: config
                    .color
                    .as_deref()
                    .and_then(|c| c.parse::<coco_types::AgentColorName>().ok()),
                plan_mode_required: config.plan_mode_required,
            };

            let task_state = Arc::new(tokio::sync::RwLock::new(
                crate::task::InProcessTeammateTaskState::new(
                    format!("task-{}", spawn_result.agent_id),
                    identity.clone(),
                    request.prompt.clone(),
                ),
            ));
            self.teammate_task_states
                .write()
                .await
                .insert(spawn_result.agent_id.clone(), task_state.clone());

            // Wire TeammateIdle hook context. SwarmAgentHandle owns
            // the registry; we synthesize the orchestration context
            // here using the same helper subagent spawning uses
            // (`spawn::hook_ctx_for_subagent`).
            let teammate_orchestration_ctx = self.hook_registry().map(|_| {
                crate::agent_handle::spawn::hook_ctx_for_subagent(
                    &self.cwd,
                    Some(&spawn_result.agent_id),
                    request.subagent_type.as_deref(),
                )
            });
            let runner_config = crate::runner_loop::InProcessRunnerConfig {
                identity,
                task_id: format!("task-{}", spawn_result.agent_id),
                prompt: request.prompt.clone(),
                model: config.model.clone(),
                system_prompt: config.system_prompt.clone(),
                system_prompt_mode: crate::pane::SystemPromptMode::Default,
                allowed_tools: config.allowed_tools.clone(),
                allow_permission_prompts: config.allow_permission_prompts,
                max_turns: config.max_turns,
                cancelled,
                auto_compact_threshold: self.teammate_auto_compact_threshold,
                bypass_permissions_available: false,
                features: request.features.clone(),
                tool_overrides: request.tool_overrides.clone(),
                parent_tool_filter: request.parent_tool_filter.clone(),
                plan_mode_required: config.plan_mode_required,
                hooks: self.hook_registry().cloned(),
                orchestration_ctx: teammate_orchestration_ctx,
            };

            let join =
                crate::runner_loop::start_in_process_teammate(runner_config, engine, task_state);
            self.runner.start_agent(&spawn_result.agent_id, join).await;
        } else {
            tracing::warn!(
                agent_id = %spawn_result.agent_id,
                "teammate registered without execution engine — no LLM turns will run; \
                 install via SwarmAgentHandle::set_teammate_execution_engine at session bootstrap"
            );
        }

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
            ..Default::default()
        })
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

    async fn resume_agent(
        &self,
        agent_id: &str,
        prompt: &str,
        session_id: &str,
    ) -> Result<AgentSpawnResponse, String> {
        // Inherent `resume_agent` lives in `resume.rs`; delegate so the
        // trait surface is satisfied without duplicating the body.
        SwarmAgentHandle::resume_agent(self, agent_id, prompt.to_string(), session_id.to_string())
            .await
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
                    color: crate::pane::layout::get_teammate_color(&from)
                        .map(|c| c.as_str().to_string()),
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
            color: crate::pane::layout::get_teammate_color(&from).map(|c| c.as_str().to_string()),
            summary: None,
        };

        write_to_mailbox(to, message, &team_name)
            .map_err(|e| format!("Failed to send message to '{to}': {e}"))?;

        Ok(format!("Message sent from '{from}' to '{to}'"))
    }

    async fn create_team(&self, name: &str) -> Result<String, String> {
        use crate::types::TeamFile;
        use crate::types::TeamMember;

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
                model: Some(self.current_main_model_id()),
                prompt: None,
                color: None,
                plan_mode_required: false,
                joined_at: chrono::Utc::now().timestamp(),
                tmux_pane_id: String::new(),
                cwd: self.cwd.clone(),
                worktree_path: None,
                session_id: None,
                subscriptions: Vec::new(),
                backend_type: Some(crate::types::BackendType::InProcess),
                is_active: true,
                mode: None,
            }],
        };

        write_team_file(name, &team_file)
            .map_err(|e| format!("Failed to create team '{name}': {e}"))?;

        {
            let mut tm = self.team_manager.write().await;
            *tm = Some(TeamManager::new(name.to_string(), team_file.clone()));
        }

        Ok(format!("Team '{name}' created with lead '{lead_agent_id}'"))
    }

    async fn delete_team(&self) -> Result<String, String> {
        // TS `TeamDeleteTool.ts:74` reads `appState.teamContext?.teamName`
        // — when no team is active it returns success with a "nothing to
        // clean up" message. Mirror that idempotency.
        let name = {
            let tm = self.team_manager.read().await;
            tm.as_ref().map(|m| m.team_name().to_string())
        };

        let Some(name) = name else {
            return Ok("No team name found, nothing to clean up".into());
        };

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

        // TS: `cleanupTeamDirectories(teamName)`.
        crate::team_file::cleanup_team_directories(&name)
            .map_err(|e| format!("Failed to delete team '{name}': {e}"))?;

        // TS: `unregisterTeamForSessionCleanup(teamName)`.
        crate::team_file::unregister_team_for_session_cleanup(&name);

        // TS: `clearTeammateColors()`.
        crate::pane::layout::clear_teammate_colors();

        {
            let mut tm = self.team_manager.write().await;
            *tm = None;
        }

        // TS parity gap: `clearLeaderTeamName()` and the app-state reset
        // (`teamContext: undefined`, `inbox.messages: []`) live outside the
        // coordinator crate. The CLI / state owner observes deletion via
        // the returned message and performs those resets. Tracked in
        // `docs/coco-rs/agentteam-architecture.md` "delete_team parity".

        Ok(format!(
            "Cleaned up directories and worktrees for team \"{name}\""
        ))
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
            ..Default::default()
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
}

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;
