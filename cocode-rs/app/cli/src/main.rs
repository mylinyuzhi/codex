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
mod sdk;
mod tui_runner;

use std::path::PathBuf;

use clap::Parser;
use clap::Subcommand;
use cocode_config::ConfigManager;
use cocode_config::ConfigOverrides;
use cocode_protocol::PermissionMode;
use cocode_protocol::ThinkingLevel;
use cocode_session::Session;

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

    /// Run in SDK mode (NDJSON over stdio, no TUI).
    ///
    /// In this mode the CLI reads JSON requests from stdin and streams
    /// JSON events to stdout. Stderr is used for logging only.
    #[arg(long, global = true)]
    sdk_mode: bool,

    // ── Critical flags ──
    /// Model override (e.g., "anthropic/claude-opus-4" or "openai/gpt-5").
    ///
    /// Must be in "provider/model" format.
    #[arg(short, long, global = true)]
    model: Option<String>,

    /// Thinking effort level (none, low, medium, high, xhigh).
    #[arg(long, global = true)]
    effort: Option<String>,

    // ── High-priority flags ──
    /// Resume a session by ID or name (inline alternative to the `resume` subcommand).
    #[arg(long, global = true)]
    resume: Option<String>,

    /// Continue the most recent session.
    #[arg(long, global = true, alias = "continue")]
    r#continue: bool,

    /// Comma-separated list of allowed tool names.
    #[arg(long, global = true, value_delimiter = ',')]
    allowed_tools: Vec<String>,

    /// Comma-separated list of disallowed tool names.
    #[arg(long, global = true, value_delimiter = ',')]
    disallowed_tools: Vec<String>,

    /// Permission mode (default, plan, acceptEdits, bypassPermissions).
    #[arg(long, global = true)]
    permission_mode: Option<String>,

    /// Bypass all permission checks (shorthand for --permission-mode bypassPermissions).
    #[arg(long, global = true)]
    dangerously_skip_permissions: bool,

    // ── Medium-priority flags ──
    /// Create a git worktree for the session.
    #[arg(long, global = true)]
    worktree: bool,

    /// Path to MCP server configuration file.
    #[arg(long, global = true)]
    mcp_config: Option<PathBuf>,

    /// Output format (text, json, streaming-json).
    #[arg(long, global = true)]
    output_format: Option<String>,

    /// Enable debug logging.
    #[arg(long, global = true)]
    debug: bool,

    /// Write debug logs to a file.
    #[arg(long, global = true)]
    debug_file: Option<PathBuf>,

    /// Run initialization hooks and exit.
    #[arg(long, global = true)]
    init: bool,

    /// Disable skill/slash commands for the session.
    #[arg(long, global = true)]
    disable_slash_commands: bool,

    /// Fork an existing session by ID or name.
    #[arg(long, global = true)]
    fork_session: Option<String>,

    /// Maximum turns before stopping (overrides per-subcommand values).
    #[arg(long, global = true)]
    max_turns: Option<i32>,

    /// Maximum budget in USD before pausing (e.g., 5.0 for $5.00).
    #[arg(long, global = true)]
    max_budget_usd: Option<f64>,
}

/// Flags parsed from the CLI that are threaded through to session creation.
///
/// Consolidates all non-subcommand CLI flags into a single struct to avoid
/// ever-growing parameter lists.
#[derive(Debug, Clone, Default)]
pub struct CliFlags {
    pub verbose: bool,
    pub system_prompt_suffix: Option<String>,
    pub cli_agents: Vec<cocode_subagent::AgentDefinition>,
    pub model: Option<String>,
    pub effort: Option<ThinkingLevel>,
    pub resume: Option<String>,
    pub r#continue: bool,
    pub allowed_tools: Vec<String>,
    pub disallowed_tools: Vec<String>,
    pub permission_mode: Option<PermissionMode>,
    pub worktree: bool,
    pub mcp_config: Option<PathBuf>,
    pub output_format: Option<String>,
    pub debug: bool,
    pub debug_file: Option<PathBuf>,
    pub init: bool,
    pub disable_slash_commands: bool,
    pub fork_session: Option<String>,
    pub max_turns: Option<i32>,
    pub max_budget_usd: Option<f64>,
}

impl CliFlags {
    /// Apply `--model` and `--effort` overrides to role selections.
    pub fn apply_model_overrides(
        &self,
        config: &ConfigManager,
        selections: &mut cocode_protocol::RoleSelections,
    ) -> anyhow::Result<()> {
        if let Some(ref model_str) = self.model {
            let spec: cocode_protocol::ModelSpec = model_str
                .parse()
                .map_err(|e| anyhow::anyhow!("Invalid --model value: {e}"))?;
            let selection = config
                .resolve_selection(&spec.provider, &spec.slug)
                .unwrap_or_else(|_| cocode_protocol::RoleSelection::new(spec));
            selections.set(cocode_protocol::model::ModelRole::Main, selection);
        }
        if let Some(ref effort) = self.effort
            && let Some(main_sel) = selections.get_mut(cocode_protocol::model::ModelRole::Main)
        {
            main_sel.thinking_level = Some(effort.clone());
        }
        Ok(())
    }
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

