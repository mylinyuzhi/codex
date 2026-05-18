//! Bootstrap-time assembly of the production [`SwarmAgentHandle`].
//!
//! The handle owns the in-process runner + team manager + worktree
//! manager and is the single seam through which `AgentTool` /
//! `SendMessage` / `TeamCreate` / `TeamDelete` reach the coordinator.
//! Today the only other implementation is `NoOpAgentHandle` (returns
//! "not available in this context" for every call), used when
//! `Feature::AgentTeams` is off or in test fixtures.
//!
//! The factory closure inside [`coco_query::QueryEngineAdapter`] is
//! the trickiest piece — it needs to spawn a fresh `QueryEngine` per
//! subagent call, route the call's `ModelRole` to the right
//! `ApiClient`, and thread it back through `SessionRuntime`'s
//! standard wiring (compaction observers, mailbox, hooks, etc.). This
//! module owns the closure construction so `app/cli/main.rs` doesn't
//! grow a 50-line lambda.
//!
//! TS parity — none. TS spawns subagents via `runAgent.ts` directly
//! against the parent's `claudeAPIClient`; multi-provider role
//! resolution is a Rust-only feature so the wiring lives here.

use std::sync::Arc;

use coco_coordinator::agent_handle::SwarmAgentHandle;
use coco_coordinator::runner::InProcessAgentRunner;
use coco_coordinator::types::TeamManager;
use coco_query::agent_adapter::{QueryEngineAdapter, QueryEngineFactory};
use coco_tool_runtime::AgentHandleRef;
use coco_tool_runtime::AgentQueryEngineRef;
use coco_types::LlmModelSelection;
use coco_types::ModelRole;
use coco_types::ModelSpec;
use coco_types::ProviderModelSelection;
use tokio::sync::RwLock;
use tracing::warn;

use crate::session_runtime::SessionRuntime;

/// Per-runtime handles required to construct the production
/// `SwarmAgentHandle`. Splitting this out of `SessionRuntime::build`
/// avoids growing the build-options surface for an opt-in feature.
pub struct AgentTeamWiring {
    pub agent_handle: AgentHandleRef,
}

async fn resolve_agent_client(
    runtime: &Arc<SessionRuntime>,
    selection: &LlmModelSelection,
) -> Arc<coco_inference::ApiClient> {
    match selection {
        LlmModelSelection::InheritMain
        | LlmModelSelection::Role {
            role: ModelRole::Main,
        } => runtime.main_client().await,
        LlmModelSelection::Role { role } => resolve_role_client(runtime, *role).await,
        LlmModelSelection::Explicit { primary } => {
            match resolve_explicit_client(runtime, primary).await {
                Some(client) => client,
                None => runtime.main_client().await,
            }
        }
        LlmModelSelection::ExplicitWithFallbackRole {
            primary,
            fallback_role,
        } => match resolve_explicit_client(runtime, primary).await {
            Some(client) => client,
            None => resolve_role_client(runtime, *fallback_role).await,
        },
    }
}

async fn resolve_role_client(
    runtime: &Arc<SessionRuntime>,
    role: ModelRole,
) -> Arc<coco_inference::ApiClient> {
    if role == ModelRole::Main {
        return runtime.main_client().await;
    }

    match runtime.client_for_role(role).await {
        Ok(client) => client,
        Err(e) => {
            warn!(?role, error = %e, "client_for_role failed; falling back to Main");
            runtime.main_client().await
        }
    }
}

async fn resolve_explicit_client(
    runtime: &Arc<SessionRuntime>,
    selection: &ProviderModelSelection,
) -> Option<Arc<coco_inference::ApiClient>> {
    let provider = match runtime.runtime_config.providers.get(&selection.provider) {
        Some(provider) => provider,
        None => {
            warn!(
                provider = %selection.provider,
                model = %selection.model_id,
                "explicit model references unknown provider; falling back"
            );
            return None;
        }
    };
    let spec = ModelSpec {
        provider: selection.provider.clone(),
        api: provider.api,
        display_name: selection.model_id.clone(),
        model_id: selection.model_id.clone(),
    };
    let retry: coco_inference::RetryConfig = runtime.runtime_config.api.retry.clone().into();
    match coco_inference::model_factory::build_api_client(&runtime.runtime_config, &spec, retry) {
        Ok(client) => Some(client),
        Err(e) => {
            warn!(
                provider = %selection.provider,
                model = %selection.model_id,
                error = %e,
                "failed to build explicit model ApiClient; falling back"
            );
            None
        }
    }
}

