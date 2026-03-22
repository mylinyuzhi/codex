//! cocode - Multi-provider LLM CLI
//!
//! A command-line interface for interacting with multiple LLM providers.
//!
//! This binary uses the arg0 dispatcher for single-binary deployment,
//! supporting apply_patch and sandbox invocation via PATH hijacking.

mod commands;
mod otel_init;
mod output;
mod repl;
mod tui_runner;

use std::path::PathBuf;

use clap::Parser;
use clap::Subcommand;
use cocode_config::ConfigManager;

/// Multi-provider LLM CLI
#[derive(Parser)]
#[command(name = "cocode", version, about = "Multi-provider LLM CLI")]
struct Cli {
    /// Subcommand to execute
    #[command(subcommand)]
    command: Option<Commands>,

    /// Configuration profile to use
    #[arg(short, long, global = true)]
    profile: Option<String>,

    /// Prompt to execute (non-interactive mode)
    prompt: Option<String>,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Disable TUI mode (use simple REPL instead)
    #[arg(long, global = true)]
    no_tui: bool,

    /// Append additional text to the end of the system prompt.
    #[arg(long, global = true)]
    system_prompt_suffix: Option<String>,

    /// JSON string defining custom agent types (highest priority).
    ///
    /// Format: `{"agent-name": {"description": "...", "prompt": "...", "tools": [...], "model": "sonnet"}}`
    #[arg(long, global = true)]
    agents: Option<String>,

    /// Load a plugin directory for the session (can be repeated).
    ///
    /// The plugin directory should contain a `plugin.json` manifest.
    /// Loaded as Flag scope (highest priority).
    #[arg(long = "plugin-dir", global = true)]
    plugin_dirs: Vec<PathBuf>,
}

/// Config subcommands
#[derive(Subcommand)]
pub enum ConfigAction {
    /// Show current configuration
    Show,
    /// List available providers and models
    List,
    /// Set a configuration value
    Set {
        /// Configuration key (model, provider)
        key: String,
        /// Value to set
        value: String,
    },
}

/// Plugin management subcommands
#[derive(Subcommand)]
pub enum PluginAction {
    /// Install a plugin from a marketplace
    Install {
        /// Plugin name (or name@marketplace)
        plugin_id: String,

        /// Installation scope
        #[arg(long, default_value = "user")]
        scope: String,
    },

    /// Uninstall a plugin
    Uninstall {
        /// Plugin name
        plugin_id: String,

        /// Scope to uninstall from
        #[arg(long, default_value = "user")]
        scope: String,
    },

    /// Enable a disabled plugin
    Enable {
        /// Plugin name
        plugin_id: String,
    },

    /// Disable an installed plugin
    Disable {
        /// Plugin name
        plugin_id: String,
    },

    /// Update an installed plugin to the latest version
    Update {
        /// Plugin name (or "all" to update all plugins)
        plugin_id: String,

        /// Scope of the plugin to update
        #[arg(long, default_value = "user")]
        scope: String,
    },

    /// List installed plugins
    List,

    /// Validate a plugin directory structure
    Validate {
        /// Path to the plugin directory (default: current directory)
        #[arg(default_value = ".")]
        path: PathBuf,
    },
}

/// Available subcommands
#[derive(Subcommand)]
enum Commands {
    /// Start an interactive chat session
    Chat {
        /// Session title
        #[arg(short, long)]
        title: Option<String>,

        /// Maximum turns before stopping
        #[arg(long)]
        max_turns: Option<i32>,
    },

    /// Configure providers and settings
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Resume a previous session
    Resume {
        /// Session ID to resume
        session_id: String,
    },

    /// List sessions
    Sessions {
        /// Show all sessions (including completed)
        #[arg(short, long)]
        all: bool,
    },

    /// Show current model and provider
    Status,

    /// Manage plugins (install, uninstall, enable, disable, update, list)
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
    },
}

fn main() -> anyhow::Result<()> {
    // Use arg0 dispatcher for single-binary deployment.
    // This handles:
    // - argv[0] dispatch: apply_patch, cocode-linux-sandbox
    // - argv[1] hijack: --cocode-run-as-apply-patch
    // - PATH setup with symlinks for subprocess integration
    // - dotenv loading from ~/.cocode/.env
    cocode_arg0::arg0_dispatch_or_else(cli_main)
}

