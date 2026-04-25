//! CLI entry point via clap.
//!
//! TS: entrypoints/ + main.tsx + cli/ + server/

pub mod output;
pub mod sdk;
pub mod sdk_server;

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
    ///
    /// Starts the session directly in `BypassPermissions` mode AND
    /// unlocks it as a reachable target for Shift+Tab / plan-mode exit.
    #[arg(long)]
    pub dangerously_skip_permissions: bool,

    /// Unlock `BypassPermissions` as an option without entering it at
    /// startup.
    ///
    /// TS parity: `--allow-dangerously-skip-permissions`. The user still
    /// starts in the default (or `--permission-mode`) mode, but can
    /// later cycle into bypass via Shift+Tab or plan-mode exit.
    #[arg(long)]
    pub allow_dangerously_skip_permissions: bool,

    /// Print response and exit (non-interactive mode).
    #[arg(long, alias = "print")]
    pub non_interactive: bool,

    /// Automatic fallback model(s) on overload. Repeatable — each
    /// occurrence appends one more tier to the Main role's fallback
    /// chain. Accepted form: `provider/model_id`. The chain is
    /// walked in flag order on capacity-error streaks.
    ///
    /// Legacy single-flag usage (`--fallback-model anthropic/sonnet`)
    /// continues to work and produces a 1-tier chain.
    #[arg(long, value_name = "PROVIDER/MODEL_ID")]
    pub fallback_model: Vec<String>,

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

    // ── PR-E3: TS-parity SDK/scripting flags ──
    /// Structured input format for non-interactive mode.
    ///
    /// TS: `--input-format <text|stream-json>` — pairs with `--output-format`
    /// to drive scripted pipelines over stdio.
    #[arg(long)]
    pub input_format: Option<String>,

    /// Path to a JSON schema file that validates structured output.
    ///
    /// TS: `--json-schema <file>` — applied to the final response when
    /// `output_format == stream-json`.
    #[arg(long)]
    pub json_schema: Option<String>,

    /// Replay user messages on resume (includes them in the transcript replay).
    ///
    /// TS: `--replay-user-messages` — useful for fixture-driven tests.
    #[arg(long)]
    pub replay_user_messages: bool,

    /// Emit hook lifecycle events in the stream-json output.
    ///
    /// TS: `--include-hook-events` — gates `HookStarted/Progress/Response`
    /// in the wire stream.
    #[arg(long)]
    pub include_hook_events: bool,

    /// Emit partial (incomplete) assistant messages in the stream-json output.
    ///
    /// TS: `--include-partial-messages` — exposes in-flight streaming
    /// content for clients that render mid-turn.
    #[arg(long)]
    pub include_partial_messages: bool,

    /// Thinking mode: enabled, adaptive, or disabled.
    ///
    /// TS: `--thinking <mode>` — orthogonal to `--thinking-budget` (which
    /// sets the token ceiling when enabled).
    #[arg(long)]
    pub thinking: Option<String>,

    /// Max tokens for extended thinking.
    ///
    /// TS: `--max-thinking-tokens <N>` — cap on reasoning tokens per turn.
    #[arg(long)]
    pub max_thinking_tokens: Option<i64>,

    /// File containing instructions to append to the system prompt.
    ///
    /// TS: `--append-system-prompt-file <path>` — reads the file and
    /// appends its contents to the default system prompt.
    #[arg(long)]
    pub append_system_prompt_file: Option<String>,

    /// Fail fast on invalid MCP config rather than best-effort loading.
    ///
    /// TS: `--strict-mcp-config` — if set, any malformed server entry
    /// aborts startup.
    #[arg(long)]
    pub strict_mcp_config: bool,

    /// Comma-separated list of setting sources to load (user, project, local).
    ///
    /// TS: `--setting-sources <csv>` — restrict which layers participate.
    #[arg(long)]
    pub setting_sources: Option<String>,

    /// Fork a new session from the provided session ID.
    ///
    /// TS: `--fork-session` — copies history from `--resume <id>` into a
    /// fresh session rather than continuing it.
    #[arg(long)]
    pub fork_session: bool,

    /// Comma-separated list of provider beta headers to opt into.
    ///
    /// TS: `--betas <csv>` — e.g. `prompt-caching-2024-07-31`.
    #[arg(long)]
    pub betas: Option<String>,

    /// Explicit session ID to use for this run.
    ///
    /// TS: `--session-id <uuid>` — for deterministic session IDs in
    /// automation. Distinct from `--resume` (continue existing) and
    /// `--fork-session` (copy existing).
    #[arg(long)]
    pub session_id: Option<String>,

    /// MCP tool name to delegate permission prompts to.
    ///
    /// TS: `--permission-prompt-tool <name>` — routes `Ask` decisions to
    /// the named tool instead of the built-in TUI / SDK bridge.
    #[arg(long)]
    pub permission_prompt_tool: Option<String>,

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
    /// List discovered agent definitions.
    ///
    /// TS: `src/cli/handlers/agents.ts` — walks `~/.coco/agents/` and
    /// `.claude/agents/` for markdown frontmatter agent specs.
    Agents,
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

    /// Run in SDK mode — NDJSON over stdio with the JSON-RPC control
    /// protocol. Intended to be spawned as a subprocess by the
    /// Python/TypeScript SDK client.
    ///
    /// TS: `src/cli/structuredIO.ts` — the `StructuredIO` loop.
    Sdk,
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
    /// Install a plugin from a local path (copies into user plugin dir).
    /// URL-based install (marketplace/git) is not yet implemented.
    Install {
        /// Local directory containing `PLUGIN.toml`, or plugin URL.
        name: String,
    },
    /// Uninstall a plugin by name.
    Uninstall {
        /// Plugin name.
        name: String,
    },
    /// Validate a plugin manifest at the given path.
    ///
    /// TS: `pluginValidateHandler` — checks PLUGIN.toml structure.
    Validate {
        /// Path to plugin directory (must contain `PLUGIN.toml`).
        path: String,
    },
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