        /// Session name for listing and resume-by-name
        #[arg(short = 'n', long)]
        name: Option<String>,

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

/// Parse permission mode from CLI string.
///
/// Accepts multiple formats for user convenience:
/// - "default", "plan", "acceptEdits"/"accept-edits", "bypassPermissions"/"bypass"
fn parse_permission_mode(s: &str) -> anyhow::Result<PermissionMode> {
    match s.to_lowercase().replace('_', "-").as_str() {
        "default" => Ok(PermissionMode::Default),
        "plan" => Ok(PermissionMode::Plan),
        "acceptedits" | "accept-edits" => Ok(PermissionMode::AcceptEdits),
        "bypasspermissions" | "bypass-permissions" | "bypass" => Ok(PermissionMode::Bypass),
        "dontask" | "dont-ask" => Ok(PermissionMode::DontAsk),
        _ => Err(anyhow::anyhow!(
            "Unknown permission mode: '{s}'. Valid values: default, plan, acceptEdits, bypassPermissions, dontAsk"
        )),
    }
}

/// Build `CliFlags` from the parsed `Cli` struct.
fn build_cli_flags(cli: &mut Cli) -> anyhow::Result<CliFlags> {
    // Parse --agents JSON into agent definitions if provided
    let cli_agents = if let Some(ref json_str) = cli.agents {
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

    // Parse --effort into ThinkingLevel
    let effort = if let Some(ref effort_str) = cli.effort {
        Some(
            effort_str
                .parse::<ThinkingLevel>()
                .map_err(|e| anyhow::anyhow!("Invalid --effort value: {e}"))?,
        )
    } else {
        None
    };

    // Parse --permission-mode, with --dangerously-skip-permissions as override
    let permission_mode = if cli.dangerously_skip_permissions {
        Some(PermissionMode::Bypass)
    } else if let Some(ref mode_str) = cli.permission_mode {
        Some(parse_permission_mode(mode_str)?)
    } else {
        None
    };

    if let Some(turns) = cli.max_turns
        && turns <= 0
    {
        return Err(anyhow::anyhow!("--max-turns must be positive, got {turns}"));
    }
    if let Some(usd) = cli.max_budget_usd
        && usd <= 0.0
    {
        return Err(anyhow::anyhow!(
            "--max-budget-usd must be positive, got {usd}"
        ));
    }

    Ok(CliFlags {
        verbose: cli.verbose || cli.debug,
        system_prompt_suffix: cli.system_prompt_suffix.take(),
        cli_agents,
        model: cli.model.take(),
        effort,
        resume: cli.resume.take(),
        r#continue: cli.r#continue,
        allowed_tools: std::mem::take(&mut cli.allowed_tools),
        disallowed_tools: std::mem::take(&mut cli.disallowed_tools),
        permission_mode,
        worktree: cli.worktree,
        mcp_config: cli.mcp_config.take(),
        output_format: cli.output_format.take(),
        debug: cli.debug,
        debug_file: cli.debug_file.take(),
        init: cli.init,
        disable_slash_commands: cli.disable_slash_commands,
        fork_session: cli.fork_session.take(),
        max_turns: cli.max_turns,
        max_budget_usd: cli.max_budget_usd,
    })
}

/// Main CLI entry point (runs inside Tokio runtime created by arg0).
///
/// Note: Logging is NOT initialized here. Instead:
/// - TUI mode: Initializes file logging in tui_runner.rs
/// - REPL mode: Initializes stderr logging in commands/chat.rs
async fn cli_main(_arg0_paths: cocode_arg0::Arg0DispatchPaths) -> anyhow::Result<()> {
    let mut cli = Cli::parse();

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

    // SDK mode: NDJSON over stdio, no TUI
    if cli.sdk_mode {
        return sdk::run_sdk_mode(&config).await;
    }

    let no_tui = cli.no_tui;

    // Store plugin dirs for session layer to pick up via env var
    // (must happen before build_cli_flags since it's on the Cli struct, not CliFlags)
    if !cli.plugin_dirs.is_empty() {
        // SAFETY: set_var is called before any threads are spawned (single-threaded init).
        unsafe {
            std::env::set_var(
                "COCODE_PLUGIN_DIRS",
                serde_json::to_string(&cli.plugin_dirs).unwrap_or_default(),
            );
        }
    }

    let flags = build_cli_flags(&mut cli)?;

    // Handle --init: validate config, create ephemeral session, and exit.
    // This mirrors Claude Code's --init which validates the environment without
    // starting an interactive session — useful for CI/CD setup verification.
    if flags.init {
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let snapshot = std::sync::Arc::new(
            config.build_config(ConfigOverrides::default().with_cwd(cwd.clone()))?,
        );
        let selections = config.build_all_selections();
        let session = Session::with_selections(cwd, selections);
        // Creating SessionState validates config and initializes all subsystems
        let _state = cocode_session::SessionState::new(session, snapshot).await?;
        eprintln!("Initialization complete.");
        return Ok(());
    }

    // Handle --resume, --continue, --fork-session as top-level flags (override subcommands)
    if flags.r#continue {
        return commands::resume::run_most_recent(&config).await;
    }
    if let Some(ref session_id) = flags.resume {
        return commands::resume::run(session_id, &config).await;
    }
    if let Some(ref session_id) = flags.fork_session {
        return commands::resume::run_fork(session_id, &config).await;
    }

    // Dispatch to appropriate command
    match cli.command {
        Some(Commands::Chat {
            title,
            name,
            max_turns,
        }) => {
            // Global --max-turns takes precedence over subcommand --max-turns
            let effective_max_turns = flags.max_turns.or(max_turns);
            run_interactive(
                None, // No initial prompt for chat mode
                title,
                name,
                effective_max_turns,
                &config,
                no_tui,
                flags,
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
                // Global --max-turns overrides the default single-turn limit
                let effective_max_turns = flags.max_turns.or(Some(1));
                run_interactive(
                    Some(prompt),
                    None,
                    None, // No session name for single prompt
                    effective_max_turns,
                    &config,
                    true, // Force no-tui for single prompt
                    flags,
                )
                .await
            } else {
                // Interactive mode: start chat (use TUI by default)
                run_interactive(
                    None,
                    None,
                    None, // No session name for default mode
                    flags.max_turns,
                    &config,
                    no_tui,
                    flags,
                )
                .await
            }
        }
    }
}

/// Run interactive mode (TUI or REPL).
async fn run_interactive(
    initial_prompt: Option<String>,
    title: Option<String>,
    name: Option<String>,
    max_turns: Option<i32>,
    config: &ConfigManager,
    no_tui: bool,
    flags: CliFlags,
) -> anyhow::Result<()> {
    // Set all CLI-flag env vars once, before forking into TUI or REPL.
    // This runs during single-threaded init, before any async tasks spawn.
    set_cli_env_vars(&flags);

    // For single prompt or explicit --no-tui, use REPL mode
    if initial_prompt.is_some() || no_tui {
        return commands::chat::run(initial_prompt, title, name, max_turns, config, flags).await;
    }

    // Interactive mode: use TUI
    tui_runner::run_tui(title, name, config, flags).await
}

/// Set environment variables from CLI flags for downstream consumption.
///
/// Must be called during single-threaded init (before any async tasks spawn)
/// so the `unsafe` `set_var` calls are sound. Both TUI and REPL paths read
/// these env vars from the session layer.
fn set_cli_env_vars(flags: &CliFlags) {
    if !flags.cli_agents.is_empty() {
        // SAFETY: called during single-threaded init before async tasks spawn.
        unsafe {
            std::env::set_var(
                "COCODE_CLI_AGENTS",
                serde_json::to_string(&flags.cli_agents).unwrap_or_default(),
            );
        }
    }
    if !flags.allowed_tools.is_empty() {
        unsafe { std::env::set_var("COCODE_ALLOWED_TOOLS", flags.allowed_tools.join(",")) };
    }
    if !flags.disallowed_tools.is_empty() {
        unsafe { std::env::set_var("COCODE_DISALLOWED_TOOLS", flags.disallowed_tools.join(",")) };
    }
    if let Some(ref format) = flags.output_format {
        unsafe { std::env::set_var("COCODE_OUTPUT_FORMAT", format) };
    }
    if flags.disable_slash_commands {
        unsafe { std::env::set_var("COCODE_DISABLE_SLASH_COMMANDS", "1") };
    }
    if flags.worktree {
        unsafe { std::env::set_var("COCODE_WORKTREE", "1") };
    }
    if let Some(ref path) = flags.mcp_config {
        unsafe { std::env::set_var("COCODE_MCP_CONFIG", path.display().to_string()) };
    }
    if let Some(ref path) = flags.debug_file {
        unsafe { std::env::set_var("COCODE_DEBUG_FILE", path.display().to_string()) };
    }
    if let Some(usd) = flags.max_budget_usd {
        unsafe { std::env::set_var("COCODE_MAX_BUDGET_USD", format!("{usd}")) };
    }
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
