//! Shared session bootstrap helpers.
//!
//! TUI (`tui_runner::run_tui`) and SDK (`run_sdk_mode`) both build
//! the same per-session subsystems before handing off to their
//! respective event loops. This module owns the assembly logic so
//! the two runners cannot drift apart on what gets installed.
//!
//! Two stages:
//! 1. [`build_engine_resources`] resolves the model client, fallback
//!    chain, recovery policy, tool registry, and system prompt from
//!    the already-built [`coco_config::RuntimeConfig`].
//! 2. [`install_session_late_binds`] performs the post-`SessionRuntime::build`
//!    wirings (task runtime, agent transcript store, agent-team
//!    handle, fork dispatcher). The MCP handle is intentionally
//!    caller-driven because SDK constructs an `McpConnectionManager`
//!    that the dispatch handlers also need to mutate.

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;

use coco_commands::CommandRegistry;
use coco_commands::build_command_registry;
use coco_config::FallbackRecoveryPolicy;
use coco_config::RuntimeConfig;
use coco_config::global_config;
use coco_inference::ApiClient;
use coco_inference::model_factory::build_fallback_clients_for_role;
use coco_tool_runtime::ToolRegistry;
use coco_types::ModelRole;
use coco_types::ProviderApi;
use coco_types::UserType;

use crate::Cli;
use crate::headless::StartupPermissionState;
use crate::headless::build_output_style_manager;
use crate::headless::build_system_prompt_for_model;
use crate::headless::create_api_client;
use crate::headless::resolve_additional_dirs_display;
use crate::headless::resolve_startup_permission_state;
use crate::session_runtime::SessionRuntime;

/// Resources produced by [`build_engine_resources`]. Caller threads
/// these through into [`crate::session_runtime::SessionRuntimeBuildOpts`].
pub struct EngineResources {
    pub client: Arc<ApiClient>,
    pub fallback_clients: Vec<Arc<ApiClient>>,
    pub recovery_policy: Option<FallbackRecoveryPolicy>,
    pub tools: Arc<ToolRegistry>,
    pub system_prompt: String,
    pub model_id: String,
    pub provider_api: Option<ProviderApi>,
    pub startup: StartupPermissionState,
    /// Slash-command registry built once with the full TS-parity load
    /// order (builtins → extended → skills → plugin contributions →
    /// TS-parity P1 handlers). Both the SDK `initialize.commands`
    /// advertisement and the TUI `dispatch_slash_command` chain
    /// resolve through this slot. Wrapped in `RwLock` so
    /// `/reload-plugins` can hot-swap the inner `Arc<CommandRegistry>`
    /// without rebuilding the session — consumers snapshot the inner
    /// `Arc` once per dispatch (see
    /// [`crate::session_runtime::SessionRuntime::current_command_registry`]).
    pub command_registry: Arc<tokio::sync::RwLock<Arc<CommandRegistry>>>,
    /// Session-scoped `SkillManager`. Hoisted out of
    /// [`build_session_command_registry`] so the same instance is
    /// shared with the per-engine reminder pipeline (`SkillsSource`)
    /// instead of being dropped after building the command registry.
    pub skill_manager: Arc<coco_skills::SkillManager>,
    /// Resolved output-style catalog + active style. The CLI threads
    /// this into:
    /// - [`Self::system_prompt`] (already injected at build time)
    /// - `SessionBootstrap.output_style` (name only, for SDK init +
    ///   the per-turn reminder generator)
    /// - `CliInitializeBootstrap.{output_style, available_styles}`
    ///
    /// Stored here so SDK / TUI bootstraps don't each re-build it.
    pub output_style_manager: coco_output_styles::OutputStyleManager,
}

