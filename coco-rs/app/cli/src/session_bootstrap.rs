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
use crate::headless::build_system_prompt_for_model;
use crate::headless::create_api_client;
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
    /// resolve through this Arc.
    pub command_registry: Arc<CommandRegistry>,
}

/// Build the shared engine resources from a resolved `RuntimeConfig`.
///
/// The `RuntimeConfig` itself is caller-built — TUI typically
/// snapshots it from a hot-reload publisher, SDK / headless build it
/// once via [`crate::headless::build_runtime_config_for_cli`].
pub fn build_engine_resources(
    cli: &Cli,
    runtime_config: &RuntimeConfig,
    cwd: &Path,
) -> Result<EngineResources> {
    let retry: coco_inference::RetryConfig = runtime_config.api.retry.clone().into();
    let (client, provider_api, model_id) = create_api_client(runtime_config, retry.clone());

    let fallback_clients = build_fallback_clients_for_role(runtime_config, ModelRole::Main, retry)?;
    let recovery_policy = runtime_config.model_roles.recovery(ModelRole::Main);

    let registry = ToolRegistry::new();
    coco_tools::register_all_tools(&registry);
    let tools = Arc::new(registry);

    let system_prompt =
        build_system_prompt_for_model(cwd, runtime_config, client.provider(), &model_id);

    let startup = resolve_startup_permission_state(cli, &runtime_config.settings.merged)?;

    let command_registry = Arc::new(build_session_command_registry(runtime_config, cwd));

    Ok(EngineResources {
        client,
        fallback_clients,
        recovery_policy,
        tools,
        system_prompt,
        model_id,
        provider_api,
        startup,
        command_registry,
    })
}

/// Construct the TS-parity slash-command registry (builtins → extended
/// → skills → plugin contributions → TS-parity P1 handlers). Pulled out
/// of [`build_engine_resources`] so the per-step manager loads stay
/// self-contained.
fn build_session_command_registry(runtime_config: &RuntimeConfig, cwd: &Path) -> CommandRegistry {
    let config_home = global_config::config_home();

    let mut skill_manager = coco_skills::SkillManager::new();
    skill_manager.load_from_dirs(&[config_home.join("skills"), cwd.join(".coco").join("skills")]);

    let mut plugin_manager = coco_plugins::PluginManager::new();
    plugin_manager.load_from_dirs(&coco_plugins::get_plugin_dirs(&config_home, cwd));

    build_command_registry(
        &skill_manager,
        &plugin_manager,
        UserType::from_env(),
        runtime_config.features.clone(),
        cwd.to_path_buf(),
        dirs::home_dir().unwrap_or_else(|| cwd.to_path_buf()),
        /*managed_root*/ None,
    )
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
pub async fn install_session_late_binds(
    runtime: Arc<SessionRuntime>,
    cwd: &Path,
    mcp_handle: Option<coco_tool_runtime::McpHandleRef>,
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
    let task_runtime = Arc::new(crate::task_runtime::TaskRuntime::with_session_dir(
        Arc::new(coco_tasks::TaskManager::new()),
        task_session_dir,
    ));
    runtime.attach_task_runtime(task_runtime).await;

    // Per-agent transcript persistence (TS-faithful resume). The
    // sessions_dir matches the runtime's transcript path so
    // `<sessions_dir>/<session_id>/subagents/agent-<id>.*` lives
    // alongside the main session JSONL.
    let agent_transcript_store: Arc<dyn coco_tool_runtime::AgentTranscriptStore> = Arc::new(
        crate::agent_transcript_persistence::SessionAgentTranscriptStore::new(Arc::new(
            coco_session::TranscriptStore::new(crate::paths::sessions_dir()),
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

    // Agent-team wiring (`SwarmAgentHandle` + `QueryEngineAdapter`
    // factory). No-op when `Feature::AgentTeams` is off.
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