/// Assemble the production [`SwarmAgentHandle`] for `runtime`.
///
/// Returns `None` when the runtime's resolved features don't include
/// `Feature::AgentTeams` — callers should treat this as "no spawn
/// support installed; AgentTool will degrade to NoOpAgentHandle".
///
/// **Late-bind contract**: this fn must run *after*
/// `SessionRuntime::build` has returned the `Arc`, because the
/// `QueryEngineAdapter` factory captures `Arc<SessionRuntime>` to
/// drive per-spawn engine assembly. Calling it from inside `build()`
/// would force a cycle (`Arc::new_cyclic` works, but the resulting
/// closure-vs-Arc dance is far less readable than two-phase init).
pub async fn build_agent_team_wiring(
    runtime: Arc<SessionRuntime>,
    cwd: String,
) -> Option<AgentTeamWiring> {
    if !runtime
        .runtime_config
        .features
        .enabled(coco_types::Feature::AgentTeams)
    {
        return None;
    }

    // In-process subagents inherit the leader's `ToolPermissionBridge`
    // (installed on `SessionRuntime` and propagated by `wire_engine`):
    // SDK leaders forward over `approval/askForApproval`, TUI leaders
    // drive the permission overlay (P0 work). No extra channel needed
    // here — the prior in-process mpsc circuit was orphaned and
    // removed in the post-D cleanup pass.
    let runner = Arc::new(InProcessAgentRunner::new(
        cwd.clone(),
        runtime.runtime_config.agent_teams.max_agents,
    ));
    let team_manager = Arc::new(RwLock::new(None::<TeamManager>));

    let mut handle = SwarmAgentHandle::new(
        runner.clone(),
        team_manager,
        cwd.clone(),
        runtime.runtime_config.clone(),
    );
    let backend_registry = Arc::new(coco_coordinator::pane::BackendRegistry::new());
    backend_registry
        .register_in_process_backend(Arc::new(coco_coordinator::InProcessBackend::new(
            runner.clone(),
        )))
        .await;
    let detection = backend_registry.detect_backend().await;
    match detection.backend_type {
        coco_coordinator::BackendType::Tmux => {
            backend_registry
                .register_pane_backend(Arc::new(coco_coordinator::pane::tmux::TmuxBackend::new(
                    detection.is_native,
                )))
                .await;
        }
        coco_coordinator::BackendType::Iterm2 => {
            backend_registry
                .register_pane_backend(
                    Arc::new(coco_coordinator::pane::iterm2::ITermBackend::new()),
                )
                .await;
        }
        coco_coordinator::BackendType::InProcess => {
            backend_registry.mark_in_process_fallback().await;
        }
        _ => {
            backend_registry.mark_in_process_fallback().await;
        }
    }
    handle.set_backend_registry(backend_registry);

    // P2'+: install the AgentTaskRegistry side of the production
    // task runtime so AgentTool background spawns register through
    // the same `TaskManager` the engine's `Task*` tools read from.
    // `runtime.current_task_runtime()` is `Some` only when CLI
    // bootstrap installed it (via `install` below) — tests that
    // construct `SessionRuntime` directly skip this and bg spawns
    // run unregistered.
    if let Some(task_rt) = runtime.current_task_runtime().await {
        handle.set_task_registry(task_rt as coco_tool_runtime::AgentTaskRegistryRef);
    }
    if let Some(task_list) = runtime.current_task_list().await {
        handle.set_task_list(task_list);
    }
    if let Some(router) = runtime.current_team_task_list_router().await {
        handle.set_team_task_list_router(router);
    }

    // Per-agent transcript store for `SwarmAgentHandle::resume_agent`
    // (TS parity: `tools/AgentTool/resumeAgent.ts`). The same Arc is
    // shared across spawns so concurrent bg agents read/write
    // through one store; absent only in minimal test embeddings.
    if let Some(transcript_store) = runtime.current_agent_transcript_store().await {
        handle.set_transcript_store(transcript_store);
    }

    // Wire the per-spawn engine factory. Captures `Arc<SessionRuntime>`
    // and resolves a fresh role-aware engine on each call. The
    // factory is async (`QueryEngineFactory` returns a boxed future)
    // so it can call `build_engine_from_config` + `client_for_role`
    // directly without blocking the runtime.
    let factory: QueryEngineFactory = {
        let runtime_for_factory = runtime.clone();
        Arc::new(move |mut engine_config, role, cancel| {
            let runtime = runtime_for_factory.clone();
            Box::pin(async move {
                // ── Gap A fix — inherit parent's resolved RuntimeConfig ──
                //
                // Before this fix, `agent_adapter::execute_query` populated
                // 8 policy fields with `*::default()` and the engine then
                // read its own `self.config.*` for those settings — so
                // subagent compact / sandbox / web / shell / memory /
                // plan-mode / system-reminder / tool config silently
                // ignored the user's `~/.coco/settings.json`.
                //
                // TS parity: `runAgent.ts:667-695` shares the parent's
                // `toolUseContext.options` directly; subagents see the
                // exact same config tree the leader does.
                //
                // Overwrite those fields here from the live RuntimeConfig
                // so subagent engines compact at the user's tuned
                // thresholds, honour the user's sandbox mode, etc.
                engine_config.compact = runtime.runtime_config.compact.clone();
                engine_config.system_reminder = runtime
                    .runtime_config
                    .settings
                    .merged
                    .system_reminder
                    .clone();
                engine_config.tool_config = runtime.runtime_config.tool.clone();
                engine_config.sandbox_config = runtime.runtime_config.sandbox.clone();
                engine_config.memory_config = runtime.runtime_config.memory.clone();
                engine_config.shell_config = runtime.runtime_config.shell.clone();
                engine_config.web_fetch_config = runtime.runtime_config.web_fetch.clone();
                engine_config.web_search_config = runtime.runtime_config.web_search.clone();
                engine_config.plan_mode_settings =
                    runtime.runtime_config.settings.merged.plan_mode.clone();

                let client = resolve_agent_client(&runtime, &role).await;
                // Build a fresh engine via the runtime's standard
                // path, then swap in the role-resolved client.
                // `wire_engine` (called inside `build_engine_from_config`)
                // installs all the same observers / mailbox / hooks
                // the top-level engine gets so subagent execution
                // stays observable.
                let engine = runtime
                    .build_engine_from_config(
                        engine_config,
                        cancel.unwrap_or_else(tokio_util::sync::CancellationToken::new),
                        None,
                    )
                    .await;
                // Per-engine `live_command_rules` is fresh for every
                // forked subagent (constructed inside
                // `QueryEngine::new`), so the subagent's skill rules
                // cannot leak to the parent — different Arcs. No
                // NoOp override needed: TS parity stays without an
                // explicit isolation seam.
                engine.with_client(client)
            })
        })
    };
    let adapter: AgentQueryEngineRef = Arc::new(QueryEngineAdapter::new(factory));
    handle.set_execution_engine(adapter.clone());

    // ── Gap C fix — install teammate execution engine ──
    //
    // The same QueryEngineAdapter that drives subagent spawns also
    // drives in-process teammates via the coordinator's runner-loop,
    // bridged through `TeammateExecutionAdapter`. Without this,
    // `spawn_teammate` registered teammates that never executed.
    //
    // Auto-compact threshold flows from the user's CompactConfig so
    // teammates compact at the same tuning as the leader. Default
    // 100k matches the threshold-formula floor when CompactConfig is
    // unset.
    handle.set_teammate_execution_engine(coco_coordinator::agent_handle::into_execution_engine(
        adapter,
    ));
    let auto_compact_threshold = coco_compact::auto_compact_threshold(
        /*context_window*/ 200_000,
        /*max_output_tokens*/ 16_384,
        &runtime.runtime_config.compact.auto,
    );
    handle.set_teammate_auto_compact_threshold(auto_compact_threshold);

    // Pass the leader's full system prompt as the teammate base. The
    // runner-loop composes this with `TEAMMATE_PROMPT_ADDENDUM` so
    // teammates inherit the same CLAUDE.md + env-context + memory
    // blocks the leader uses (TS parity: `inProcessRunner.ts` builds
    // the teammate prompt by composing `getSystemPrompt(...)` with
    // the team addendum).
    if let Some(base_prompt) = runtime.current_engine_config().await.system_prompt.clone() {
        handle.set_teammate_base_system_prompt(base_prompt).await;
    }

    // Wire the hook registry so SubagentStart / SubagentStop hooks fire
    // around subagent execution. TS parity: `runAgent.ts:530-555`.
    handle.set_hook_registry(runtime.hook_registry.clone());

    // Wire the MCP handle so per-agent inline `mcpServers: [{name: config}]`
    // entries get registered as dynamic servers at spawn and torn down
    // at SubagentStop. TS parity: `runAgent.ts:95-218
    // initializeAgentMcpServers`. String-ref entries don't need this
    // wire — they reuse the parent's pre-existing connection.
    if let Some(mcp) = runtime.current_mcp_handle().await {
        handle.set_mcp_handle(mcp);
    }

    // Auto-sync per-agent project snapshots into local memory dirs at
    // bootstrap. TS parity: `loadAgentsDir.ts:268-282` calls
    // `checkAgentMemorySnapshot` + `initializeFromSnapshot` while
    // loading agent definitions. `prompt-update` is policy-treated as
    // an automatic re-sync here — Rust has no interactive prompt at
    // bootstrap and forcing manual approval would leave newer team
    // baselines silently unconsumed.
    sync_agent_memory_snapshots(&runtime, &cwd).await;

    Some(AgentTeamWiring {
        agent_handle: Arc::new(handle),
    })
}