/// Build the shared engine resources from a resolved `RuntimeConfig`.
///
/// The `RuntimeConfig` itself is caller-built — TUI typically
/// snapshots it from a hot-reload publisher, SDK / headless build it
/// once via [`crate::headless::build_runtime_config_for_cli`].
/// Build a real LSP handle when `Feature::Lsp` is enabled and the
/// session has a workspace root + coco-home to load
/// `lsp_servers.json` from. Returns `None` otherwise — callers thread
/// the `None` into [`install_session_late_binds`] so the runtime's LSP
/// slot stays empty and `LspTool` is hidden from the model.
///
/// The constructed `LspServerManager` is wrapped in
/// [`crate::lsp_handle_adapter::LspManagerAdapter`] so file mutation
/// tools (Write / Edit / NotebookEdit) can dispatch `didSave` +
/// `clearDeliveredDiagnosticsForFile` through `ctx.lsp.notify_save`.
///
/// The adapter is **prewarmed** before returning (TS parity:
/// `manager.initialize()` at session bootstrap so `LSPTool.isEnabled`
/// reads accurate running-state by turn 1). Prewarm is best-effort —
/// servers that fail to spawn just flip the adapter's `is_connected`
/// gate to `false`, hiding the tool cleanly instead of throwing on the
/// first call.
pub async fn build_lsp_handle_if_enabled(
    runtime_config: &RuntimeConfig,
    coco_home: &Path,
    cwd: &Path,
) -> Option<coco_tool_runtime::LspHandleRef> {
    if !runtime_config.features.enabled(coco_types::Feature::Lsp) {
        return None;
    }
    let manager = coco_lsp::create_manager(Some(coco_home), Some(cwd.to_path_buf()));
    let adapter = crate::lsp_handle_adapter::LspManagerAdapter::new(manager);
    adapter.prewarm(cwd).await;
    Some(Arc::new(adapter))
}

pub fn build_engine_resources(
    cli: &Cli,
    runtime_config: &RuntimeConfig,
    cwd: &Path,
) -> Result<EngineResources> {
    tracing::info!(
        target: "coco_cli::session_bootstrap",
        cwd = %cwd.display(),
        "building engine resources"
    );

    let retry: coco_inference::RetryConfig = runtime_config.api.retry.clone().into();
    let (client, provider_api, model_id) = create_api_client(runtime_config, retry.clone());

    let fallback_clients = build_fallback_clients_for_role(
        runtime_config,
        ModelRole::Main,
        retry,
        Some(&crate::provider_login::shared_resolver()),
    )?;
    let recovery_policy = runtime_config.model_roles.recovery(ModelRole::Main);

    let registry = ToolRegistry::new();
    coco_tools::register_all_tools(&registry);
    let tool_count = registry.len();
    let tools = Arc::new(registry);

    // Resolve the active output style up front: it shapes the system
    // prompt cache prefix and surfaces on the SDK init message + the
    // per-turn reminder generator.
    let output_style_manager = build_output_style_manager(runtime_config, cwd, &[]);

    // `--add-dir` flow into the env block. TS:
    // `enhanceSystemPromptWithEnvDetails([...], model, additionalWorkingDirectories)`.
    // Single source of truth lives in `headless::resolve_additional_dirs_display`.
    let additional_working_directories = resolve_additional_dirs_display(cli, cwd);

    let system_prompt = build_system_prompt_for_model(
        cwd,
        runtime_config,
        client.provider(),
        &model_id,
        output_style_manager.active(),
        &additional_working_directories,
    );

    let startup = resolve_startup_permission_state(cli, &runtime_config.settings.merged)?;

    let (command_registry, skill_manager) = build_session_command_registry(runtime_config, cwd);
    let command_count = command_registry.len();
    let skill_count = skill_manager.len();

    tracing::info!(
        target: "coco_cli::session_bootstrap",
        provider = client.provider(),
        model_id = %model_id,
        real_provider = provider_api.is_some(),
        fallback_count = fallback_clients.len(),
        recovery_policy_set = recovery_policy.is_some(),
        tool_count,
        command_count,
        skill_count,
        permission_mode = ?startup.mode,
        bypass_available = startup.bypass_available,
        sandbox_mode = ?runtime_config.sandbox.mode,
        system_prompt_chars = system_prompt.len(),
        "engine resources built"
    );
    tracing::debug!(
        target: "coco_cli::session_bootstrap",
        max_turns = ?runtime_config.loop_config.max_turns,
        total_token_budget = ?runtime_config.loop_config.total_token_budget,
        streaming_tools = runtime_config.loop_config.enable_streaming_tools,
        auto_compact_enabled = runtime_config.compact.auto.enabled,
        auto_compact_disabled_by_env = runtime_config.compact.auto.auto_disabled_by_env,
        memory_extraction = runtime_config.memory.extraction_enabled,
        web_fetch_enabled = runtime_config.features.enabled(coco_types::Feature::WebFetch),
        web_search_enabled = runtime_config.features.enabled(coco_types::Feature::WebSearch),
        retrieval_enabled = runtime_config.features.enabled(coco_types::Feature::Retrieval),
        "runtime config public knobs"
    );

    Ok(EngineResources {
        client,
        fallback_clients,
        recovery_policy,
        tools,
        system_prompt,
        model_id,
        provider_api,
        startup,
        command_registry: Arc::new(tokio::sync::RwLock::new(Arc::new(command_registry))),
        skill_manager,
        output_style_manager,
    })
}