/// Main CLI entry point (runs inside Tokio runtime created by arg0).
///
/// Note: Logging is NOT initialized here. Instead:
/// - TUI mode: Initializes file logging in tui_runner.rs
/// - REPL mode: Initializes stderr logging in commands/chat.rs
async fn cli_main(_arg0_paths: cocode_arg0::Arg0DispatchPaths) -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Load configuration first
    let config = ConfigManager::from_default()?;

    // Apply profile if specified
    if let Some(profile) = &cli.profile {
        match config.set_profile(profile) {
            Ok(true) => {
                // Profile applied successfully - will log after tracing is initialized
            }
            Ok(false) => {
                eprintln!("Warning: Profile '{profile}' not found in config, using defaults");
            }
            Err(e) => {
                eprintln!("Error setting profile: {e}");
            }
        }
    }

    // Dispatch to appropriate command
    match cli.command {
        Some(Commands::Chat { title, max_turns }) => {
            run_interactive(
                None, // No initial prompt for chat mode
                title,
                max_turns,
                &config,
                cli.no_tui,
                cli.verbose,
                cli.system_prompt_suffix,
                cli.agents,
                cli.plugin_dirs,
            )
            .await
        }
        Some(Commands::Config { action }) => commands::config::run(action, &config).await,
        Some(Commands::Resume { session_id }) => commands::resume::run(&session_id, &config).await,
        Some(Commands::Sessions { all }) => commands::sessions::run(all, &config).await,
        Some(Commands::Status) => commands::status::run(&config).await,
        Some(Commands::Plugin { action }) => commands::plugin::run(action, &config).await,
        None => {
            // No subcommand - either run prompt or start interactive chat
            if let Some(prompt) = cli.prompt {
                // Non-interactive mode: run single prompt (always uses REPL mode)
                run_interactive(
                    Some(prompt),
                    None,
                    Some(1), // Single turn for prompt mode
                    &config,
                    true, // Force no-tui for single prompt
                    cli.verbose,
                    cli.system_prompt_suffix,
                    cli.agents,
                    cli.plugin_dirs,
                )
                .await
            } else {
                // Interactive mode: start chat (use TUI by default)
                run_interactive(
                    None,
                    None,
                    None,
                    &config,
                    cli.no_tui,
                    cli.verbose,
                    cli.system_prompt_suffix,
                    cli.agents,
                    cli.plugin_dirs,
                )
                .await
            }
        }
    }
}

/// Run interactive mode (TUI or REPL).
#[allow(clippy::too_many_arguments)]
async fn run_interactive(
    initial_prompt: Option<String>,
    title: Option<String>,
    max_turns: Option<i32>,
    config: &ConfigManager,
    no_tui: bool,
    verbose: bool,
    system_prompt_suffix: Option<String>,
    agents_json: Option<String>,
    plugin_dirs: Vec<PathBuf>,
) -> anyhow::Result<()> {
    // Parse --agents JSON into agent definitions if provided
    let cli_agents = if let Some(ref json_str) = agents_json {
        match parse_cli_agents(json_str) {
            Ok(agents) => {
                tracing::info!(count = agents.len(), "Parsed CLI agent definitions");
                agents
            }
            Err(e) => {
                eprintln!("Warning: Failed to parse --agents JSON: {e}");
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };

    // Store CLI agents for session layer to pick up via env var
    if !cli_agents.is_empty() {
        // SAFETY: set_var is called before any threads are spawned (single-threaded init).
        unsafe {
            std::env::set_var(
                "COCODE_CLI_AGENTS",
                serde_json::to_string(&cli_agents).unwrap_or_default(),
            );
        }
    }

    // Store plugin dirs for session layer to pick up via env var
    if !plugin_dirs.is_empty() {
        // SAFETY: set_var is called before any threads are spawned (single-threaded init).
        unsafe {
            std::env::set_var(
                "COCODE_PLUGIN_DIRS",
                serde_json::to_string(&plugin_dirs).unwrap_or_default(),
            );
        }
    }

    // For single prompt or explicit --no-tui, use REPL mode
    if initial_prompt.is_some() || no_tui {
        return commands::chat::run(
            initial_prompt,
            title,
            max_turns,
            config,
            verbose,
            system_prompt_suffix,
            cli_agents,
        )
        .await;
    }

    // Interactive mode: use TUI
    tui_runner::run_tui(title, config, verbose, system_prompt_suffix, cli_agents).await
}

/// Parse `--agents` JSON into agent definitions.
///
/// Expected format: `{"name": {"description": "...", "tools": [...], "model": "sonnet"}}`
fn parse_cli_agents(json_str: &str) -> anyhow::Result<Vec<cocode_subagent::AgentDefinition>> {
    use cocode_subagent::AgentDefinition;
    use cocode_subagent::AgentSource;

    let map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(json_str)?;
    let mut agents = Vec::new();

    for (name, value) in map {
        let description = value["description"].as_str().unwrap_or("").to_string();
        let tools: Vec<String> = value["tools"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let disallowed_tools: Vec<String> = value["disallowedTools"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let max_turns = value["maxTurns"].as_i64().map(|n| n as i32);
        let prompt = value["prompt"].as_str().map(String::from);

        agents.push(AgentDefinition {
            name: name.clone(),
            description,
            agent_type: name,
            tools,
            disallowed_tools,
            identity: None,
            max_turns,
            permission_mode: None,
            fork_context: false,
            color: None,
            critical_reminder: prompt,
            source: AgentSource::CliFlag,
            skills: vec![],
            background: false,
            memory: None,
            hooks: None,
            mcp_servers: None,
            isolation: None,
            use_custom_prompt: false,
        });
    }

    Ok(agents)
}
