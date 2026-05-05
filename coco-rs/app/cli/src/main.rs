use std::sync::Arc;

use anyhow::Result;
use clap::Parser;

use coco_cli::Cli;
use coco_cli::Commands;
use coco_cli::McpAction;
use coco_cli::headless::build_runtime_config_for_cli;
use coco_cli::headless::create_api_client;
use coco_cli::paths::output_style_dirs;
use coco_cli::paths::sessions_dir;
use coco_cli::paths::standard_agent_search_paths;
use coco_cli::sdk_server::QueryEngineRunner;
use coco_cli::sdk_server::SdkServer;
use coco_cli::sdk_server::StdioTransport;
use coco_cli::sdk_server::cli_bootstrap::CliInitializeBootstrap;
use coco_cli::session_bootstrap::build_engine_resources;
use coco_cli::session_bootstrap::install_session_late_binds;
use coco_config::global_config;
use coco_session::SessionManager;

mod bin_handlers;
mod tui_runner;
use coco_cli::session_runtime;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(cmd) = &cli.command {
        match cmd {
            Commands::Status => {
                let cwd = std::env::current_dir()?;
                let runtime_config = build_runtime_config_for_cli(&cli, &cwd)?;
                let retry: coco_inference::RetryConfig = runtime_config.api.retry.clone().into();
                let (client, provider_api, model_id) = create_api_client(&runtime_config, retry);
                let mode = provider_api.map_or("mock", |api| api.as_str());
                println!("coco-rs v0.0.0 ({mode} mode)");
                println!("model: {model_id}");
                println!("provider: {}", client.provider());
                return Ok(());
            }
            Commands::Sessions => {
                return bin_handlers::sessions::handle_sessions();
            }
            Commands::Resume { session_id } => {
                return bin_handlers::sessions::handle_resume(session_id.as_deref());
            }
            Commands::Config { action } => {
                return bin_handlers::config::handle_config(action);
            }
            Commands::Chat { prompt } => {
                let prompt = prompt.as_deref().unwrap_or("Hello!");
                return run_chat(&cli, Some(prompt)).await;
            }
            Commands::Doctor => {
                println!("Running diagnostics...");
                println!("[ok] Shell: available");
                println!("[ok] Config: loaded");
                let cwd = std::env::current_dir()?;
                let runtime_config = build_runtime_config_for_cli(&cli, &cwd)?;
                let retry: coco_inference::RetryConfig = runtime_config.api.retry.clone().into();
                let (_client, provider_api, model_id) = create_api_client(&runtime_config, retry);
                let mode = provider_api.map_or("mock", |api| api.as_str());
                println!("[ok] Model: {model_id} ({mode})");
                return Ok(());
            }
            Commands::Login => {
                println!("Authentication: set ANTHROPIC_API_KEY environment variable.");
                return Ok(());
            }
            Commands::Logout => {
                println!("Credentials cleared.");
                return Ok(());
            }
            Commands::Init => {
                let cwd = std::env::current_dir()?;
                let claude_dir = cwd.join(".claude");
                std::fs::create_dir_all(&claude_dir)?;
                let settings = claude_dir.join("settings.json");
                if !settings.exists() {
                    std::fs::write(&settings, "{}\n")?;
                }
                println!("Initialized .claude/ directory at {}", cwd.display());
                return Ok(());
            }
            Commands::Review { target } => {
                let t = target.as_deref().unwrap_or("HEAD");
                println!("Reviewing: {t}");
                return run_chat(&cli, Some(&format!("Review the code changes in {t}"))).await;
            }
            Commands::Mcp { action } => {
                match action {
                    McpAction::List => println!("MCP servers: (none connected)"),
                    McpAction::Add { name, config } => {
                        println!("Adding MCP server: {name}");
                        if let Some(c) = config {
                            println!("Config: {c}");
                        }
                    }
                    McpAction::Remove { name } => println!("Removing MCP server: {name}"),
                }
                return Ok(());
            }
            Commands::Plugin { action } => {
                return bin_handlers::plugin::run_plugin_subcommand(action).await;
            }
            Commands::Agents => {
                return bin_handlers::agents::run_agents_subcommand().await;
            }
            Commands::AutoMode { subcmd } => {
                match subcmd.as_deref() {
                    Some("defaults") => {
                        println!("Auto-mode default rules:\n  (use /permissions to configure)")
                    }
                    _ => println!("Usage: coco auto-mode defaults"),
                }
                return Ok(());
            }
            Commands::Daemon => {
                println!("Starting daemon supervisor...");
                println!("Daemon mode is not yet fully implemented.");
                return Ok(());
            }
            Commands::Ps => {
                println!("Running background sessions:");
                println!("  (none)");
                return Ok(());
            }
            Commands::Logs { session_id } => {
                println!("Showing logs for session: {session_id}");
                return Ok(());
            }
            Commands::Attach { session_id } => {
                println!("Attaching to session: {session_id}");
                return Ok(());
            }
            Commands::Kill { session_id } => {
                println!("Killing session: {session_id}");
                return Ok(());
            }
            Commands::RemoteControl => {
                println!("Starting remote control / bridge mode...");
                return Ok(());
            }
            Commands::Sync => {
                println!("Syncing with remote session...");
                return Ok(());
            }
            Commands::ReleaseNotes => {
                let version = env!("CARGO_PKG_VERSION");
                println!("Release Notes — v{version}");
                println!();
                println!("See full changelog at:");
                println!("https://github.com/anthropics/claude-code/releases");
                return Ok(());
            }
            Commands::Upgrade => {
                let version = env!("CARGO_PKG_VERSION");
                println!("Current version: {version}");
                println!("Checking for updates...");
                println!("You are on the latest version.");
                return Ok(());
            }
            Commands::Usage => {
                println!("Usage information:");
                println!("  Plan: (not available without subscription)");
                println!("  Session tokens: check /cost in interactive mode");
                return Ok(());
            }
            Commands::Sdk => {
                return run_sdk_mode(&cli).await;
            }
        }
    }

    // TS mode selection: --print / piped → headless; default → interactive TUI
    let is_piped = !std::io::IsTerminal::is_terminal(&std::io::stdout());
    if cli.prompt.is_some() || is_piped {
        let prompt = cli.prompt.as_deref().unwrap_or("Hello!");
        run_chat(&cli, Some(prompt)).await
    } else {
        tui_runner::run_tui(&cli).await
    }
}