/// Construct the TS-parity slash-command registry (builtins → extended
/// → skills → plugin contributions → TS-parity P1 handlers) AND return
/// the `SkillManager` Arc that backed the skill-derived commands. The
/// caller threads the manager into `SessionRuntime` so the per-turn
/// reminder pipeline's `SkillsSource` reads the same in-memory catalog.
fn build_session_command_registry(
    runtime_config: &RuntimeConfig,
    cwd: &Path,
) -> (CommandRegistry, Arc<coco_skills::SkillManager>) {
    let config_home = global_config::config_home();

    let skill_manager = coco_skills::SkillManager::new();
    skill_manager.load_from_dirs(&[config_home.join("skills"), cwd.join(".coco").join("skills")]);
    let skill_manager = Arc::new(skill_manager);

    let mut plugin_manager = coco_plugins::PluginManager::new();
    plugin_manager.load_from_dirs(&coco_plugins::get_plugin_dirs(&config_home, cwd));

    let registry = build_command_registry(
        skill_manager.as_ref(),
        &plugin_manager,
        UserType::from_env(),
        runtime_config.features.clone(),
        cwd.to_path_buf(),
        dirs::home_dir().unwrap_or_else(|| cwd.to_path_buf()),
        /*managed_root*/ None,
        &runtime_config.skill_overrides,
    );
    (registry, skill_manager)
}

