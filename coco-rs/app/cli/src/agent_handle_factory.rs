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
use tokio::sync::RwLock;
use tracing::warn;

use crate::session_runtime::SessionRuntime;

/// Hard cap on concurrent in-process subagents per session.
///
/// 16 mirrors the TS `MAX_TEAM_SIZE` (`utils/swarm/constants.ts`).
/// Going past this risks tokio task explosion on shared resources
/// (mailbox file IO, retry-jittered locks, per-agent state) without a
/// matching UX surface — the TUI's coordinator panel doesn't render
/// well past ~10 simultaneous agents either. Override only when a
/// specific workload genuinely needs more.
const MAX_IN_PROCESS_AGENTS: i32 = 16;

/// Per-runtime handles required to construct the production
/// `SwarmAgentHandle`. Splitting this out of `SessionRuntime::build`
/// avoids growing the build-options surface for an opt-in feature.
pub struct AgentTeamWiring {
    pub agent_handle: AgentHandleRef,
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
        MAX_IN_PROCESS_AGENTS,
    ));
    let team_manager = Arc::new(RwLock::new(None::<TeamManager>));

    let mut handle = SwarmAgentHandle::new(
        runner,
        team_manager,
        cwd.clone(),
        runtime.runtime_config.clone(),
    );

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
        Arc::new(move |mut engine_config, role| {
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

                // Resolve per-role client. `role_clients` caches
                // post-warm so this is effectively instant after the
                // first call per role; the cold path pays one
                // `build_api_client` cost per session per role.
                let client = match role {
                    Some(r) if r != coco_types::ModelRole::Main => {
                        runtime.client_for_role(r).await.unwrap_or_else(|e| {
                            warn!(?r, error = %e, "client_for_role failed; falling back to Main");
                            runtime.client.clone()
                        })
                    }
                    _ => runtime.client.clone(),
                };
                // Build a fresh engine via the runtime's standard
                // path, then swap in the role-resolved client.
                // `wire_engine` (called inside `build_engine_from_config`)
                // installs all the same observers / mailbox / hooks
                // the top-level engine gets so subagent execution
                // stays observable.
                let mut engine = runtime
                    .build_engine_from_config(
                        engine_config,
                        tokio_util::sync::CancellationToken::new(),
                        None,
                    )
                    .await;
                engine = engine.with_client(client);
                engine
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
