//! Standalone-subagent spawn dispatch.
//!
//! Owns:
//! - [`SwarmAgentHandle::spawn_subagent`] — registers the subagent on the
//!   agents list, applies worktree isolation, resolves the spawn-time
//!   identity (`coco_subagent::resolve_subagent_selection`), builds the
//!   `AgentQueryConfig`, dispatches sync vs background, and updates
//!   tracked status.
//! - [`spawn_failed`] — tiny shorthand for sync-path early failures.
//!
//! Pure-logic helpers live in `core/subagent`. Handoff classification and
//! AgentSummary live in `super::handoff`. Background-spawn resume lives in
//! `super::resume`.

use std::sync::Arc;
use std::time::Instant;

use coco_tool_runtime::AgentSpawnRequest;
use coco_tool_runtime::AgentSpawnResponse;
use coco_tool_runtime::AgentSpawnStatus;
use coco_types::SubAgentState;
use coco_types::SubAgentStatus;

use super::SwarmAgentHandle;

/// Build an `OrchestrationContext` for subagent-spawn hook firing.
///
/// Standalone so the bg path can build it inside a detached task
/// without holding `&SwarmAgentHandle`. The hook system's full
/// feature surface (project_dir resolution, per-session disable
/// flag, attachment emitter) is approximated here — subagent-spawn
/// hooks don't need the same wire-up the parent's per-turn hooks do.
pub(super) fn hook_ctx_for_cwd(cwd: &str) -> coco_hooks::orchestration::OrchestrationContext {
    hook_ctx_for_subagent(cwd, None, None)
}

/// Variant that stamps `agent_id` / `agent_type` onto the
/// [`coco_hooks::orchestration::OrchestrationContext`] so every fired
/// hook's `BaseHookInput` carries the subagent identity (TS parity:
/// `createBaseHookInput()` in `utils/hooks.ts:301-328`).
pub(super) fn hook_ctx_for_subagent(
    cwd: &str,
    agent_id: Option<&str>,
    agent_type: Option<&str>,
) -> coco_hooks::orchestration::OrchestrationContext {
    coco_hooks::orchestration::OrchestrationContext {
        session_id: String::new(),
        cwd: std::path::PathBuf::from(cwd),
        project_dir: Some(std::path::PathBuf::from(cwd)),
        permission_mode: None,
        transcript_path: None,
        agent_id: agent_id.map(str::to_string),
        agent_type: agent_type.map(str::to_string),
        cancel: tokio_util::sync::CancellationToken::new(),
        disable_all_hooks: false,
        allow_managed_hooks_only: false,
        attachment_emitter: coco_messages::AttachmentEmitter::noop(),
        // Subagent-spawn hooks aren't reminder-bearing in TS — their
        // output flows back into the spawn's initial-user message via
        // `additional_contexts`, not the per-turn reminder pipeline.
        sync_event_sink: None,
        http_url_allowlist: None,
        http_env_var_policy: None,
        async_registry: None,
        llm_handle: None,
        workspace_trust_accepted: None,
    }
}

/// Free-function helper for `WorktreeCreate` (TS
/// `executeWorktreeCreateHook(slug)` at `worktree.ts:716/913/1262`).
/// Coco-rs does git creation directly (`AgentWorktreeManager::create_for`),
/// so the hook is observe-only — non-zero exit codes are logged but
/// don't roll back the worktree.
pub(super) async fn fire_worktree_create_hook(
    registry: Option<Arc<coco_hooks::HookRegistry>>,
    cwd: &str,
    name: &str,
) {
    let Some(registry) = registry else { return };
    let ctx = hook_ctx_for_cwd(cwd);
    if let Err(e) = coco_hooks::orchestration::execute_worktree_create(&registry, &ctx, name).await
    {
        tracing::warn!(error = %e, %name, "WorktreeCreate hook firing failed");
    }
}

/// Free-function helper for `WorktreeRemove` (TS
/// `executeWorktreeRemoveHook(path)` at `worktree.ts:827/968`). Fired
/// only when `cleanup_if_unchanged` actually removed the worktree;
/// `Kept` outcomes preserve the user's work so the remove notification
/// is suppressed (TS parity).
pub(super) async fn fire_worktree_remove_hook(
    registry: Option<Arc<coco_hooks::HookRegistry>>,
    cwd: &str,
    worktree_path: &str,
) {
    let Some(registry) = registry else { return };
    let ctx = hook_ctx_for_cwd(cwd);
    if let Err(e) =
        coco_hooks::orchestration::execute_worktree_remove(&registry, &ctx, worktree_path).await
    {
        tracing::warn!(error = %e, %worktree_path, "WorktreeRemove hook firing failed");
    }
}

/// Free-function variant of `fire_subagent_start_hook` for the bg
/// path. Same fail-open semantics.
pub(super) async fn fire_subagent_start_for_task(
    registry: Option<Arc<coco_hooks::HookRegistry>>,
    cwd: &str,
    agent_id: &str,
    agent_type: &str,
    prompt: &str,
) -> String {
    let Some(registry) = registry else {
        return prompt.to_string();
    };
    let ctx = hook_ctx_for_subagent(cwd, Some(agent_id), Some(agent_type));
    match coco_hooks::orchestration::execute_subagent_start(&registry, &ctx, agent_type, agent_id)
        .await
    {
        Ok(result) if !result.additional_contexts.is_empty() => {
            let blocks = result
                .additional_contexts
                .iter()
                .map(|c| format!("<hook-additional-context>\n{c}\n</hook-additional-context>"))
                .collect::<Vec<_>>()
                .join("\n\n");
            format!("{blocks}\n\n{prompt}")
        }
        Ok(_) => prompt.to_string(),
        Err(e) => {
            tracing::warn!(
                error = %e,
                %agent_id,
                %agent_type,
                "SubagentStart (bg) hook firing failed; proceeding without injected context"
            );
            prompt.to_string()
        }
    }
}

/// Free-function variant of `fire_subagent_stop_hook` for the bg
/// path. Errors logged + swallowed.
pub(super) async fn fire_subagent_stop_for_task(
    registry: Option<Arc<coco_hooks::HookRegistry>>,
    cwd: &str,
    agent_id: &str,
    agent_type: &str,
    transcript_path: Option<&str>,
) {
    let Some(registry) = registry else { return };
    let ctx = hook_ctx_for_subagent(cwd, Some(agent_id), Some(agent_type));
    if let Err(e) = coco_hooks::orchestration::execute_subagent_stop(
        &registry,
        &ctx,
        /*stop_hook_active*/ false,
        agent_type,
        agent_id,
        transcript_path.unwrap_or(""),
        /*last_assistant_message*/ None,
    )
    .await
    {
        tracing::warn!(
            error = %e,
            %agent_id,
            %agent_type,
            "SubagentStop (bg) hook firing failed"
        );
    }
}

impl SwarmAgentHandle {
    /// Build an `OrchestrationContext` keyed off the handle's cwd.
    /// Thin wrapper around [`hook_ctx_for_cwd`] for sync-path callers
    /// that already hold `&self`.
    fn hook_orchestration_context(&self) -> coco_hooks::orchestration::OrchestrationContext {
        hook_ctx_for_cwd(&self.cwd)
    }

