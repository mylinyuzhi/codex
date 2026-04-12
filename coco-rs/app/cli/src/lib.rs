//! CLI entry point via clap.
//!
//! TS: entrypoints/ + main.tsx + cli/ + server/

pub mod output;
pub mod sdk;
pub mod transport;

use clap::Parser;
use clap::Subcommand;

/// The coco CLI.
#[derive(Parser)]
#[command(name = "coco", about = "AI coding agent", version)]
pub struct Cli {
    /// Prompt to send (non-interactive mode).
    #[arg(short, long)]
    pub prompt: Option<String>,

    /// Model to use.
    #[arg(short, long)]
    pub model: Option<String>,

    /// Run without TUI (REPL mode).
    #[arg(long)]
    pub no_tui: bool,

    /// Output as NDJSON (SDK mode).
    #[arg(long)]
    pub json: bool,

    /// Settings file override.
    #[arg(long)]
    pub settings: Option<String>,

    /// Maximum tokens.
    #[arg(long)]
    pub max_tokens: Option<i64>,

    /// Maximum turns.
    #[arg(long)]
    pub max_turns: Option<i32>,

    /// Permission mode.
    #[arg(long)]
    pub permission_mode: Option<String>,

    /// Working directory override.
    #[arg(long, short = 'C')]
    pub cwd: Option<String>,

    /// Debug mode.
    #[arg(long)]
    pub debug: bool,

    /// Verbose mode.
    #[arg(long, short)]
    pub verbose: bool,

    /// Run the conversation in the background.
    #[arg(long, alias = "background")]
    pub bg: bool,

    /// Resume a specific session by ID (shorthand for `resume <id>`).
    #[arg(long)]
    pub resume: Option<String>,

    /// Thinking budget for extended thinking mode.
    #[arg(long)]
    pub thinking_budget: Option<i64>,

    /// System prompt override (appended to default).
    #[arg(long)]
    pub system_prompt: Option<String>,

    /// Append instructions from a file to the system prompt.
    #[arg(long)]
    pub append_system_prompt: Option<String>,

    /// MCP config JSON (inline server definitions).
    #[arg(long)]
    pub mcp_config: Option<String>,

    /// Continue the most recent conversation.
    #[arg(long, short = 'c', alias = "continue")]
    pub continue_session: bool,

    /// Output format: text, json, stream-json.
    #[arg(long, default_value = "text")]
    pub output_format: String,

    /// Reasoning effort level: low, medium, high, max.
    #[arg(long)]
    pub effort: Option<String>,

    /// Allow specific tools (repeatable).
    #[arg(long, num_args = 1..)]
    pub allowed_tools: Vec<String>,

    /// Deny specific tools (repeatable).
    #[arg(long, num_args = 1..)]
    pub disallowed_tools: Vec<String>,

    /// Additional directories to allow access to (repeatable).
    #[arg(long, num_args = 1..)]
    pub add_dir: Vec<String>,

    /// Create a git worktree for isolated work.
    #[arg(long, short = 'w')]
    pub worktree: Option<Option<String>>,

    /// Set display name for the session.
    #[arg(long, short = 'n')]
    pub name: Option<String>,

    /// Bypass all permission checks (dangerous).
    #[arg(long)]
    pub dangerously_skip_permissions: bool,

    /// Print response and exit (non-interactive mode).
    #[arg(long, alias = "print")]
    pub non_interactive: bool,

    /// Automatic fallback model on overload.
    #[arg(long)]
    pub fallback_model: Option<String>,

    /// Custom agent for the session.
    #[arg(long)]
    pub agent: Option<String>,

    /// Maximum spending limit in USD.
    #[arg(long)]
    pub max_budget_usd: Option<f64>,

    /// Run setup hooks and exit.
    #[arg(long)]
    pub init_only: bool,

    /// Disable session persistence.
    #[arg(long)]
    pub no_session_persistence: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// CLI subcommands.
///
/// TS: Commands enum + handlers/
#[derive(Subcommand)]
pub enum Commands {
    /// Start a new conversation.
    Chat {
        /// Initial prompt.
        prompt: Option<String>,
    },
    /// Manage configuration.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Resume a previous session.
    Resume {
        /// Session ID or title.
        session_id: Option<String>,
    },
    /// List sessions.
    Sessions,
    /// Show status.
    Status,
    /// Run diagnostics.
    Doctor,
    /// Authenticate with Anthropic.
    Login,
    /// Clear credentials.
    Logout,
    /// Initialize project (.claude/ directory).
    Init,
    /// Review code changes or a PR.
    Review {
        /// PR number or file to review.
        target: Option<String>,
    },
    /// Manage MCP server connections.
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },
    /// Manage plugins.
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
    },
    /// Show auto-mode defaults.
    #[command(name = "auto-mode")]
    AutoMode {
        /// Subcommand: "defaults" to show default rules.
        subcmd: Option<String>,
    },

    // ── TS-parity subcommands ──
    /// Run a long-running background supervisor (daemon mode).
    ///
    /// TS: `daemon` subcommand (DAEMON feature).
    Daemon,

    /// List running background sessions.
    ///
    /// TS: `ps` subcommand (BG_SESSIONS feature).
    Ps,

    /// Show logs from a background session.
    ///
    /// TS: `logs` subcommand (BG_SESSIONS feature).
    Logs {
        /// Session ID.
        session_id: String,
    },

    /// Attach to a running background session.
    ///
    /// TS: `attach` subcommand (BG_SESSIONS feature).
    Attach {
        /// Session ID.
        session_id: String,
    },

    /// Kill a running background session.
    ///
    /// TS: `kill` subcommand (BG_SESSIONS feature).
    Kill {
        /// Session ID.
        session_id: String,
    },

    /// Start remote control / bridge mode.
    ///
    /// TS: `remote-control`/`rc`/`bridge` subcommand (BRIDGE_MODE feature).
    #[command(alias = "rc", alias = "bridge")]
    RemoteControl,

    /// Sync with a remote session.
    ///
    /// TS: `sync` subcommand (BRIDGE_MODE feature).
    Sync,

    /// Show release notes for the current version.
    #[command(name = "release-notes")]
    ReleaseNotes,

    /// Upgrade to the latest version.
    Upgrade,

    /// Show cost and usage information.
    Usage,
}

/// Config subcommand actions.
#[derive(Subcommand)]
pub enum ConfigAction {
    /// Get a configuration value.
    Get {
        /// Configuration key.
        key: String,
    },
    /// Set a configuration value.
    Set {
        /// Configuration key.
        key: String,
        /// New value.
        value: String,
    },
    /// List all configuration values.
    List,
    /// Reset to defaults.
    Reset,
}

/// MCP subcommand actions.
#[derive(Subcommand)]
pub enum McpAction {
    /// List connected servers.
    List,
    /// Add a server.
    Add {
        /// Server name.
        name: String,
        /// Configuration JSON.
        config: Option<String>,
    },
    /// Remove a server.
    Remove {
        /// Server name.
        name: String,
    },
}

/// Plugin subcommand actions.
#[derive(Subcommand)]
pub enum PluginAction {
    /// List installed plugins.
    List,
    /// Install a plugin.
    Install {
        /// Plugin name or URL.
        name: String,
    },
    /// Uninstall a plugin.
    Uninstall {
        /// Plugin name.
        name: String,
    },
}