/// Install the post-`SessionRuntime::build` late-binds shared by TUI
/// and SDK. Without this both runners must independently remember to
/// attach `task_runtime`, `agent_transcript_store`, the agent-team
/// wiring, and the fork dispatcher — TUI used to forget all four,
/// causing background AgentTool, transcript resume, and `/btw` to
/// silently degrade to no-ops.
///
/// `mcp_handle` is optional because TUI does not yet bootstrap an
/// `McpConnectionManager`. SDK passes `Some(handle)` and gets the
/// original install ordering preserved (mcp before agent-team).
///
/// `lsp_handle` is optional and **independently gated** by
/// [`coco_types::Feature::Lsp`] at the caller (CLI / SDK / TUI). When
/// `None`, the runtime's LSP slot stays unset and
/// `LspTool::is_enabled()` reports `false` (via `NoOpLspHandle`), so
/// the tool is hidden from the model's tool list — TS parity
/// (`LSPTool.isEnabled() = isLspConnected()`).
pub async fn install_session_late_binds(
    runtime: Arc<SessionRuntime>,
    cwd: &Path,
    mcp_handle: Option<coco_tool_runtime::McpHandleRef>,
    lsp_handle: Option<coco_tool_runtime::LspHandleRef>,
) -> Result<()> {
    // Background task runtime — owns the `TaskManager` and per-task
    // disk output; shared with `SwarmAgentHandle` so AgentTool
    // background spawns and the engine's `Task*` tools see one
    // source of truth.
    //
    // Disk-output session dir mirrors TS's
    // `getProjectTempDir()/{sessionId}/tasks/`. Captured ONCE here so
    // subsequent `/clear` regenerations don't invalidate paths held
    // by in-flight `DiskTaskOutput` instances.
    let task_session_id = runtime.current_session_id().await;
    let task_session_dir = coco_config::global_config::config_home()
        .join("cache")
        .join("tasks")
        .join(&task_session_id);
    // Wire the session-scoped `CommandQueue` into the TaskRuntime
    // via the `NotificationSink` trait so terminal lifecycle events
    // (mark_completed / mark_failed / kill_task / bg shell exit)
    // push a `<task-notification>` envelope onto the queue. The
    // engine's per-turn drain
    // (`engine_finalize_turn::drain_command_queue_into_history`)
    // then injects it as a User message wrapped in
    // `<system-reminder>` — TS parity for
    // `enqueuePendingNotification({mode: 'task-notification'})`.
    let sink: coco_tasks::NotificationSinkRef = Arc::new(
        crate::command_queue_sink::CommandQueueNotificationSink::new(
            runtime.command_queue().clone(),
        ),
    );
    let task_runtime = Arc::new(
        crate::task_runtime::TaskRuntime::with_session_dir(
            Arc::new(coco_tasks::TaskManager::new()),
            task_session_dir,
        )
        .with_notification_sink(sink),
    );
    runtime.attach_task_runtime(task_runtime).await;
    let task_list_id = coco_tasks::resolve_task_list_id(None, None, &task_session_id);
    let task_list_root = coco_config::global_config::config_home().join("tasks");
    let task_list_router =
        crate::team_task_list_router::RoutedTaskList::open(task_list_root, task_list_id)?;
    runtime
        .attach_task_list(task_list_router.clone() as coco_tool_runtime::TaskListHandleRef)
        .await;
    runtime
        .attach_team_task_list_router(task_list_router as coco_tool_runtime::TeamTaskListRouterRef)
        .await;

    // Per-agent transcript persistence (TS-faithful resume). The
    // project paths match the runtime's transcript path so
    // `<project_dir>/<session_id>/subagents/agent-<id>.*` lives
    // alongside the main session JSONL.
    let agent_transcript_store: Arc<dyn coco_tool_runtime::AgentTranscriptStore> = Arc::new(
        crate::agent_transcript_persistence::SessionAgentTranscriptStore::new(Arc::new(
            coco_session::TranscriptStore::new(crate::paths::project_paths(cwd)),
        )),
    );
    runtime
        .attach_agent_transcript_store(agent_transcript_store)
        .await;

    // MCP handle (if any). Installed BEFORE `install_agent_team` so
    // AgentTool's prompt-time MCP filter sees a populated handle on
    // the very first engine build, matching the original SDK ordering.
    if let Some(handle) = mcp_handle {
        runtime.attach_mcp_handle(handle).await;
    }

    // LSP handle install: same pattern as MCP. When the caller did the
    // `Feature::Lsp` gate + manager construction (CLI / SDK), the
    // handle threads in here so per-turn engines pick it up via
    // `wire_engine`. TUI passes `None` (no LSP boot yet).
    if let Some(handle) = lsp_handle {
        runtime.attach_lsp_handle(handle).await;
    }

    // Agent wiring (`SwarmAgentHandle` + `QueryEngineAdapter`
    // factory). LocalAgent requires the TaskRuntime registry installed
    // above; team-specific tools remain feature-gated at the tool layer.
    crate::agent_handle_factory::install_agent_team(runtime.clone(), cwd.display().to_string())
        .await?;

    // Post-turn fork dispatcher (`/btw`, `promptSuggestion`). Captures
    // `Arc<SessionRuntime>` and routes every dispatch through
    // `build_engine_from_config`, leaving the parent loop untouched.
    crate::fork_dispatcher::install(runtime).await;

    Ok(())
}

#[cfg(test)]
#[path = "session_bootstrap.test.rs"]
mod tests;