    /// Fire SubagentStart hooks and prepend the aggregated
    /// `additional_contexts` (each wrapped in a `<hook-additional-context>`
    /// XML block) to `prompt`. Returns `(effective_prompt, raw_result)`.
    /// Fail-open: any hook error is logged and the original prompt is
    /// returned unchanged.
    pub(super) async fn fire_subagent_start_hook(
        &self,
        agent_id: &str,
        agent_type: &str,
        prompt: &str,
    ) -> (
        String,
        Option<coco_hooks::orchestration::AggregatedHookResult>,
    ) {
        let Some(registry) = self.hook_registry().cloned() else {
            return (prompt.to_string(), None);
        };
        let ctx = self.hook_orchestration_context();
        match coco_hooks::orchestration::execute_subagent_start(
            &registry, &ctx, agent_type, agent_id,
        )
        .await
        {
            Ok(result) => {
                if result.additional_contexts.is_empty() {
                    (prompt.to_string(), Some(result))
                } else {
                    let blocks = result
                        .additional_contexts
                        .iter()
                        .map(|c| {
                            format!("<hook-additional-context>\n{c}\n</hook-additional-context>")
                        })
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    (format!("{blocks}\n\n{prompt}"), Some(result))
                }
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    %agent_id,
                    %agent_type,
                    "SubagentStart hook firing failed; proceeding without injected context"
                );
                (prompt.to_string(), None)
            }
        }
    }

    /// Register frontmatter hooks scoped to this spawn's agent_id.
    /// Returns `true` if any hooks were registered (caller should
    /// arrange `clear_frontmatter_hooks(agent_id)` at SubagentStop).
    /// TS parity: `runAgent.ts:564-575`
    /// `registerFrontmatterHooks(setAppState, agentId, definition.hooks, ...)`.
    ///
    /// The `def.hooks` field is `serde_json::Value` because TS's
    /// shape is an event-keyed map of arrays — same as
    /// `Settings.hooks`. We parse via `coco_hooks::load_hooks_from_config`
    /// and stamp `HookScope::Session` (TS treats agent-scoped hooks
    /// as session-priority because they're programmatic, not config).
    pub(super) fn register_frontmatter_hooks(
        &self,
        agent_id: &str,
        definition: Option<&coco_types::AgentDefinition>,
    ) -> bool {
        let Some(def) = definition else {
            return false;
        };
        if def.hooks.is_null() {
            return false;
        }
        let Some(registry) = self.hook_registry() else {
            tracing::debug!(
                agent_type = %def.agent_type,
                "agent declares frontmatter hooks but no HookRegistry is installed; skipping"
            );
            return false;
        };
        match coco_hooks::load_hooks_from_config(&def.hooks, coco_types::HookScope::Session) {
            Ok(hooks) if !hooks.is_empty() => {
                let count = hooks.len();
                // `is_agent: true` — subagent termination fires
                // SubagentStop, not Stop, so frontmatter Stop hooks
                // need rewriting (parity with TS
                // registerFrontmatterHooks.ts:38-45).
                registry.register_for_agent(agent_id.to_string(), hooks, /*is_agent*/ true);
                tracing::debug!(
                    agent_type = %def.agent_type,
                    %agent_id,
                    count,
                    "registered frontmatter hooks for agent scope"
                );
                true
            }
            Ok(_) => false,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    agent_type = %def.agent_type,
                    "failed to parse frontmatter hooks; skipping"
                );
                false
            }
        }
    }

    /// Symmetrical cleanup: drop the per-agent hook bucket.
    /// TS parity: `clearSessionHooks(setAppState, agentId)`.
    ///
    /// **W6.2 full**: kept (rather than deleted as "dead code")
    /// because the bg path's `tokio::spawn` closure uses
    /// `registry.clear_agent_scope` directly via a cloned Arc, and
    /// the sync path now does the same in its detached engine task.
    /// Both bypass this `&self` wrapper. Marking `#[allow(dead_code)]`
    /// keeps the helper available for future callers (e.g. error
    /// paths that abandon a spawn before the engine task runs).
    #[allow(dead_code)]
    pub(super) fn clear_frontmatter_hooks(&self, agent_id: &str) {
        if let Some(registry) = self.hook_registry() {
            registry.clear_agent_scope(agent_id);
        }
    }

    /// Initialise per-agent MCP servers from `def.mcp_servers`. For
    /// each spec:
    /// - `Name(s)` (string-ref): no mutation — we trust the parent's
    ///   pre-existing connection. The model just sees the server's
    ///   tools through the inherited `McpHandle`.
    /// - `Inline(config)`: call `add_dynamic_server(name, config)` on
    ///   the handle. Track the new name under `agent_id` so
    ///   `cleanup_per_agent_mcp` removes only the newly-created
    ///   ones. TS parity: `runAgent.ts:135-191` distinguishes
    ///   `isNewlyCreated` so cleanup doesn't touch shared servers.
    ///
    /// Any inline-add failure logs at warn and continues — TS
    /// behaviour: a failed server logs and the agent runs without
    /// that one.
    pub(super) async fn initialize_per_agent_mcp(
        &self,
        agent_id: &str,
        definition: Option<&coco_types::AgentDefinition>,
    ) {
        let Some(def) = definition else {
            return;
        };
        if def.mcp_servers.is_empty() {
            return;
        }
        let Some(handle) = self.mcp_handle() else {
            tracing::debug!(
                agent_type = %def.agent_type,
                "agent declares mcpServers but no McpHandle is installed; skipping per-agent MCP init"
            );
            return;
        };

        let mut newly_created: Vec<String> = Vec::new();
        for spec in &def.mcp_servers {
            match spec {
                coco_types::AgentMcpServerSpec::Name(_) => {
                    // String-ref → relies on parent's connection. No
                    // dynamic mutation; nothing to track for cleanup.
                }
                coco_types::AgentMcpServerSpec::Inline(map) => {
                    let Some((name, config)) = map.iter().next() else {
                        continue;
                    };
                    match handle.add_dynamic_server(name, config.clone()).await {
                        Ok(()) => {
                            tracing::debug!(
                                agent_type = %def.agent_type,
                                %agent_id,
                                server = %name,
                                "registered dynamic agent MCP server"
                            );
                            newly_created.push(name.clone());
                        }
                        Err(e) => {
                            tracing::warn!(
                                error = %e,
                                agent_type = %def.agent_type,
                                %agent_id,
                                server = %name,
                                "failed to register dynamic agent MCP server; agent runs without it"
                            );
                        }
                    }
                }
            }
        }
        if !newly_created.is_empty() {
            self.dynamic_mcp_servers()
                .write()
                .await
                .insert(agent_id.to_string(), newly_created);
        }
    }

    /// Tear down dynamically-added MCP servers registered for this
    /// `agent_id`. Mirror of [`Self::initialize_per_agent_mcp`]. No-op
    /// when nothing was tracked.
    ///
    /// **W6.2 full**: see `clear_frontmatter_hooks` — both detached
    /// spawn paths now do this inline via cloned Arcs. Wrapper kept
    /// for future error-path callers.
    #[allow(dead_code)]
    pub(super) async fn cleanup_per_agent_mcp(&self, agent_id: &str) {
        let entry = self.dynamic_mcp_servers().write().await.remove(agent_id);
        let Some(names) = entry else { return };
        let Some(handle) = self.mcp_handle() else {
            return;
        };
        for name in names {
            if let Err(e) = handle.remove_dynamic_server(&name).await {
                tracing::debug!(
                    error = %e,
                    %agent_id,
                    server = %name,
                    "failed to remove dynamic agent MCP server (continuing cleanup)"
                );
            }
        }
    }

    /// Resolve every skill name in `def.skills` via the installed
    /// `SkillHandle` and prepend the loaded bodies to `prompt` as
    /// `<preloaded-skill name="...">` XML blocks. Missing skills /
    /// missing handle / read errors are logged at debug and silently
    /// dropped — the spawn proceeds with whatever loaded.
    pub(super) async fn preload_frontmatter_skills(
        &self,
        definition: Option<&coco_types::AgentDefinition>,
        prompt: &str,
    ) -> String {
        let Some(def) = definition else {
            return prompt.to_string();
        };
        if def.skills.is_empty() {
            return prompt.to_string();
        }
        let Some(handle) = self.skill_handle() else {
            tracing::debug!(
                agent_type = %def.agent_type,
                count = def.skills.len(),
                "agent declares frontmatter skills but no SkillHandle is installed; skipping preload"
            );
            return prompt.to_string();
        };

        let mut blocks = Vec::with_capacity(def.skills.len());
        for name in &def.skills {
            match handle.read_skill_body(name).await {
                Some(body) if !body.trim().is_empty() => blocks.push(format!(
                    "<preloaded-skill name=\"{name}\">\n{body}\n</preloaded-skill>"
                )),
                _ => {
                    tracing::debug!(
                        agent_type = %def.agent_type,
                        skill = %name,
                        "frontmatter skill not found / empty body; skipping preload"
                    );
                }
            }
        }
        if blocks.is_empty() {
            return prompt.to_string();
        }
        format!("{}\n\n{prompt}", blocks.join("\n\n"))
    }

    /// Fire SubagentStop hooks. Errors are logged and swallowed — a
    /// failed stop hook must not gate the spawn's response. TS parity:
    /// stop hooks run for completion / failure / cancel.
    ///
    /// **W6.2 full**: see `clear_frontmatter_hooks` — both detached
    /// spawn paths now invoke `fire_subagent_stop_for_task` directly.
    /// Wrapper kept for early-failure paths that have `&self` in scope.
    #[allow(dead_code)]
    pub(super) async fn fire_subagent_stop_hook(
        &self,
        agent_id: &str,
        agent_type: &str,
        transcript_path: Option<&str>,
    ) -> Option<coco_hooks::orchestration::AggregatedHookResult> {
        let registry = self.hook_registry().cloned()?;
        let ctx = self.hook_orchestration_context();
        match coco_hooks::orchestration::execute_subagent_stop(
            &registry,
            &ctx,
            /*stop_hook_active*/ false,
            agent_type,
            agent_id,
            transcript_path.unwrap_or(""),
            /*last_assistant_message*/ None,
        )
        .await
        {
            Ok(result) => Some(result),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    %agent_id,
                    %agent_type,
                    "SubagentStop hook firing failed"
                );
                None
            }
        }
    }
}