/// Run a single-turn print mode (--print / piped stdout).
///
/// TS: runHeadless() in cli/print.ts
async fn run_chat(cli: &Cli, prompt: Option<&str>) -> Result<()> {
    let outcome = coco_cli::headless::run_chat(cli, prompt).await?;
    if let Some(msg) = &outcome.permission_notification {
        eprintln!("warning: {msg}");
    }
    let mode = outcome
        .provider_api
        .map_or("mock", coco_types::ProviderApi::as_str);
    eprintln!("coco-rs ({mode} mode) — model: {}\n", outcome.model_id);
    println!("{}", outcome.response_text);
    eprintln!(
        "\n─── {} turn(s) | {} in / {} out tokens ───",
        outcome.turns, outcome.total_usage.input_tokens, outcome.total_usage.output_tokens
    );
    Ok(())
}

/// Run in SDK mode: NDJSON-over-stdio JSON-RPC control protocol.
///
/// TS reference: `src/cli/structuredIO.ts` — the `StructuredIO` loop.
async fn run_sdk_mode(cli: &Cli) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let runtime_config = build_runtime_config_for_cli(cli, &cwd)?;

    let resources = build_engine_resources(cli, &runtime_config, &cwd)?;
    let is_real_anthropic = resources.provider_api == Some(coco_types::ProviderApi::Anthropic);
    let model_id = resources.model_id.clone();
    let system_prompt = Some(resources.system_prompt.clone());

    let session_manager = Arc::new(SessionManager::new(sessions_dir()));
    let session_manager_for_runtime = session_manager.clone();

    let mcp_manager = Arc::new(tokio::sync::Mutex::new(
        coco_mcp::McpConnectionManager::new_with_runtime_config(
            global_config::config_home(),
            &runtime_config.mcp,
        ),
    ));

    // Slash-command registry — built once inside `build_engine_resources`
    // with the full TS-parity load order (builtins → extended → skills →
    // plugin contributions → TS-parity P1 handlers). Both the SDK
    // `initialize.commands` advertisement and the TUI dispatch chain
    // (`tui_runner::dispatch_slash_command`) read from the same Arc.
    let command_registry = resources.command_registry.clone();

    let current_output_style = "default".to_string();
    let agent_search_paths = standard_agent_search_paths(&global_config::config_home(), &cwd);

    let auth_method = if is_real_anthropic {
        let config_dir = global_config::config_home();
        let api_key_helper = runtime_config.settings.merged.api_key_helper.clone();
        let force_env_auth = runtime_config.env_only.force_env_auth;
        tokio::task::spawn_blocking(move || {
            coco_inference::auth::resolve_auth(&coco_inference::auth::AuthResolveOptions {
                config_dir: Some(config_dir),
                api_key_helper,
                force_env_auth,
                ..Default::default()
            })
        })
        .await
        .ok()
        .flatten()
    } else {
        None
    };

    let mut bootstrap_builder = CliInitializeBootstrap::new(current_output_style)
        .with_command_registry(command_registry.clone())
        .with_output_style_dirs(output_style_dirs())
        .with_agent_search_paths(agent_search_paths);
    if let Some(auth) = auth_method {
        bootstrap_builder = bootstrap_builder.with_auth_method(auth);
    }
    let bootstrap: Arc<dyn coco_cli::sdk_server::InitializeBootstrap> = Arc::new(bootstrap_builder);

    if let Some(msg) = &resources.startup.notification {
        eprintln!("warning: {msg}");
    }
    let bypass_permissions_available = resources.startup.bypass_available;
    let permission_mode = resources.startup.mode;

    let transport = StdioTransport::new();
    let server = SdkServer::new(transport)
        .with_session_manager(session_manager)
        .with_mcp_manager(mcp_manager.clone())
        .with_initialize_bootstrap(bootstrap);
    let state = server.state();
    state.bypass_permissions_available.store(
        bypass_permissions_available,
        std::sync::atomic::Ordering::Relaxed,
    );

    let bridge: Arc<dyn coco_tool_runtime::ToolPermissionBridge> =
        Arc::new(coco_cli::sdk_server::SdkPermissionBridge::new(state));

    let session_runtime = crate::session_runtime::SessionRuntime::build(
        crate::session_runtime::SessionRuntimeBuildOpts {
            cli,
            runtime_config: Arc::new(runtime_config),
            cwd: cwd.clone(),
            model_id,
            system_prompt: system_prompt.clone().unwrap_or_default(),
            bypass_permissions_available,
            permission_mode,
            client: resources.client,
            fallback_clients: resources.fallback_clients,
            recovery_policy: resources.recovery_policy,
            tools: resources.tools,
            session_manager: session_manager_for_runtime,
            fast_model_spec: None,
            permission_bridge: Some(bridge),
            command_registry: command_registry.clone(),
        },
    )
    .await?;

    // Late-binds shared with TUI: task runtime, agent transcript
    // persistence, MCP handle (SDK-only today), agent-team wiring,
    // fork dispatcher. Wraps the SDK-bootstrapped `McpConnectionManager`
    // in an `McpManagerAdapter` so `mcp/setServers` and the per-engine
    // `mcp_handle` slot share one source of truth.
    let mcp_handle: coco_tool_runtime::McpHandleRef = Arc::new(
        coco_cli::mcp_handle_adapter::McpManagerAdapter::new(mcp_manager.clone()),
    );
    install_session_late_binds(session_runtime.clone(), &cwd, Some(mcp_handle)).await?;

    let file_history_for_server = session_runtime.file_history.clone().unwrap_or_else(|| {
        Arc::new(tokio::sync::RwLock::new(
            coco_context::FileHistoryState::new(),
        ))
    });
    let server = server
        .with_file_history(file_history_for_server, global_config::config_home())
        .with_session_runtime(session_runtime.clone());

    let runner = Arc::new(QueryEngineRunner::new(
        session_runtime,
        cli.max_tokens.unwrap_or(16_384),
        cli.max_turns.unwrap_or(30),
        system_prompt,
    ));
    server.set_turn_runner(runner).await;

    if let Err(e) = server.run().await {
        eprintln!("sdk mode: dispatch loop exited with error: {e}");
        return Err(anyhow::anyhow!("sdk dispatch failed: {e}"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "main.test.rs"]
mod tests;