/// Walk every known agent type's snapshot dir under
/// `<cwd>/.coco/agent-memory-snapshots/` and apply the appropriate
/// `SnapshotAction` for each (User / Project / Local) scope. Errors
/// are logged + swallowed — a snapshot sync failure must not gate
/// session startup.
async fn sync_agent_memory_snapshots(_runtime: &Arc<SessionRuntime>, cwd: &str) {
    let cwd_path = std::path::PathBuf::from(cwd);
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    let snapshots_root = cwd_path
        .join(".coco")
        .join(coco_memory::agent_memory_snapshot::SNAPSHOT_BASE);

    let entries = match std::fs::read_dir(&snapshots_root) {
        Ok(e) => e,
        Err(_) => return, // No snapshot tree at all — nothing to do.
    };

    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let agent_type = entry.file_name().to_string_lossy().to_string();
        for scope in [
            coco_types::MemoryScope::User,
            coco_types::MemoryScope::Project,
            coco_types::MemoryScope::Local,
        ] {
            let action = coco_memory::agent_memory_snapshot::check_agent_memory_snapshot(
                &agent_type,
                scope,
                &cwd_path,
                &home,
            );
            match action {
                coco_memory::agent_memory_snapshot::SnapshotAction::Initialize {
                    snapshot_timestamp,
                } => {
                    if let Err(e) = coco_memory::agent_memory_snapshot::initialize_from_snapshot(
                        &agent_type,
                        scope,
                        &snapshot_timestamp,
                        &cwd_path,
                        &home,
                    ) {
                        warn!(error = %e, %agent_type, ?scope, "snapshot initialize failed");
                    }
                }
                coco_memory::agent_memory_snapshot::SnapshotAction::PromptUpdate {
                    snapshot_timestamp,
                } => {
                    if let Err(e) = coco_memory::agent_memory_snapshot::replace_from_snapshot(
                        &agent_type,
                        scope,
                        &snapshot_timestamp,
                        &cwd_path,
                        &home,
                    ) {
                        warn!(error = %e, %agent_type, ?scope, "snapshot replace failed");
                    }
                }
                coco_memory::agent_memory_snapshot::SnapshotAction::None => {}
            }
        }
    }
}

/// Convenience for one-shot bootstrap: build the wiring and attach it
/// to the runtime. No-op (returns `Ok`) when `Feature::AgentTeams` is
/// off.
pub async fn install_agent_team(runtime: Arc<SessionRuntime>, cwd: String) -> anyhow::Result<()> {
    if let Some(wiring) = build_agent_team_wiring(runtime.clone(), cwd).await {
        runtime.attach_agent_handle(wiring.agent_handle).await;
    }
    Ok(())
}