/// Sync-path early-failure response. Worktree cleanup is the caller's
/// responsibility — this helper only builds the `AgentSpawnResponse`
/// shell.
pub(super) fn spawn_failed(
    agent_id: String,
    message: String,
    duration_ms: i64,
) -> AgentSpawnResponse {
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
        ..Default::default()
    }
}

impl SwarmAgentHandle {
    pub(super) async fn spawn_subagent(
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
        tracing::info!(
            agent_id = %agent_id,
            agent_type = %agent_type,
            run_in_background = request.run_in_background,
            isolation = ?request.isolation,
            spawn_mode = ?request.spawn_mode,
            "subagent spawn dispatch"
        );

        // Validation must complete BEFORE registering the agent state.
        // Earlier code pushed the entry first, then validated — leaving a
        // dangling Pending entry on every worktree-creation or
        // missing-engine failure. Build the prospective state now and
        // commit it only after both gates pass.
        let prospective_state = SubAgentState {
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

        // Worktree isolation: any creation error returns a model-visible
        // failure — never silently fall back to sync-without-isolation.
        let worktree_session = if matches!(request.isolation.as_deref(), Some("worktree")) {
            match self.worktree_manager() {
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
                        Ok(s) => {
                            // TS `executeWorktreeCreateHook(slug)` fires
                            // here so user hooks can react to (or override
                            // future creations of) per-agent worktrees.
                            // Coco-rs always uses git internally — the
                            // hook is observe-only for now (`worktree.ts:716`).
                            fire_worktree_create_hook(
                                self.hook_registry().cloned(),
                                &self.cwd,
                                &slug,
                            )
                            .await;
                            Some(s)
                        }
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

        let Some(engine) = self.execution_engine() else {
            if let (Some(m), Some(session)) = (self.worktree_manager(), worktree_session.clone()) {
                let removed_path = session.path.display().to_string();
                let _ = m.cleanup_if_unchanged(session);
                fire_worktree_remove_hook(self.hook_registry().cloned(), &self.cwd, &removed_path)
                    .await;
            }
            return Ok(spawn_failed(
                agent_id,
                "No AgentQueryEngine configured on SwarmAgentHandle. Use \
                 SwarmAgentHandle::set_execution_engine at session bootstrap."
                    .into(),
                start.elapsed().as_millis() as i64,
            ));
        };

        // All validation passed — commit the agent state. Subsequent
        // failures (engine errors during execute_query, transcript-write
        // failures) update the same entry's `status` rather than leaking
        // it.
        self.agents().write().await.push(prospective_state);

        let cwd_override = worktree_session
            .as_ref()
            .map(|s| s.path.clone())
            .or_else(|| request.cwd.clone());

        // Spawn-time identity resolution (T3 + T7). Centralizes model +
        // role precedence in `coco_subagent`:
        //   model:  request.model > definition.model > role-resolved
        //   role:   request.model_role > definition.model_role
        //         > subagent_type → role > Subagent
        //
        // Memory-crate forks (extract / dream / session-memory) set
        // `request.model_role = Some(ModelRole::Memory)` so an operator
        // configuring `settings.models.memory` actually steers them
        // instead of falling through to `general-purpose →
        // ModelRole::Subagent`.
        //
        // The definition flows through `AgentSpawnRequest.definition`,
        // populated by AgentTool from `ctx.agent_catalog`. When the catalog
        // isn't installed, `definition` is `None` and the resolver
        // degrades cleanly to subagent_type→role mapping.
        let agent_type_id: Option<coco_types::AgentTypeId> = request
            .subagent_type
            .as_deref()
            .map(|t| t.parse().expect("AgentTypeId::from_str is Infallible"));
        let selection = coco_subagent::resolve_subagent_selection(
            request.model.as_deref(),
            request.model_role,
            request.definition.as_deref(),
            agent_type_id.as_ref(),
        );

        // Pin the agent type's color in the per-`AgentTypeId` cache so
        // `SubagentPanel` renders all `Explore` spawns in the same color
        // regardless of how many copies are running. Teammates use a
        // separate per-`name@team` cache. TS:
        // `tools/AgentTool/agentColorManager.ts:setAgentColor`.
        if let Some(id) = agent_type_id.as_ref() {
            let _ = crate::pane::layout::assign_agent_type_color(id);
        }

        // Resolve the prior-history + system-prompt pair from the
        // requested spawn mode:
        //
        // - Fresh     → no history; system prompt seeded from
        //               `definition.system_prompt` (TS-parity
        //               `runAgent.ts:906-932 getAgentSystemPrompt` →
        //               `getSystemPrompt({...})`). Built-ins populate
        //               this via [`coco_subagent::builtin_prompts`];
        //               markdown agents via the body of their `.md`
        //               file. Without this, the child would fall
        //               through to the engine's generic default
        //               instead of receiving its role instructions.
        // - Fork      → parent's pre-rendered system-prompt bytes
        //               verbatim (cache parity), parent history with
        //               `tool_result` blocks rewritten to
        //               `FORK_PLACEHOLDER` (TS `forkSubagent.ts`).
        // - Resume    → seed from `definition.system_prompt` like
        //               Fresh (TS `resumeAgent.ts` rebuilds from the
        //               definition); prior history kept verbatim (NO
        //               placeholder rewrite — the child needs real
        //               tool outputs to continue).
        //
        // `preserve_tool_use_results` flips on for Fork and Resume so
        // downstream compaction doesn't strip the inherited results.
        // Resolve cwd + model once — both feed into env_info / CLAUDE.md
        // discovery / per-agent memory.
        let cwd_for_prompt = cwd_override
            .clone()
            .unwrap_or_else(|| std::path::PathBuf::from(&self.cwd));
        // Model resolution by spawn mode:
        //
        // - **Fork**: pin to the snapshot's `api_model_name` (carried
        //   non-optionally inside `SpawnMode::Fork`). The whole point
        //   of fork is prompt-cache parity; reading live
        //   `RuntimeConfig` here would silently bust the cache after
        //   a hot-reload between the parent's last turn and this
        //   spawn. Used for BOTH env-block rendering AND the actual
        //   `AgentQueryConfig.model` below — they must agree.
        //
        // - **Resume**: rebuild fresh from current runtime. Resume
        //   restarts a previously-backgrounded agent in a (possibly
        //   different) process; pinning to a snapshot captured *now*
        //   at engine bootstrap would conflate "current parent" with
        //   "original spawn" and is meaningless.
        //
        // - **Fresh**: caller's `request.model` > `def.model` >
        //   coordinator's current Main role.
        let model_pinned_to_snapshot = match &request.spawn_mode {
            coco_tool_runtime::SpawnMode::Fork {
                parent_snapshot, ..
            } => Some(parent_snapshot.api_model_name.clone()),
            _ => None,
        };
        let model_for_env = model_pinned_to_snapshot.clone().unwrap_or_else(|| {
            selection
                .model
                .clone()
                .unwrap_or_else(|| self.current_main_model_id())
        });
        // `dirs::home_dir()` can return `None` on minimal containers
        // (no `$HOME`, no passwd entry). The legacy code fell back to
        // `/tmp`, which silently routed memory lookups to the wrong
        // User-scope per-agent memory follows `COCO_CONFIG_HOME` (via
        // `global_config::config_home()`), NOT the system home dir.
        // Multi-tenant / containerised setups where `~/.coco` is
        // unwritable still get a usable agent-memory dir. Project /
        // Local scopes are per-repo and ignore `config_home`.
        let config_home = coco_config::global_config::config_home();

        // Per-agent memory block (TS parity:
        // `tools/AgentTool/loadAgentsDir.ts:484,728` + `loadPluginAgents.ts:207`).
        // Fork inherits parent's rendered prompt verbatim so memory
        // injection is skipped there.
        let inject_memory = !matches!(
            request.spawn_mode,
            coco_tool_runtime::SpawnMode::Fork { .. }
        );
        let memory_block = match (
            inject_memory,
            request.definition.as_deref().and_then(|d| d.memory_scope),
        ) {
            (true, Some(scope)) => Some(coco_memory::agent_memory::load_agent_memory_prompt(
                agent_type,
                scope,
                &cwd_for_prompt,
                &config_home,
            )),
            _ => None,
        };

        // Build the Fresh/Resume system prompt via the shared
        // `coco_context::build_system_prompt` assembler — same code path
        // the leader uses. This restores:
        //   - <env>...</env> block (Working directory, git repo Y/N,
        //     Platform, Shell, OS Version, model line, knowledge cutoff)
        //   - 4 AGENT_NOTES bullets (absolute paths, no emojis, …)
        //   - CLAUDE.md discovery (gated by `def.omit_claude_md`)
        //   - Memory block (appended at the correct cache-broken
        //     position)
        // TS parity: `AgentTool.tsx:534`
        // `enhanceSystemPromptWithEnvDetails([agentPrompt], model, …)`.
        let build_fresh_prompt = || -> String {
            let def = request.definition.as_deref();
            let identity = def
                .and_then(|d| d.system_prompt.as_deref())
                .filter(|s| !s.is_empty())
                .unwrap_or(coco_context::prompt::DEFAULT_AGENT_IDENTITY);
            let claude_md_files: Vec<coco_context::MemoryFile> =
                if def.map(|d| d.omit_claude_md).unwrap_or(false) {
                    Vec::new()
                } else {
                    coco_context::discover_memory_files(&cwd_for_prompt)
                };
            let env_info = coco_context::get_environment_info(&cwd_for_prompt, &model_for_env);
            coco_context::build_system_prompt(
                identity,
                &claude_md_files,
                &env_info,
                // `skill_listing` here is the system-prompt slot — never
                // populated by the main agent either (skill listing flows
                // via `coco_system_reminder::generators::skill_listing`
                // per-turn, not as a baked-in section). Subagent matches
                // main agent: pass None. Separate gap covered below
                // wires the skill listing through the subagent's
                // GeneratorContext so the per-turn reminder fires.
                /*skill_listing=*/
                None,
                memory_block.as_deref(),
                // AGENT_NOTES via `notes_after_env` — TS subagent path
                // (`AgentTool.tsx:534 enhanceSystemPromptWithEnvDetails`)
                // bundles `notes` with the env block. By passing them
                // through this slot they render BEFORE memory (matching
                // TS), not after. Main agent path passes `None` because
                // TS `getSystemPrompt` has richer per-section rules
                // instead of these 4 condensed bullets.
                Some(coco_context::prompt::AGENT_NOTES),
                /*output_style=*/ None,
                /*additional_working_directories=*/ &[],
            )
            .full_text()
        };

        // Resolve (system_prompt, prior_messages, preserve_tool_use_results)
        // by spawn mode. `is_fork` controls whether the child's first
        // user turn gets wrapped in `<fork-boilerplate>` XML so the
        // recursion guard ([`coco_subagent::is_in_fork_child`]) can
        // detect fork-of-fork and the worker receives its rules. TS
        // parity: `forkSubagent.ts::buildChildMessage`.
        let (system_prompt, fork_context_messages, preserve_tool_use_results, is_fork) =
            match &request.spawn_mode {
                coco_tool_runtime::SpawnMode::Fork {
                    rendered_system_prompt,
                    parent_messages,
                    parent_snapshot: _,
                } => {
                    // Fork MUST use parent's pre-rendered prompt verbatim
                    // for prompt-cache parity. Memory was already
                    // captured by the parent's own assembly.
                    let ctx = coco_subagent::build_fork_context(parent_messages, &request.prompt);
                    (rendered_system_prompt.clone(), ctx.messages, true, true)
                }
                coco_tool_runtime::SpawnMode::Resume { parent_messages } => {
                    (build_fresh_prompt(), parent_messages.clone(), true, false)
                }
                coco_tool_runtime::SpawnMode::Fresh => (
                    build_fresh_prompt(),
                    request.fork_context_messages.clone(),
                    false,
                    false,
                ),
                // `SpawnMode` is `#[non_exhaustive]` (cross-crate), so the
                // compiler forces a wildcard. Future variants MUST be
                // wired explicitly at this seam — failing fast beats
                // a Fresh fallback that silently degrades cache parity
                // / recursion guarantees the new variant might rely on.
                other => {
                    return Ok(spawn_failed(
                        agent_id,
                        format!(
                            "Unhandled SpawnMode variant {other:?}; the coordinator must be \
                             updated to thread the new mode through `spawn_subagent`."
                        ),
                        start.elapsed().as_millis() as i64,
                    ));
                }
            };
        let query_config = coco_tool_runtime::AgentQueryConfig {
            system_prompt,
            // **Fork**: pin to `parent_snapshot.api_model_name`
            // (carried by `SpawnMode::Fork`). Cache parity requires
            // the API call to use the exact model the parent's
            // snapshot captured — falling back to `current_main_model_id()`
            // after a hot-reload would silently break the cache.
            //
            // **Fresh/Resume**: `selection.model` (request > definition)
            // wins; otherwise fall through to the role-resolved primary
            // from live `RuntimeConfig` via `current_main_model_id()`
            // (T6 hot-reload pickup).
            model: model_pinned_to_snapshot.clone().unwrap_or_else(|| {
                selection
                    .model
                    .clone()
                    .unwrap_or_else(|| self.current_main_model_id())
            }),
            model_selection: if let Some(model) = model_pinned_to_snapshot.as_deref() {
                coco_types::LlmModelSelection::from_model_and_role(
                    Some(model),
                    Some(coco_types::ModelRole::Main),
                )
            } else if request.mode.as_deref() == Some("plan")
                && !matches!(
                    request.spawn_mode,
                    coco_tool_runtime::SpawnMode::Fork { .. }
                )
            {
                coco_types::LlmModelSelection::from_model_and_role(
                    selection.model.as_deref(),
                    Some(coco_types::ModelRole::Plan),
                )
            } else {
                selection.model_selection.clone()
            },
            max_turns: request
                .constraints
                .as_ref()
                .and_then(|c| c.max_turns)
                .or(request.max_turns),
            context_window: None,
            prompt_cache: None,
            max_output_tokens: None,
            // Coordinator-mode tool pool: when the leader is in coordinator
            // mode, AgentTool spawns are workers and must see only the
            // worker tool pool. Outside coordinator mode the child's own
            // `AgentDefinition.allowed_tools` (later resolved by the
            // engine) controls — leave empty here.
            allowed_tools: if request
                .features
                .as_deref()
                .is_some_and(coco_subagent::is_coordinator_mode)
            {
                let simple_mode = coco_config::env::is_env_truthy(coco_config::EnvKey::CocoSimple);
                coco_subagent::worker_tool_pool(simple_mode)
                    .into_iter()
                    .map(str::to_string)
                    .collect()
            } else {
                Vec::new()
            },
            disallowed_tools: request.disallowed_tools.clone(),
            // Coordinator / AgentTool spawns don't carry skill-style
            // auto-allow rules — those flow only through
            // `SkillRuntime` Fork path. Leave empty.
            extra_permission_rules: Vec::new(),
            live_permission_rules: None,
            live_permission_mode: None,
            tool_overrides: request.tool_overrides.clone(),
            features: request.features.clone(),
            parent_tool_filter: request.parent_tool_filter.clone(),
            preserve_tool_use_results,
            permission_mode: request.mode.clone(),
            agent_id: Some(agent_id.clone()),
            is_teammate: false,
            is_in_process_teammate: false,
            plan_mode_required: false,
            session_id: None,
            bypass_permissions_available: false,
            cwd_override,
            fork_context_messages,
            allowed_write_roots: request
                .constraints
                .as_ref()
                .map(|c| c.allowed_write_roots.clone())
                .unwrap_or_default(),
            // P1-6 fix — plan-mode children route through ModelRole::Plan
            // regardless of the agent's declared role. TS parity: in
            // plan mode the leader's main client swap promotes to the
            // plan model (`engine.rs:1056-1087`); for custom agents
            // spawned with `mode: "plan"` we replicate that promotion
            // at the role level so the inference layer's role-resolver
            // routes to the plan client. Without this, a custom agent
            // declaring `model_role: subagent` and called with
            // `mode: plan` ran on the cheaper Subagent model — silently
            // worse for plan-mode reasoning quality.
            model_role: Some(
                if request.mode.as_deref() == Some("plan")
                    && !matches!(
                        request.spawn_mode,
                        coco_tool_runtime::SpawnMode::Fork { .. }
                    )
                {
                    coco_types::ModelRole::Plan
                } else {
                    selection.model_role
                },
            ),
            effort: request.effort.clone(),
            use_exact_tools: request.use_exact_tools,
            mcp_servers: request.mcp_servers.clone(),
            initial_prompt: request.initial_prompt.clone(),
            definition: request.definition.clone(),
            // In-process AgentTool spawns inherit the leader's
            // `ToolPermissionBridge` via `wire_engine`. Setting an override
            // here would mask whatever the leader installed (SDK
            // askForApproval / TUI overlay / legacy auto-allow).
            // Cross-process pane teammates use `MailboxPermissionBridge`
            // (file IPC) instead.
            permission_bridge: None,
            // The bg path below populates a per-task event channel so live
            // text deltas reach the task's output buffer.
            event_tx: None,
            // Per-fork canUseTool callback inherits from the request. The
            // AgentTool spawn path doesn't set one by default; memory /
            // dream / session services thread their per-policy handle
            // via `request.can_use_tool` after PR 4.
            can_use_tool: request.can_use_tool.clone(),
            require_can_use_tool: request.require_can_use_tool,
            fork_label: request.fork_label,
            max_output_tokens_override: None,
            cancel: None,
        };

        if request.run_in_background {
            return self
                .spawn_background(
                    request,
                    agent_id,
                    agent_type,
                    query_config,
                    worktree_session,
                    start,
                    engine,
                    is_fork,
                )
                .await;
        }

        // ── Frontmatter hooks registration (TS parity: runAgent.ts:564-575) ──
        //
        // Register `def.hooks` under the spawn's agent_id so they're
        // visible to every event firing during this spawn. Cleared at
        // SubagentStop below. No-op when def.hooks is null / hook
        // registry unwired.
        let registered_frontmatter_hooks =
            self.register_frontmatter_hooks(&agent_id, request.definition.as_deref());

        // ── Per-agent MCP servers (TS parity: runAgent.ts:95-218 initializeAgentMcpServers) ──
        //
        // String-ref entries piggyback on the parent's pre-existing
        // connections; inline `{name: config}` entries get registered
        // as dynamic servers and torn down at SubagentStop.
        self.initialize_per_agent_mcp(&agent_id, request.definition.as_deref())
            .await;

        // ── Frontmatter skills preload (TS parity: runAgent.ts:577-645) ──
        //
        // When `def.skills` is non-empty, resolve each skill's body
        // via the installed `SkillHandle` and prepend the loaded
        // contents to the prompt — wrapped in `<preloaded-skill>`
        // blocks so the model can distinguish them from the actual
        // task prompt.
        let prompt_with_skills = self
            .preload_frontmatter_skills(request.definition.as_deref(), &request.prompt)
            .await;

        // ── SubagentStart hook firing (TS parity: runAgent.ts:530-555) ──
        //
        // Fire user-defined SubagentStart hooks and inject their
        // `additional_contexts` into the child's prompt as a leading
        // system-reminder block. Pre-fix: SubagentStart was defined in
        // the hook event taxonomy but no caller ever fired it for
        // subagent spawns.
        let (decorated_prompt, _start_result) = self
            .fire_subagent_start_hook(&agent_id, agent_type, &prompt_with_skills)
            .await;

        // Fork mode: wrap the decorated directive in the TS-parity
        // `<fork-boilerplate>...</fork-boilerplate>` envelope so:
        //   - the worker receives its rules (no-converse, scope-bound,
        //     report-format) — TS `forkSubagent.ts:173-194`.
        //   - the conversation contains the boilerplate tag so a future
        //     `is_in_fork_child(parent_messages)` scan blocks recursive
        //     forking — `forkSubagent.ts::isInForkChild`.
        let effective_prompt = if is_fork {
            coco_subagent::build_fork_child_message(&decorated_prompt)
        } else {
            decorated_prompt
        };

        // W4 (B1 fix): register the sync agent in TaskRuntime when
        // a registry is wired. TS parity: sync agents go through
        // `registerAgentForeground` which populates `appState.tasks`
        // so the UI panel + TaskList tool see them as Running.
        // Without this, sync agents were invisible — only background
        // agents were tracked.
        //
        // W6 (Dream registration): when the spawn carries the
        // `AutoDream` fork label, register as `TaskType::Dream`
        // instead of `LocalAgent` so the TUI panel + `TaskList` tool
        // can differentiate auto-memory consolidation from
        // user-spawned subagents. TS parity:
        // `tasks/DreamTask/DreamTask.ts:72` `registerTask({type:
        // 'dream'})`. Other framework-spawned forks (extract /
        // session-memory / prompt-suggestion / side-question /
        // compact / agent-summary / speculation) are
        // service-internal and don't surface as user-visible
        // tasks — TS doesn't register them either.
        //
        // The cancel token from `register_*_task` lets `kill_task`
        // propagate into the engine via the `tokio::select!` race
        // below — same mechanism the bg path uses.
        let task_registry = self.task_registry().cloned();
        let is_dream = matches!(request.fork_label, Some(coco_types::ForkLabel::AutoDream));
        let is_skip_registration = matches!(
            request.fork_label,
            Some(coco_types::ForkLabel::ExtractMemories)
                | Some(coco_types::ForkLabel::SessionMemoryAuto)
                | Some(coco_types::ForkLabel::SessionMemoryManual)
                | Some(coco_types::ForkLabel::PromptSuggestion)
                | Some(coco_types::ForkLabel::SideQuestion)
                | Some(coco_types::ForkLabel::Compact)
                | Some(coco_types::ForkLabel::AgentSummary)
                | Some(coco_types::ForkLabel::Speculation)
        );
        let sync_task = if let Some(reg) = task_registry.as_ref() {
            if is_skip_registration {
                None
            } else {
                let task_cancel = tokio_util::sync::CancellationToken::new();
                let description = request
                    .description
                    .clone()
                    .unwrap_or_else(|| agent_type.to_string());
                let tid = if is_dream {
                    reg.register_dream_task(&description, task_cancel.clone())
                        .await
                } else {
                    reg.register_agent_task(
                        &description,
                        request.tool_use_id.as_deref(),
                        request.invoking_agent_id.as_deref(),
                        task_cancel.clone(),
                    )
                    .await
                };
                Some((tid, task_cancel))
            }
        } else {
            None
        };

        // W6.2 (full): the entire sync execution path — engine call,
        // cleanup chain, response build — now lives inside a detached
        // `tokio::spawn` body. The inline caller races a oneshot
        // receiver against the detach signal and external cancel:
        //
        // - **Oneshot delivery**: engine task finishes, sends the
        //   built `AgentSpawnResponse`. Inline caller returns it.
        //   `complete_silent` runs (state only, no notification —
        //   response goes inline).
        // - **Detach signal**: external `signal_detach(tid)` (TUI
        //   Ctrl+B). Inline caller sets the detached flag and
        //   returns `AsyncLaunched` immediately. Engine task keeps
        //   running, eventually calls `mark_completed`/`mark_failed`
        //   on its own (pushes `<task-notification>` envelope).
        //   This is the TS-parity "detach but keep running" behavior.
        // - **External cancel** (`kill_task(tid)`): engine task's
        //   inner select observes the cancel and exits with an Err
        //   QueryResult. Cleanup still runs in the engine task; the
        //   final response is `AgentSpawnResponse::Failed`. Inline
        //   caller receives it via oneshot.
        //
        // Pre-clone every Arc the engine task needs (so the closure
        // doesn't borrow `&self`). Matches the pattern used by
        // `spawn_background` below.
        let hook_registry_for_engine = self.hook_registry().cloned();
        let mcp_handle_for_engine = self.mcp_handle().cloned();
        let dynamic_mcp_servers_for_engine = self.dynamic_mcp_servers().clone();
        let worktree_manager_for_engine = self.worktree_manager().cloned();
        let agents_for_engine = self.agents().clone();
        let side_query_for_engine = self.side_query().cloned();
        let cwd_for_engine = self.cwd.clone();
        let task_registry_for_engine = task_registry.clone();
        let agent_id_for_engine = agent_id.clone();
        let agent_type_for_engine = agent_type.to_string();
        let task_id_for_engine = sync_task.as_ref().map(|(id, _)| id.clone());
        let task_cancel_for_engine = sync_task.as_ref().map(|(_, c)| c.clone());
        let worktree_session_for_engine = worktree_session.clone();
        let registered_frontmatter_hooks_for_engine = registered_frontmatter_hooks;

        // Detach handle for the inline caller's race. `None` for memory
        // forks / no registry; degrades to a 2-arm select (resp + cancel).
        let detach_handle_for_inline: Option<std::sync::Arc<tokio::sync::Notify>> =
            match (task_registry.as_ref(), sync_task.as_ref()) {
                (Some(reg), Some((tid, _))) => reg.detach_handle(tid).await,
                _ => None,
            };

        let (resp_tx, resp_rx) = tokio::sync::oneshot::channel::<AgentSpawnResponse>();
        let detached_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let detached_flag_for_engine = detached_flag.clone();
        let sync_start = start;
        let engine_for_task = engine.clone();

        tokio::spawn(async move {
            // ── Engine query (race against task cancel) ──────────
            let query_result = if let Some(c) = task_cancel_for_engine.as_ref() {
                tokio::select! {
                    biased;
                    () = c.cancelled() => {
                        Err(Box::new(coco_error::PlainError::new(
                            "task cancelled by leader",
                            coco_error::StatusCode::Cancelled,
                        )) as coco_error::BoxedError)
                    }
                    r = engine_for_task.execute_query(&effective_prompt, query_config) => r,
                }
            } else {
                engine_for_task
                    .execute_query(&effective_prompt, query_config)
                    .await
            };
            let duration_ms = sync_start.elapsed().as_millis() as i64;

            // ── Cleanup chain (no `&self` — uses cloned Arcs) ─────
            fire_subagent_stop_for_task(
                hook_registry_for_engine.clone(),
                &cwd_for_engine,
                &agent_id_for_engine,
                &agent_type_for_engine,
                /*transcript*/ None,
            )
            .await;

            if registered_frontmatter_hooks_for_engine
                && let Some(reg) = hook_registry_for_engine.as_ref()
            {
                reg.clear_agent_scope(&agent_id_for_engine);
            }

            let dynamic_names = dynamic_mcp_servers_for_engine
                .write()
                .await
                .remove(&agent_id_for_engine);
            if let (Some(names), Some(handle)) = (dynamic_names, mcp_handle_for_engine.as_ref()) {
                for name in names {
                    if let Err(e) = handle.remove_dynamic_server(&name).await {
                        tracing::debug!(
                            error = %e,
                            agent_id = %agent_id_for_engine,
                            server = %name,
                            "sync cleanup: failed to remove dynamic agent MCP server"
                        );
                    }
                }
            }

            let (worktree_path, worktree_branch) = match (
                worktree_manager_for_engine.as_ref(),
                worktree_session_for_engine,
            ) {
                (Some(m), Some(session)) => {
                    let session_path = session.path.display().to_string();
                    match m.cleanup_if_unchanged(session) {
                        crate::worktree::WorktreeCleanupOutcome::Removed => {
                            fire_worktree_remove_hook(
                                hook_registry_for_engine.clone(),
                                &cwd_for_engine,
                                &session_path,
                            )
                            .await;
                            (None, None)
                        }
                        crate::worktree::WorktreeCleanupOutcome::Kept { path, branch, .. } => {
                            (Some(path), Some(branch))
                        }
                    }
                }
                _ => (None, None),
            };

            {
                let mut agents = agents_for_engine.write().await;
                if let Some(agent) = agents
                    .iter_mut()
                    .find(|a| a.agent_id == agent_id_for_engine)
                {
                    agent.status = match &query_result {
                        Ok(_) => SubAgentStatus::Completed,
                        Err(_) => SubAgentStatus::Failed,
                    };
                }
            }

            // ── Build AgentSpawnResponse ──────────────────────────
            let response = match query_result {
                Ok(qr) => {
                    tracing::info!(
                        agent_id = %agent_id_for_engine,
                        agent_type = %agent_type_for_engine,
                        tool_use_count = qr.tool_use_count,
                        tokens_in = qr.input_tokens,
                        tokens_out = qr.output_tokens,
                        duration_ms,
                        "subagent spawn ok"
                    );
                    let response_text = super::handoff::classify_handoff_inline(
                        &agent_type_for_engine,
                        &qr,
                        side_query_for_engine.as_ref(),
                    )
                    .await;
                    super::handoff::summarize_handoff_inline(
                        &agent_type_for_engine,
                        &qr,
                        &agent_id_for_engine,
                        side_query_for_engine.as_ref(),
                        &agents_for_engine,
                    )
                    .await;
                    AgentSpawnResponse {
                        status: AgentSpawnStatus::Completed,
                        agent_id: Some(agent_id_for_engine.clone()),
                        result: response_text,
                        error: None,
                        total_tool_use_count: qr.tool_use_count,
                        total_tokens: qr.input_tokens + qr.output_tokens,
                        input_tokens: qr.input_tokens,
                        output_tokens: qr.output_tokens,
                        tool_use_counts: count_tool_uses_in_messages(&qr.messages),
                        cache_read_tokens: 0,
                        cache_creation_tokens: 0,
                        paths_written: Vec::new(),
                        duration_ms,
                        worktree_path: worktree_path.clone(),
                        worktree_branch: worktree_branch.clone(),
                        output_file: None,
                        prompt: None,
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        agent_id = %agent_id_for_engine,
                        agent_type = %agent_type_for_engine,
                        duration_ms,
                        error = %e,
                        "subagent spawn failed"
                    );
                    AgentSpawnResponse {
                        status: AgentSpawnStatus::Failed,
                        agent_id: Some(agent_id_for_engine.clone()),
                        result: None,
                        error: Some(e.to_string()),
                        total_tool_use_count: 0,
                        total_tokens: 0,
                        duration_ms,
                        worktree_path: worktree_path.clone(),
                        worktree_branch: worktree_branch.clone(),
                        output_file: None,
                        prompt: None,
                        ..Default::default()
                    }
                }
            };

            // ── Route based on detached flag ─────────────────────
            let was_detached = detached_flag_for_engine.load(std::sync::atomic::Ordering::SeqCst);
            if was_detached {
                // Caller already returned AsyncLaunched. Push the
                // `<task-notification>` envelope (same path bg uses).
                if let (Some(reg), Some(tid)) = (
                    task_registry_for_engine.as_ref(),
                    task_id_for_engine.as_deref(),
                ) {
                    match response.status {
                        AgentSpawnStatus::Completed => {
                            let payload = coco_tool_runtime::AgentCompletionPayload {
                                result: response.result.clone(),
                                usage: Some(coco_tool_runtime::AgentUsage {
                                    total_tokens: response.total_tokens,
                                    tool_uses: response.total_tool_use_count as i32,
                                    duration_ms,
                                }),
                                worktree: response.worktree_path.clone().map(|path| {
                                    coco_tool_runtime::AgentWorktree {
                                        path: path.display().to_string(),
                                        branch: response.worktree_branch.clone(),
                                    }
                                }),
                            };
                            reg.mark_completed(tid, payload).await;
                        }
                        AgentSpawnStatus::Failed => {
                            reg.mark_failed(
                                tid,
                                response.error.as_deref().unwrap_or("agent failed"),
                            )
                            .await;
                        }
                        _ => {}
                    }
                }
            } else {
                // Inline caller is still alive. Silent terminal +
                // send response via oneshot.
                if let (Some(reg), Some(tid)) = (
                    task_registry_for_engine.as_ref(),
                    task_id_for_engine.as_deref(),
                ) {
                    reg.complete_silent(
                        tid,
                        matches!(response.status, AgentSpawnStatus::Completed),
                    )
                    .await;
                }
                let _ = resp_tx.send(response);
            }
        });

        // ── Inline caller: race resp_rx vs detach vs (kill_task → resp_rx) ──
        let task_id_for_async_response = sync_task.as_ref().map(|(id, _)| id.clone());

        if let Some(detach) = detach_handle_for_inline {
            tokio::select! {
                biased;
                res = resp_rx => {
                    Ok(res.unwrap_or_else(|_| AgentSpawnResponse {
                        status: AgentSpawnStatus::Failed,
                        agent_id: Some(agent_id.clone()),
                        result: None,
                        error: Some("engine task panicked".into()),
                        duration_ms: start.elapsed().as_millis() as i64,
                        ..Default::default()
                    }))
                }
                () = detach.notified() => {
                    detached_flag.store(true, std::sync::atomic::Ordering::SeqCst);
                    tracing::info!(
                        target: "coco::agent_handle::sync",
                        agent_id = %agent_id,
                        "sync agent detached via signal_detach; engine continues in bg"
                    );
                    Ok(AgentSpawnResponse {
                        status: AgentSpawnStatus::AsyncLaunched,
                        agent_id: Some(task_id_for_async_response.unwrap_or(agent_id)),
                        result: None,
                        error: None,
                        total_tool_use_count: 0,
                        total_tokens: 0,
                        duration_ms: start.elapsed().as_millis() as i64,
                        worktree_path: None,
                        worktree_branch: None,
                        output_file: None,
                        prompt: None,
                        ..Default::default()
                    })
                }
            }
        } else {
            // No detach handle (memory forks / no registry). Just
            // await the engine task's response.
            Ok(resp_rx.await.unwrap_or_else(|_| AgentSpawnResponse {
                status: AgentSpawnStatus::Failed,
                agent_id: Some(agent_id.clone()),
                result: None,
                error: Some("engine task panicked".into()),
                duration_ms: start.elapsed().as_millis() as i64,
                ..Default::default()
            }))
        }
    }

    /// Background dispatch: spawn the engine in a detached tokio task,
    /// register with `AgentTaskRegistry` (if installed), persist transcript
    /// metadata for resume, drain text deltas into the task's output
    /// buffer, and run the periodic AgentSummary timer.
    ///
    /// Returns `AsyncLaunched` immediately. Worktree isolation is rejected
    /// at this seam so the cleanup contract isn't ambiguous.
    #[allow(clippy::too_many_arguments)]
    async fn spawn_background(
        &self,
        request: &AgentSpawnRequest,
        agent_id: String,
        agent_type: &str,
        query_config: coco_tool_runtime::AgentQueryConfig,
        worktree_session: Option<crate::worktree::AgentWorktreeSession>,
        start: Instant,
        engine: coco_tool_runtime::AgentQueryEngineRef,
        is_fork: bool,
    ) -> Result<AgentSpawnResponse, String> {
        let agents = self.agents().clone();
        let prompt = request.prompt.clone();
        let agent_id_for_task = agent_id.clone();

        let task_registry = self.task_registry().cloned();
        let cancel = tokio_util::sync::CancellationToken::new();
        let task_id = if let Some(reg) = task_registry.as_ref() {
            let description = request
                .description
                .clone()
                .unwrap_or_else(|| agent_type.to_string());
            // D3 / D4 (PR-1 W1): forward both the originating
            // `Agent(...)` tool_use_id and the *invoker* agent_id
            // (the agent that called AgentTool, not the new
            // subagent's id) so completion notifications route
            // correctly. TS parity: `AgentTool.tsx` passes both into
            // `registerAsyncAgent`.
            Some(
                reg.register_agent_task(
                    &description,
                    request.tool_use_id.as_deref(),
                    request.invoking_agent_id.as_deref(),
                    cancel.clone(),
                )
                .await,
            )
        } else {
            None
        };

        // Per-agent metadata sidecar (TS `writeAgentMetadata` at
        // `utils/sessionStorage.ts:283`). Persisted at registration so
        // resume can route the rehydrated spawn to the right `agent_type`
        // and (if worktree-isolated) restore cwd_override.
        let session_id = request.session_id.clone();
        if let (Some(store), Some(tid)) = (self.transcript_store(), task_id.as_deref()) {
            let store_for_meta = store.clone();
            let session_for_meta = session_id.clone();
            let task_for_meta = tid.to_string();
            let meta = coco_tool_runtime::AgentSpawnMetadata {
                agent_type: agent_type.to_string(),
                worktree_path: worktree_session
                    .as_ref()
                    .map(|s| s.path.display().to_string()),
                description: request.description.clone(),
            };
            if let Err(e) = store_for_meta
                .write_agent_metadata(&session_for_meta, &task_for_meta, &meta)
                .await
            {
                tracing::debug!(error = %e, "agent metadata write failed");
            }
        }

        // When a registry is installed, drain `Stream::TextDelta` events
        // from the engine into the task's output buffer so `TaskOutput`
        // returns mid-flight text. Without a registry the channel is unset
        // and the adapter's discarded fallback is used.
        let (event_tx, event_rx) = if task_registry.is_some() {
            let (tx, rx) = tokio::sync::mpsc::channel::<coco_types::CoreEvent>(64);
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };
        let mut query_config = query_config;
        query_config.event_tx = event_tx;

        if let (Some(reg), Some(tid), Some(mut rx)) =
            (task_registry.clone(), task_id.clone(), event_rx)
        {
            tokio::spawn(async move {
                while let Some(event) = rx.recv().await {
                    if let coco_types::CoreEvent::Stream(
                        coco_types::AgentStreamEvent::TextDelta { delta, .. },
                    ) = event
                    {
                        reg.append_output(&tid, &delta).await;
                    }
                }
            });
        }

        // Periodic AgentSummary timer (TS parity gate `AgentTool.tsx:750-852`):
        // only run when the spawn requested it via `enable_summarization`.
        // AgentTool resolves the flag at the boundary as
        // `is_coordinator || is_fork_subagent || sdk_opt_in`. Default-off
        // keeps a saturated coordinator (16 spawns × 30 s = 32 LLM calls/min)
        // off the user's hot path unless they explicitly opted in.
        const AGENT_SUMMARY_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);
        if !request.enable_summarization {
            tracing::debug!(
                %agent_id,
                "periodic AgentSummary disabled (request.enable_summarization = false)"
            );
        } else if let (Some(reg), Some(tid)) = (task_registry.clone(), task_id.clone()) {
            let agents_for_summary = self.agents().clone();
            let agent_id_for_summary = agent_id.clone();
            let cancel_for_summary = cancel.clone();
            let agent_type_owned = agent_type.to_string();
            let engine_for_summary = engine.clone();
            let definition_for_summary = request.definition.clone();
            // Reuse the spawn's resolved model role so the periodic
            // summary lands on the same per-role provider+model and
            // shares cache state. `query_config.model_role` was set by
            // spawn_subagent from `selection.model_role`.
            let model_role_for_summary = query_config.model_role;
            tokio::spawn(async move {
                let mut previous: Option<String> = None;
                loop {
                    tokio::select! {
                        _ = cancel_for_summary.cancelled() => break,
                        _ = tokio::time::sleep(AGENT_SUMMARY_INTERVAL) => {}
                    }
                    // Re-check terminal status AFTER waking — engine may
                    // have completed during the sleep, in which case the
                    // one-shot completion summary already ran in
                    // `spawn_subagent` and a periodic summary here would
                    // be redundant.
                    if reg.is_terminal(&tid).await {
                        break;
                    }
                    let buf = reg.read_output(&tid).await;
                    if buf.trim().is_empty() {
                        continue;
                    }
                    let (sys, user) = coco_subagent::build_summary_prompts(
                        &agent_type_owned,
                        previous.as_deref(),
                    );
                    // Bound the input to keep the summary call cheap — TS
                    // uses transcript filtering; we approximate by
                    // clipping the tail.
                    let tail = if buf.len() > 4_000 {
                        &buf[buf.len() - 4_000..]
                    } else {
                        buf.as_str()
                    };
                    let user_with_buf = format!("{user}\n\n--- recent output ---\n{tail}");

                    // ── AgentSummary cache parity ──
                    //
                    // TS `services/AgentSummary/agentSummary.ts` uses
                    // `runForkedAgent` with `CacheSafeParams` so the
                    // periodic summary call shares the parent agent's
                    // prompt cache. Pre-fix (Round 4): coco-rs used
                    // `SideQueryRequest::simple` which created a fresh
                    // request with no cache overlap.
                    //
                    // Now: route through the wrapped `AgentQueryEngine`
                    // with the same `definition` + `model_role` the
                    // parent spawn used, allowed_tools empty (no-tools
                    // turn). The engine factory installs the same
                    // system prompt, tools, and per-role provider so
                    // the cache key prefix lines up — the summary call
                    // benefits from the parent's warm cache.
                    let summary_cfg = coco_tool_runtime::AgentQueryConfig {
                        system_prompt: sys.clone(),
                        model: String::new(),
                        max_turns: Some(1),
                        // Empty allow-list → no tools fire during
                        // summarization (defense-in-depth alongside
                        // the typed canUseTool callback below).
                        allowed_tools: vec![String::new()],
                        is_teammate: false,
                        definition: definition_for_summary.clone(),
                        model_role: model_role_for_summary,
                        // TS parity: `services/AgentSummary/agentSummary.ts:109`
                        // `canUseTool: async () => deny-all`. Typed
                        // handle documents intent + composes through
                        // step 3.5 of execute_tool_call uniformly
                        // with the other 8 fork variants.
                        can_use_tool: Some(coco_tool_runtime::deny_all_handle(
                            "agent_summary: tools disabled",
                        )),
                        fork_label: Some(coco_types::ForkLabel::AgentSummary),
                        ..Default::default()
                    };
                    let summary_text = match engine_for_summary
                        .execute_query(&user_with_buf, summary_cfg)
                        .await
                    {
                        Ok(resp) => resp.response_text.unwrap_or_default(),
                        Err(_) => continue,
                    };
                    if let Some(clean) = coco_subagent::sanitize_summary(&summary_text) {
                        previous = Some(clean.clone());
                        let mut agents = agents_for_summary.write().await;
                        if let Some(state) = agents
                            .iter_mut()
                            .find(|s| s.agent_id == agent_id_for_summary)
                        {
                            state.last_message = Some(clean);
                        }
                    }
                }
            });
        }

        let registry_for_task = task_registry.clone();
        let task_id_for_task = task_id.clone();
        let cancel_for_task = cancel.clone();
        let transcript_store_for_task = self.transcript_store().cloned();
        let session_id_for_task = session_id.clone();
        let hook_registry_for_task = self.hook_registry().cloned();
        let cwd_for_task = self.cwd.clone();
        let agent_type_for_task = agent_type.to_string();
        // Preload frontmatter skills synchronously here — bg task can't
        // borrow `&self`, so resolve bodies upfront and prepend
        // before handing the prompt to the detached task.
        let prompt_after_skills = self
            .preload_frontmatter_skills(request.definition.as_deref(), &prompt)
            .await;
        let prompt = prompt_after_skills;
        // Register frontmatter hooks BEFORE the detached task starts
        // so they're visible during execute_query. Tracked here so the
        // task can clear them at SubagentStop time without re-borrowing
        // `&self`.
        let registered_frontmatter_hooks =
            self.register_frontmatter_hooks(&agent_id, request.definition.as_deref());
        // Initialise per-agent MCP servers synchronously here. Cleanup
        // happens after the spawn completes — we capture the dynamic
        // server map + handle into the task so it can run cleanup
        // without re-borrowing `&self`.
        self.initialize_per_agent_mcp(&agent_id, request.definition.as_deref())
            .await;
        let mcp_handle_for_task = self.mcp_handle().cloned();
        let dynamic_mcp_servers_for_task = self.dynamic_mcp_servers().clone();
        let worktree_manager_for_task = self.worktree_manager().cloned();
        let worktree_session_for_task = worktree_session.clone();
        let bg_start = std::time::Instant::now();
        tokio::spawn(async move {
            // Fire SubagentStart hooks before kicking off execution and
            // prepend any returned context blocks to the prompt. TS
            // parity: `runAgent.ts:530-555`. Bg path mirrors the sync
            // path's behaviour (added in this fix round).
            let decorated_prompt = fire_subagent_start_for_task(
                hook_registry_for_task.clone(),
                &cwd_for_task,
                &agent_id_for_task,
                &agent_type_for_task,
                &prompt,
            )
            .await;

            // Fork mode: wrap the decorated directive in the TS-parity
            // `<fork-boilerplate>...</fork-boilerplate>` envelope. See
            // sync-path comment above for the rationale (recursion
            // guard + worker rules). Mirrors `forkSubagent.ts::buildChildMessage`.
            let effective_prompt = if is_fork {
                coco_subagent::build_fork_child_message(&decorated_prompt)
            } else {
                decorated_prompt
            };

            // Race the engine query against the cancellation token so
            // `kill_task` propagates. The engine itself honours its config
            // `cancel`; the extra select here covers a cancel mid-engine.
            let outcome = tokio::select! {
                _ = cancel_for_task.cancelled() => {
                    Err(Box::new(coco_error::PlainError::new(
                        "task cancelled by leader",
                        coco_error::StatusCode::Cancelled,
                    )) as coco_error::BoxedError)
                }
                r = engine.execute_query(&effective_prompt, query_config) => r,
            };

            {
                let mut agents = agents.write().await;
                if let Some(state) = agents.iter_mut().find(|s| s.agent_id == agent_id_for_task) {
                    state.status = match &outcome {
                        Ok(_) => SubAgentStatus::Completed,
                        Err(_) => SubAgentStatus::Failed,
                    };
                }
            }

            // Fire SubagentStop AFTER execution completes (success,
            // failure, or cancel — TS parity). Transcript path is
            // populated when the per-agent JSONL store is wired and a
            // session-id is available.
            let transcript_path = transcript_store_for_task
                .as_ref()
                .zip(task_id_for_task.as_deref())
                .filter(|_| !session_id_for_task.is_empty())
                .map(|(_, tid)| format!("agent-{tid}.jsonl"));
            fire_subagent_stop_for_task(
                hook_registry_for_task.clone(),
                &cwd_for_task,
                &agent_id_for_task,
                &agent_type_for_task,
                transcript_path.as_deref(),
            )
            .await;

            // Cleanup per-agent hook bucket. Mirrors sync path's
            // `clear_frontmatter_hooks(&agent_id)`. TS parity:
            // `runAgent.ts` finally `clearSessionHooks(...)`.
            if registered_frontmatter_hooks && let Some(reg) = hook_registry_for_task.as_ref() {
                reg.clear_agent_scope(&agent_id_for_task);
            }

            // Tear down dynamically-added MCP servers. TS parity:
            // `runAgent.ts:197-210 mcpCleanup`. Pull names from the
            // shared map (same write lock the sync path uses) and
            // call `remove_dynamic_server` per entry.
            let dynamic_names = dynamic_mcp_servers_for_task
                .write()
                .await
                .remove(&agent_id_for_task);
            if let (Some(names), Some(handle)) = (dynamic_names, mcp_handle_for_task.as_ref()) {
                for name in names {
                    if let Err(e) = handle.remove_dynamic_server(&name).await {
                        tracing::debug!(
                            error = %e,
                            agent_id = %agent_id_for_task,
                            server = %name,
                            "bg cleanup: failed to remove dynamic agent MCP server"
                        );
                    }
                }
            }
            // Clone the session so the borrow at notification-build
            // time (below) still has access. `cleanup_if_unchanged`
            // takes ownership of one copy; the original survives for
            // the worktree info on the `<task-notification>` envelope.
            if let (Some(manager), Some(session)) = (
                worktree_manager_for_task.as_ref(),
                worktree_session_for_task.clone(),
            ) {
                let session_path = session.path.display().to_string();
                if matches!(
                    manager.cleanup_if_unchanged(session),
                    crate::worktree::WorktreeCleanupOutcome::Removed
                ) {
                    fire_worktree_remove_hook(
                        hook_registry_for_task.clone(),
                        &cwd_for_task,
                        &session_path,
                    )
                    .await;
                }
            }
            // Persist the full message history to the per-agent JSONL
            // transcript on success. `agent/resume` reads this back via
            // `AgentTranscriptStore::load_agent_messages` and threads the
            // entries into the resumed spawn's `fork_context_messages`.
            if let (Some(store), Some(tid), Ok(qr)) = (
                transcript_store_for_task.as_ref(),
                task_id_for_task.as_deref(),
                outcome.as_ref(),
            ) && !session_id_for_task.is_empty()
                && !qr.messages.is_empty()
                && let Err(e) = store
                    .append_agent_messages(&session_id_for_task, tid, &qr.messages)
                    .await
            {
                tracing::debug!(error = %e, "agent transcript write failed");
            }
            if let (Some(reg), Some(tid)) = (registry_for_task, task_id_for_task) {
                match outcome {
                    Ok(qr) => {
                        // TS `LocalAgentTask.tsx:249-251` — the
                        // completion notification carries the
                        // final assistant text, usage stats, and
                        // worktree info so the model sees a rich
                        // `<result>` / `<usage>` / `<worktree>`
                        // envelope on the next turn.
                        let duration_ms = bg_start.elapsed().as_millis() as i64;
                        let result = last_assistant_text(&qr.messages);
                        let usage = Some(coco_tool_runtime::AgentUsage {
                            total_tokens: qr.input_tokens + qr.output_tokens,
                            tool_uses: qr.tool_use_count as i32,
                            duration_ms,
                        });
                        let worktree = worktree_session_for_task.as_ref().map(|s| {
                            coco_tool_runtime::AgentWorktree {
                                path: s.path.display().to_string(),
                                branch: Some(s.branch.clone()),
                            }
                        });
                        reg.mark_completed(
                            &tid,
                            coco_tool_runtime::AgentCompletionPayload {
                                result,
                                usage,
                                worktree,
                            },
                        )
                        .await;
                    }
                    Err(e) => reg.mark_failed(&tid, &e.to_string()).await,
                }
            }
        });

        let output_file =
            if let (Some(reg), Some(tid)) = (task_registry.as_ref(), task_id.as_deref()) {
                reg.output_file_path(tid).await
            } else {
                None
            };

        Ok(AgentSpawnResponse {
            status: AgentSpawnStatus::AsyncLaunched,
            // Return the registry's `task_id` so the model can address the
            // spawn via TaskGet / TaskOutput / TaskStop. Falls back to
            // `agent_id` for compat with the legacy panel id.
            agent_id: Some(task_id.unwrap_or(agent_id)),
            result: None,
            error: None,
            total_tool_use_count: 0,
            total_tokens: 0,
            duration_ms: start.elapsed().as_millis() as i64,
            worktree_path: None,
            worktree_branch: None,
            output_file,
            prompt: None,
            ..Default::default()
        })
    }
}

/// Walk the child agent's message log and tally `tool_name` from every
/// assistant tool-call block. Memory telemetry uses the
/// `Write + Edit + NotebookEdit` count to populate
/// `MemoryEvent::ExtractionCompleted::files_written` without re-running
/// the LLM.
fn count_tool_uses_in_messages(
    messages: &[std::sync::Arc<coco_messages::Message>],
) -> std::collections::HashMap<String, i64> {
    let mut counts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for arc in messages {
        let coco_messages::Message::Assistant(a) = arc.as_ref() else {
            continue;
        };
        let coco_messages::LlmMessage::Assistant { content, .. } = &a.message else {
            continue;
        };
        for part in content {
            if let coco_messages::AssistantContent::ToolCall(tc) = part {
                *counts.entry(tc.tool_name.clone()).or_insert(0) += 1;
            }
        }
    }
    counts
}

/// Concatenate every assistant text part in the most recent
/// assistant message. Used for the `<result>` section of the
/// background-agent completion notification. TS:
/// `LocalAgentTask.tsx:249` `finalMessage`. Returns `None` when the
/// log has no assistant message or the last one is text-empty
/// (tool-only turn).
fn last_assistant_text(messages: &[std::sync::Arc<coco_messages::Message>]) -> Option<String> {
    for arc in messages.iter().rev() {
        let coco_messages::Message::Assistant(a) = arc.as_ref() else {
            continue;
        };
        let coco_messages::LlmMessage::Assistant { content, .. } = &a.message else {
            continue;
        };
        let mut chunks: Vec<String> = Vec::new();
        for part in content {
            if let coco_messages::AssistantContent::Text(t) = part
                && !t.text.is_empty()
            {
                chunks.push(t.text.clone());
            }
        }
        return if chunks.is_empty() {
            None
        } else {
            Some(chunks.join("\n"))
        };
    }
    None
}
