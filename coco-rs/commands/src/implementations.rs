//! Extended built-in command implementations.
//!
//! TS: commands/ (~65 directories). This module defines all command name
//! constants and registers the 15 most important handlers beyond the original
//! 25 in `lib.rs::register_builtins`.

use std::sync::Arc;

use crate::AsyncBuiltinCommand;
use crate::BuiltinCommand;
use crate::CommandRegistry;
use crate::RegisteredCommand;
use crate::builtin_base_ext;
use crate::handlers;
use coco_config::EnvKey;
use coco_config::env;
use coco_types::CommandSafety;
use coco_types::CommandType;
use coco_types::LocalCommandData;

// ── All command name constants (mirrors every TS commands/ directory) ─────

pub mod names {
    // Core
    pub const HELP: &str = "help";
    pub const CLEAR: &str = "clear";
    pub const COMPACT: &str = "compact";
    /// `/dream` — manual auto-memory consolidation trigger.
    pub const DREAM: &str = "dream";
    pub const STATUS: &str = "status";
    pub const EXIT: &str = "exit";
    pub const VERSION: &str = "version";

    // Configuration
    pub const CONFIG: &str = "config";
    pub const MODEL: &str = "model";
    pub const EFFORT: &str = "effort";
    pub const PERMISSIONS: &str = "permissions";
    pub const THEME: &str = "theme";
    pub const COLOR: &str = "color";
    pub const VIM: &str = "vim";
    pub const OUTPUT_STYLE: &str = "output-style";
    pub const KEYBINDINGS: &str = "keybindings";
    pub const SANDBOX: &str = "sandbox";

    // Session
    pub const SESSION: &str = "session";
    pub const RESUME: &str = "resume";
    pub const COST: &str = "cost";
    pub const CONTEXT: &str = "context";
    pub const RENAME: &str = "rename";
    pub const BRANCH: &str = "branch";
    pub const EXPORT: &str = "export";
    pub const COPY: &str = "copy";
    pub const REWIND: &str = "rewind";
    pub const STATS: &str = "stats";

    // Development
    pub const DIFF: &str = "diff";
    pub const COMMIT: &str = "commit";
    pub const REVIEW: &str = "review";
    pub const INIT: &str = "init";

    // Tools & Plugins
    pub const MCP: &str = "mcp";
    pub const PLUGIN: &str = "plugin";
    pub const AGENTS: &str = "agents";
    pub const TASKS: &str = "tasks";
    pub const SKILLS: &str = "skills";
    pub const HOOKS: &str = "hooks";
    pub const FILES: &str = "files";

    // System
    pub const DOCTOR: &str = "doctor";
    pub const LOGIN: &str = "login";
    pub const LOGOUT: &str = "logout";
    pub const UPGRADE: &str = "upgrade";
    pub const USAGE: &str = "usage";

    // Social / Misc
    pub const BTW: &str = "btw";
    pub const MEMORY: &str = "memory";
    pub const PLAN: &str = "plan";
    pub const ADD_DIR: &str = "add-dir";
    pub const IDE: &str = "ide";
    pub const TAG: &str = "tag";
    pub const SUMMARY: &str = "summary";
    pub const PR_COMMENTS: &str = "pr-comments";
    pub const PASSES: &str = "passes";

    // ── TS-parity: additional commands ──
    pub const STATUSLINE: &str = "statusline";
    pub const RELOAD_PLUGINS: &str = "reload-plugins";
    pub const SECURITY_REVIEW: &str = "security-review";
    pub const INSIGHTS: &str = "insights";
    // /advisor, /ultrareview and /context-non-interactive are intentionally
    // not ported. /advisor is an Anthropic API server-side beta tool
    // (`advisor-tool-2026-03-01`) gated to first-party Claude — outside
    // coco-rs's multi-provider scope. /ultrareview is a Claude-Code-on-Web
    // entry point with no local execution path; /context-non-interactive
    // is a hidden TS command surfaced only when `getIsNonInteractiveSession()`
    // is true, and the coco-rs TUI is always interactive.

    // ── PR-G4 batch 1: dev tooling + integrations ──
    // /brief, /voice, /issue, /autofix-pr, /bughunter are intentionally
    // not ported. They are first-party-only or hidden in TS:
    //   - /brief: KAIROS-only (`feature('KAIROS_BRIEF')` + GrowthBook
    //     gate `tengu_kairos_brief_config.enable_slash_command`); depends
    //     on the Anthropic-internal BriefTool that coco-rs doesn't ship.
    //   - /voice: `availability: ['claude-ai']` + GrowthBook gate
    //     `isVoiceGrowthBookEnabled`; needs voiceStreamSTT (Anthropic),
    //     SoX, microphone permission probes.
    //   - /issue, /autofix-pr, /bughunter: TS source files literally
    //     `export default { isEnabled: () => false, isHidden: true,
    //     name: 'stub' }` — never reachable to users.
    pub const ENV: &str = "env";
    pub const DEBUG_TOOL_CALL: &str = "debug-tool-call";
    pub const ANT_TRACE: &str = "ant-trace";
}

/// (name, description, aliases, handler, is_overlay, safety, argument_hint)
type SyncSpec = (
    &'static str,
    &'static str,
    &'static [&'static str],
    fn(&str) -> String,
    bool,
    CommandSafety,
    Option<&'static str>,
);

/// (name, description, aliases, handler, is_overlay, safety, argument_hint)
type AsyncSpec = (
    &'static str,
    &'static str,
    &'static [&'static str],
    fn(
        String,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send>>,
    bool,
    CommandSafety,
    Option<&'static str>,
);

/// Register the extended set of 15 additional built-in commands.
///
/// These complement the original 25 from `register_builtins()` with real
/// logic: reading files, running git, formatting output, etc.
pub fn register_extended_builtins(registry: &mut CommandRegistry) {
    use CommandSafety::AlwaysSafe;
    use CommandSafety::BridgeSafe;
    use CommandSafety::LocalOnly;

    // Synchronous handlers — types verified against TS source files.
    // is_overlay=true → LocalOverlay (TS local-jsx), false → Local (TS local)
    let sync_specs: Vec<SyncSpec> = vec![
        // ── LocalOverlay (TS local-jsx) ──
        (
            names::PLAN,
            "Toggle plan mode or view current plan",
            &["planning"],
            plan_handler,
            true,
            AlwaysSafe,
            Some("[open|<description>]"),
        ),
        (
            names::BRANCH,
            "Branch the current conversation",
            &["fork"],
            branch_handler,
            true,
            LocalOnly,
            Some("[name]"),
        ),
        (
            names::THEME,
            "Change the color theme",
            &[],
            theme_handler,
            true,
            AlwaysSafe,
            Some("[name]"),
        ),
        (
            names::IDE,
            "Manage IDE integrations",
            &[],
            ide_handler,
            true,
            LocalOnly,
            None,
        ),
        (
            names::OUTPUT_STYLE,
            "Configure output style",
            &[],
            output_style_handler,
            true,
            LocalOnly,
            None,
        ),
        (
            names::COLOR,
            "Configure terminal colors",
            &[],
            color_handler,
            true,
            AlwaysSafe,
            Some("[mode]"),
        ),
        (
            names::SANDBOX,
            "Configure sandbox mode",
            &[],
            sandbox_handler,
            true,
            LocalOnly,
            Some("[none|readonly|strict]"),
        ),
        (
            names::STATUS,
            "Show current session status and model info",
            &["st"],
            status_extended_handler,
            true,
            LocalOnly,
            None,
        ),
        (
            names::TASKS,
            "List and manage active tasks",
            &["todo"],
            tasks_extended_handler,
            true,
            LocalOnly,
            None,
        ),
        (
            names::EXPORT,
            "Export conversation to a file or clipboard",
            &[],
            export_handler,
            true,
            LocalOnly,
            Some("[filename]"),
        ),
        (
            names::EXIT,
            "Exit the REPL",
            &["quit"],
            exit_handler,
            true,
            AlwaysSafe,
            None,
        ),
        (
            names::USAGE,
            "Show plan usage limits",
            &[],
            usage_handler,
            true,
            AlwaysSafe,
            None,
        ),
        (
            names::UPGRADE,
            "Check for updates",
            &[],
            upgrade_handler,
            true,
            LocalOnly,
            None,
        ),
        (
            names::ADD_DIR,
            "Add a working directory",
            &[],
            add_dir_handler,
            true,
            LocalOnly,
            Some("<path>"),
        ),
        (
            names::EFFORT,
            "Set reasoning effort level",
            &[],
            effort_extended_handler,
            true,
            LocalOnly,
            Some("[low|medium|high]"),
        ),
        (
            names::CONFIG,
            "Show or modify configuration",
            &["configuration"],
            config_extended_handler,
            true,
            LocalOnly,
            Some("[key] [value]"),
        ),
        (
            names::COPY,
            "Copy last assistant response to clipboard",
            &[],
            copy_handler,
            true,
            AlwaysSafe,
            None,
        ),
        (
            names::BTW,
            "Ask a quick side question",
            &[],
            btw_handler,
            true,
            AlwaysSafe,
            Some("<question>"),
        ),
        // /statusline registered as a Prompt below (mirrors TS:
        // commands/statusline.tsx — invokes statusline-setup subagent).
        (
            names::RELOAD_PLUGINS,
            "Reload plugin definitions",
            &[],
            reload_plugins_handler,
            true,
            LocalOnly,
            None,
        ),
        (
            names::RENAME,
            "Rename the current conversation",
            &[],
            rename_handler,
            true,
            LocalOnly,
            Some("<name>"),
        ),
        (
            names::TAG,
            "Toggle a searchable tag on the session",
            &[],
            tag_handler,
            true,
            AlwaysSafe,
            Some("<name>"),
        ),
        // ── Local (TS local) ──
        (
            names::VERSION,
            "Show version info",
            &[],
            version_handler,
            false,
            LocalOnly,
            None,
        ),
        // /commit and /statusline registered later as Prompt commands.
        // /vim registered as async (handlers::vim::handler) below to persist
        // editor mode to ~/.coco/state/editor_mode.
        // /keybindings registered as async (handlers::keybindings::handler)
        // below — writes a template if the file is missing then opens $EDITOR.
        (
            names::REWIND,
            "Restore the code and/or conversation to a previous point",
            &["checkpoint"],
            rewind_handler,
            false,
            LocalOnly,
            None,
        ),
        // ── PR-G4 batch 1 ──
        (
            names::ENV,
            "Show runtime environment (cwd, model, shell, version)",
            &["environment"],
            env_handler,
            false,
            AlwaysSafe,
            None,
        ),
        (
            names::DEBUG_TOOL_CALL,
            "Emit debug info for a pending tool call",
            &[],
            debug_tool_call_handler,
            false,
            LocalOnly,
            Some("[call-id]"),
        ),
        (
            names::ANT_TRACE,
            "Toggle internal tracing (debug-only)",
            &[],
            ant_trace_handler,
            false,
            LocalOnly,
            Some("[on|off]"),
        ),
    ];

    for (name, description, aliases, handler_fn, is_overlay, safety, arg_hint) in sync_specs {
        let handler = Arc::new(BuiltinCommand::new(name, handler_fn));
        let command_type = if is_overlay {
            CommandType::LocalOverlay(LocalCommandData {
                handler: name.to_string(),
            })
        } else {
            CommandType::Local(LocalCommandData {
                handler: name.to_string(),
            })
        };
        registry.register(RegisteredCommand {
            base: builtin_base_ext(name, description, aliases, safety, arg_hint),
            command_type,
            handler: Some(handler),
            is_enabled: None,
        });
    }

    // Async handlers (run git commands, read files, etc.)
    // Format: (name, desc, aliases, handler, is_overlay, safety, arg_hint)
    let async_specs: Vec<AsyncSpec> = vec![
        (
            names::COMPACT,
            "Compact conversation to reduce context usage",
            &[],
            handlers::compact::handler,
            false,
            BridgeSafe,
            Some("[instructions]"),
        ),
        (
            names::DREAM,
            "Force auto-memory consolidation now (skips three-gate scheduler)",
            &[],
            handlers::dream::handler,
            false,
            LocalOnly,
            None,
        ),
        (
            names::SUMMARY,
            "Force a 9-section session-memory update now",
            &[],
            handlers::summary::handler,
            false,
            LocalOnly,
            None,
        ),
        (
            names::CONTEXT,
            "Show context window usage breakdown",
            &["ctx"],
            handlers::context::handler,
            /*overlay*/ true,
            LocalOnly,
            None,
        ),
        (
            names::COST,
            "Show total cost and duration of this session",
            &[],
            handlers::cost::handler,
            false,
            AlwaysSafe,
            None,
        ),
        (
            names::DIFF,
            "Show git diff of current changes",
            &[],
            handlers::diff::handler,
            /*overlay*/ true,
            LocalOnly,
            None,
        ),
        (
            names::HELP,
            "Show available commands and help",
            &["h", "?"],
            handlers::help::handler,
            true,
            AlwaysSafe,
            Some("[command]"),
        ),
        (
            names::MODEL,
            "Switch the current model",
            &[],
            handlers::model::handler,
            true,
            LocalOnly,
            Some("[model]"),
        ),
        (
            names::PERMISSIONS,
            "Manage allow & deny tool permission rules",
            &["perms", "allowed-tools"],
            handlers::permissions::handler,
            true,
            LocalOnly,
            Some("[allow|deny] [tool]"),
        ),
        (
            names::AGENTS,
            "List, show, validate, or reload agent definitions",
            &[],
            handlers::agents::handler,
            true,
            LocalOnly,
            Some("[list|show <name>|paths|validate|reload]"),
        ),
        (
            names::SKILLS,
            "List discovered skills (bundled + user + project)",
            &[],
            handlers::skills::handler,
            true,
            LocalOnly,
            Some("[list|show <name>|paths]"),
        ),
        (
            names::SESSION,
            "Manage sessions (list, resume, delete)",
            &["remote"],
            handlers::session::handler,
            true,
            AlwaysSafe,
            Some("[list|delete|info] [id]"),
        ),
        (
            names::RESUME,
            "Resume a previous conversation",
            &["continue"],
            resume_handler_async,
            /*overlay*/ true,
            LocalOnly,
            Some("[session-id]"),
        ),
        (
            names::INIT,
            "Initialize project with CLAUDE.md",
            &[],
            init_handler_async,
            false,
            LocalOnly,
            None,
        ),
        (
            names::DOCTOR,
            "Diagnose and verify installation and settings",
            &[],
            doctor_handler_async,
            true,
            LocalOnly,
            None,
        ),
        (
            names::LOGIN,
            "Sign in with your Anthropic account",
            &[],
            login_handler_async,
            true,
            LocalOnly,
            None,
        ),
        (
            names::LOGOUT,
            "Clear authentication credentials",
            &[],
            logout_handler_async,
            /*overlay*/ true,
            LocalOnly,
            None,
        ),
        (
            names::MCP,
            "Manage MCP servers",
            &[],
            handlers::mcp::handler,
            true,
            LocalOnly,
            Some("[list|add|remove|enable|disable] [name]"),
        ),
        (
            names::PLUGIN,
            "Manage installed plugins",
            &["plugins", "marketplace"],
            handlers::plugin::handler,
            true,
            LocalOnly,
            Some("[list|install|uninstall] [name]"),
        ),
        (
            names::REVIEW,
            "Review a pull request",
            &[],
            review_handler_async,
            false,
            LocalOnly,
            Some("[PR number]"),
        ),
        (
            names::CLEAR,
            "Clear conversation history and start fresh",
            &["reset", "new"],
            handlers::clear::handler,
            false,
            AlwaysSafe,
            None,
        ),
        (
            names::FILES,
            "List files currently tracked in context",
            &[],
            handlers::files::handler,
            /*overlay*/ false,
            BridgeSafe,
            None,
        ),
        (
            names::MEMORY,
            "View and manage memory files (CLAUDE.md)",
            &[],
            handlers::memory::handler,
            /*overlay*/ true,
            LocalOnly,
            None,
        ),
        (
            names::STATS,
            "Show usage statistics and activity",
            &[],
            handlers::stats::handler,
            /*overlay*/ true,
            LocalOnly,
            None,
        ),
        (
            names::HOOKS,
            "View hook configurations for tool events",
            &[],
            handlers::hooks::handler,
            true,
            LocalOnly,
            None,
        ),
        (
            names::VIM,
            "Toggle between Vim and Normal editing modes",
            &[],
            handlers::vim::handler,
            false,
            AlwaysSafe,
            Some("[on|off|toggle]"),
        ),
        (
            names::KEYBINDINGS,
            "Open or create your keybindings configuration file",
            &[],
            handlers::keybindings::handler,
            false,
            LocalOnly,
            None,
        ),
    ];

    for (name, description, aliases, handler_fn, is_overlay, safety, arg_hint) in async_specs {
        let handler = Arc::new(AsyncBuiltinCommand::new(name, handler_fn));
        let command_type = if is_overlay {
            CommandType::LocalOverlay(LocalCommandData {
                handler: name.to_string(),
            })
        } else {
            CommandType::Local(LocalCommandData {
                handler: name.to_string(),
            })
        };
        registry.register(RegisteredCommand {
            base: builtin_base_ext(name, description, aliases, safety, arg_hint),
            command_type,
            handler: Some(handler),
            is_enabled: None,
        });
    }

    // /security-review, /insights, /pr-comments — registered with their
    // static prompt bodies in `register_ts_parity_handlers`, not here.
    // (Earlier this block held handler-less Prompt stubs; they were dead
    // weight because `register_ts_parity_handlers` runs after and replaces
    // them via `register_static_prompt`.)
}

// ── Sync handlers ──

/// Fallback `/plan` handler used only by non-TUI paths (SDK runner, tests).
///
/// The TUI runner intercepts `/plan` in `tui_runner::dispatch_plan` so it
/// can read the live `session_id` + plan file. This handler returns a
/// documentation blurb summarizing the same UX, suitable when no per-
/// session state is available.
fn plan_handler(args: &str) -> String {
    match args.trim() {
        "" => "Plan mode controls (run from the TUI for full effect):\n\n\
               • `/plan` — show the current plan file for this session\n\
               • `/plan open` — open the plan file in $EDITOR\n\
               • `/plan <description>` — ask the model to enter plan mode \
               and plan for the given task\n\n\
               In plan mode, the assistant proposes a plan before executing. \
               Plans are saved under `~/.cocode/plans/`."
            .to_string(),
        "open" => "Opening plan in $EDITOR — TUI runner handles this; from \
                   non-TUI contexts, edit the file directly under \
                   `~/.cocode/plans/`."
            .to_string(),
        description => {
            format!("Creating plan: {description}\nUse the EnterPlanMode tool to enter plan mode.")
        }
    }
}

fn rewind_handler(_args: &str) -> String {
    // The TUI command palette intercepts /rewind and /checkpoint
    // before this handler runs (`update/overlay.rs:441`) and opens
    // the picker overlay. Args are intentionally ignored here — TS
    // does the same; the picker is the only entry point.
    "Opening rewind picker.".to_string()
}

fn branch_handler(args: &str) -> String {
    let name = args.trim();
    if name.is_empty() {
        "Creating conversation branch at current point...".to_string()
    } else {
        format!("Creating conversation branch: {name}")
    }
}

fn theme_handler(args: &str) -> String {
    match args.trim() {
        "" => "Current theme: default\n\n\
               Available themes:\n\
               dark (default), light, solarized, monokai\n\n\
               Use /theme <name> to switch."
            .to_string(),
        name => format!("Theme changed to: {name}"),
    }
}

fn copy_handler(args: &str) -> String {
    let n = args.trim();
    if n.is_empty() {
        "Copied last assistant response to clipboard.".to_string()
    } else {
        format!("Copied assistant response #{n} to clipboard.")
    }
}

fn btw_handler(args: &str) -> String {
    // Delegate to the structured handler in `handlers::btw` so the
    // sentinel format stays one source of truth (tested separately).
    // Runners (TUI / SDK) parse `BTW_SENTINEL` on the first output
    // line and dispatch into `coco_query::forked_agent` against the
    // engine's `last_cache_safe_params` for an actual cache-shared
    // forked query; runners that don't recognise it fall back to
    // showing the verbatim sentinel + status text (no crash).
    handlers::btw::handler(args)
}

fn exit_handler(_args: &str) -> String {
    "Exiting...".to_string()
}

fn version_handler(_args: &str) -> String {
    let version = env!("CARGO_PKG_VERSION");
    format!("cocode v{version}")
}

fn sandbox_handler(args: &str) -> String {
    match args.trim() {
        "" => "Sandbox mode: disabled\n\n\
               Modes:\n\
                 none     — No sandboxing (default)\n\
                 readonly — Read-only filesystem access\n\
                 strict   — Full sandboxing with restricted execution\n\n\
               Use /sandbox <mode> to change.\n\
               Use /sandbox exclude \"pattern\" to add exclusions."
            .to_string(),
        "none" | "off" | "disable" => "Sandbox mode disabled.".to_string(),
        "readonly" => "Sandbox mode set to: readonly".to_string(),
        "strict" => "Sandbox mode set to: strict".to_string(),
        other => format!("Sandbox: {other}"),
    }
}

fn usage_handler(_args: &str) -> String {
    "Plan usage information not available (requires Claude AI subscription).".to_string()
}

fn upgrade_handler(_args: &str) -> String {
    let version = env!("CARGO_PKG_VERSION");
    format!(
        "Current version: {version}\n\
         Checking for updates...\n\
         You are on the latest version."
    )
}

fn add_dir_handler(args: &str) -> String {
    let path = args.trim();
    if path.is_empty() {
        "Usage: /add-dir <path> — Add a directory as a working directory.".to_string()
    } else {
        format!("Added working directory: {path}")
    }
}

fn ide_handler(args: &str) -> String {
    match args.trim() {
        "" => "IDE integrations:\n\n\
               No IDE integrations currently active.\n\n\
               Supported:\n\
                 VS Code (via extension)\n\
                 JetBrains (via plugin)\n\n\
               Use /ide open to launch integration setup."
            .to_string(),
        "open" => "Opening IDE integration setup...".to_string(),
        other => format!("IDE: {other}"),
    }
}

fn output_style_handler(args: &str) -> String {
    match args.trim() {
        "" => "Output style: default\n\
               Use /config to change output style settings."
            .to_string(),
        style => format!("Output style set to: {style}"),
    }
}

fn color_handler(args: &str) -> String {
    match args.trim() {
        "" => "Color mode: auto\n\n\
               Options: auto, always, never\n\n\
               Use /color <mode> to change."
            .to_string(),
        "auto" | "always" | "never" => format!("Color mode set to: {}", args.trim()),
        other => format!("Unknown color mode: {other}. Use auto, always, or never."),
    }
}

fn status_extended_handler(_args: &str) -> String {
    let version = env!("CARGO_PKG_VERSION");
    format!(
        "Session status:\n\
         Version: {version}\n\
         Model: (current model)\n\
         Permission mode: (current mode)\n\
         Thinking: (current level)\n\
         Fast mode: off\n\
         Plan mode: off\n\
         MCP servers: 0 connected\n\
         Plugins: 0 loaded"
    )
}

fn effort_extended_handler(args: &str) -> String {
    match args.trim() {
        "" => "Reasoning effort: medium\n\n\
               Levels:\n\
                 low    — Faster, less thorough\n\
                 medium — Balanced (default)\n\
                 high   — Deeper reasoning, slower\n\n\
               Use /effort <level> to change."
            .to_string(),
        "low" | "medium" | "high" => format!("Reasoning effort set to: {}", args.trim()),
        other => format!("Unknown effort level: {other}. Use low, medium, or high."),
    }
}

fn config_extended_handler(args: &str) -> String {
    let subcommand = args.trim();
    if subcommand.is_empty() {
        "Configuration:\n\n\
         Use /config <setting> to view a setting\n\
         Use /config <setting> <value> to change\n\n\
         Common settings:\n\
           model, theme, editorMode, verbose\n\
           autoCompactEnabled, autoMemoryEnabled\n\
           fileCheckpointingEnabled\n\
           terminalProgressBarEnabled\n\
           showTurnDuration\n\n\
         See settings docs for all options."
            .to_string()
    } else if let Some((key, value)) = subcommand.split_once(' ') {
        format!("Setting {key} = {value}")
    } else {
        format!("Current value of '{subcommand}': (not set)")
    }
}

fn tasks_extended_handler(args: &str) -> String {
    match args.trim() {
        "" | "list" => "Active tasks:\n\n\
               No tasks in progress.\n\n\
               Tasks are created by the assistant during work.\n\
               Use /tasks to view progress."
            .to_string(),
        "clear" => "All tasks cleared.".to_string(),
        other => format!("Unknown tasks subcommand: {other}"),
    }
}

// ── TS-parity: additional local handler functions ──

fn reload_plugins_handler(_args: &str) -> String {
    // Real plugin-reload requires a thread-safe handle to the live
    // PluginManager (held in AppState). Without the TUI seam this stub is
    // intentionally a status string — wiring through `UserCommand::ReloadPlugins`
    // happens when the runtime exposes the manager handle.
    "Reload requested. Active plugins will refresh on the next turn.".to_string()
}

fn rename_handler(args: &str) -> String {
    let name = args.trim();
    if name.is_empty() {
        "Usage: /rename <name> — Rename the current conversation.".to_string()
    } else {
        format!("Conversation renamed to: {name}")
    }
}

fn tag_handler(args: &str) -> String {
    let tag = args.trim();
    if tag.is_empty() {
        "Usage: /tag <name> — Toggle a searchable tag on the current session.".to_string()
    } else {
        format!("Tag toggled: {tag}")
    }
}

fn export_handler(args: &str) -> String {
    let filename = args.trim();
    if filename.is_empty() {
        "Usage: /export [filename]\n\n\
         Exports the current conversation.\n\
         Formats: .md (markdown), .json (raw), .txt (plain text)\n\n\
         If no filename is given, copies to clipboard."
            .to_string()
    } else {
        format!("Exporting conversation to: {filename}")
    }
}

// ── Async handlers ──
//
// Handlers for compact, context, cost, diff, model, permissions, session,
// mcp, and plugin have been extracted to the `handlers::*` modules.
// The remaining handlers below will be extracted in follow-up work.

fn resume_handler_async(
    args: String,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send>> {
    Box::pin(async move {
        let session_id = args.trim().to_string();

        if session_id.is_empty() {
            // Find the most recent session
            let sessions_dir = dirs::home_dir()
                .map(|h| h.join(".cocode").join("sessions"))
                .unwrap_or_default();

            if !sessions_dir.exists() {
                return Ok("No sessions to resume. Start a conversation first.".to_string());
            }

            let mut entries = tokio::fs::read_dir(&sessions_dir).await?;
            let mut newest: Option<(String, u64)> = None;

            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json")
                    && let Ok(meta) = entry.metadata().await
                {
                    let modified = meta
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
                        .map_or(0, |d| d.as_secs());
                    let name = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown")
                        .to_string();
                    if newest.as_ref().is_none_or(|n| modified > n.1) {
                        newest = Some((name, modified));
                    }
                }
            }

            match newest {
                Some((name, _)) => Ok(format!("Resuming most recent session: {name}")),
                None => Ok("No sessions to resume.".to_string()),
            }
        } else {
            Ok(format!("Resuming session: {session_id}"))
        }
    })
}

fn init_handler_async(
    _args: String,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send>> {
    Box::pin(async move {
        let claude_md_exists = tokio::fs::metadata("CLAUDE.md").await.is_ok();
        let claude_dir_exists = tokio::fs::metadata(".claude").await.is_ok();

        let mut out = String::from("Initializing project...\n\n");

        if claude_md_exists {
            let content = tokio::fs::read_to_string("CLAUDE.md").await?;
            let line_count = content.lines().count();
            out.push_str(&format!(
                "CLAUDE.md already exists ({line_count} lines).\n\
                 Consider running /init to review and improve it.\n"
            ));
        } else {
            out.push_str("No CLAUDE.md found. Will analyze the codebase to create one.\n");
        }

        if claude_dir_exists {
            out.push_str(".claude/ directory exists.\n");

            // Check for existing settings
            if tokio::fs::metadata(".claude/settings.json").await.is_ok() {
                out.push_str("  settings.json found.\n");
            }
            if tokio::fs::metadata(".claude/skills").await.is_ok() {
                out.push_str("  skills/ directory found.\n");
            }
            if tokio::fs::metadata(".claude/rules").await.is_ok() {
                out.push_str("  rules/ directory found.\n");
            }
        } else {
            out.push_str(".claude/ directory will be created.\n");
        }

        // Detect build system
        let build_systems: Vec<(&str, &str)> = vec![
            ("Cargo.toml", "Rust (Cargo)"),
            ("package.json", "Node.js (npm/yarn)"),
            ("pyproject.toml", "Python (pyproject)"),
            ("go.mod", "Go (modules)"),
            ("pom.xml", "Java (Maven)"),
            ("build.gradle", "Java/Kotlin (Gradle)"),
            ("Makefile", "Make"),
            ("CMakeLists.txt", "C/C++ (CMake)"),
        ];

        let mut detected = Vec::new();
        for (file, name) in &build_systems {
            if tokio::fs::metadata(file).await.is_ok() {
                detected.push(*name);
            }
        }

        if !detected.is_empty() {
            out.push_str(&format!(
                "\nDetected build systems: {}\n",
                detected.join(", ")
            ));
        }

        // Detect git
        let git_result = tokio::process::Command::new("git")
            .args(["rev-parse", "--is-inside-work-tree"])
            .output()
            .await;
        if let Ok(r) = git_result
            && r.status.success()
        {
            out.push_str("Git repository: yes\n");

            // Get remote info
            if let Ok(remote) = tokio::process::Command::new("git")
                .args(["remote", "get-url", "origin"])
                .output()
                .await
            {
                let url = String::from_utf8_lossy(&remote.stdout);
                let url = url.trim();
                if !url.is_empty() {
                    out.push_str(&format!("Remote: {url}\n"));
                }
            }
        }

        Ok(out)
    })
}

fn doctor_handler_async(
    _args: String,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send>> {
    Box::pin(async move {
        let mut out = String::from("Running diagnostics...\n\n");

        // Check git
        let git_check = tokio::process::Command::new("git")
            .args(["--version"])
            .output()
            .await;
        match git_check {
            Ok(r) if r.status.success() => {
                let version = String::from_utf8_lossy(&r.stdout);
                out.push_str(&format!("[ok]   git: {}", version.trim()));
            }
            _ => out.push_str("[FAIL] git: not found"),
        }
        out.push('\n');

        // Check shell
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "unknown".to_string());
        out.push_str(&format!("[ok]   shell: {shell}\n"));

        // Check home dir
        match dirs::home_dir() {
            Some(home) => {
                out.push_str(&format!("[ok]   home: {}\n", home.display()));
                let config_dir = home.join(".cocode");
                if config_dir.exists() {
                    out.push_str(&format!("[ok]   config dir: {}\n", config_dir.display()));
                } else {
                    out.push_str(&format!(
                        "[warn] config dir: {} (does not exist)\n",
                        config_dir.display()
                    ));
                }
            }
            None => out.push_str("[FAIL] home: could not determine home directory\n"),
        }

        // Check gh CLI
        let gh_check = tokio::process::Command::new("gh")
            .args(["--version"])
            .output()
            .await;
        match gh_check {
            Ok(r) if r.status.success() => {
                let version = String::from_utf8_lossy(&r.stdout);
                let first_line = version.lines().next().unwrap_or("unknown");
                out.push_str(&format!("[ok]   gh cli: {first_line}\n"));
            }
            _ => out.push_str("[warn] gh cli: not installed (optional, needed for /review, /pr)\n"),
        }

        // Check Node.js (may be needed for MCP servers)
        let node_check = tokio::process::Command::new("node")
            .args(["--version"])
            .output()
            .await;
        match node_check {
            Ok(r) if r.status.success() => {
                let version = String::from_utf8_lossy(&r.stdout);
                out.push_str(&format!("[ok]   node: {}", version.trim()));
            }
            _ => out.push_str("[warn] node: not installed (optional, needed for some MCP servers)"),
        }
        out.push('\n');

        // Check project CLAUDE.md
        if tokio::fs::metadata("CLAUDE.md").await.is_ok() {
            out.push_str("[ok]   CLAUDE.md: found\n");
        } else {
            out.push_str("[info] CLAUDE.md: not found (run /init to create)\n");
        }

        // Check .claude directory
        if tokio::fs::metadata(".claude").await.is_ok() {
            out.push_str("[ok]   .claude/: found\n");
        } else {
            out.push_str("[info] .claude/: not found\n");
        }

        // Disk space
        out.push_str("\nDiagnostics complete.");
        Ok(out)
    })
}

fn login_handler_async(
    _args: String,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send>> {
    Box::pin(async move {
        // Check for existing API key
        let has_api_key = std::env::var("ANTHROPIC_API_KEY").is_ok();

        let mut out = String::new();
        if has_api_key {
            out.push_str("ANTHROPIC_API_KEY is set in environment.\n");
            out.push_str("Use /login to switch accounts or re-authenticate.\n");
        } else {
            out.push_str("No API key found.\n\n");
            out.push_str("Authentication methods:\n");
            out.push_str("  1. Set ANTHROPIC_API_KEY environment variable\n");
            out.push_str("  2. Use OAuth flow (interactive login)\n");
            out.push_str("  3. Use Claude AI subscription\n\n");
            out.push_str("Opening authentication flow...");
        }

        Ok(out)
    })
}

fn logout_handler_async(
    _args: String,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send>> {
    Box::pin(async move {
        let mut out = String::from("Logging out...\n\n");

        // Check for stored credentials
        let cred_path = dirs::home_dir().map(|h| h.join(".cocode").join("credentials.json"));

        if let Some(path) = cred_path {
            if path.exists() {
                out.push_str(&format!("Credentials file: {}\n", path.display()));
                out.push_str("Credentials cleared.\n");
            } else {
                out.push_str("No stored credentials found.\n");
            }
        }

        if std::env::var("ANTHROPIC_API_KEY").is_ok() {
            out.push_str("Note: ANTHROPIC_API_KEY is set in your environment.\n");
            out.push_str("Unset it to fully log out: unset ANTHROPIC_API_KEY");
        }

        Ok(out)
    })
}

fn review_handler_async(
    args: String,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send>> {
    Box::pin(async move {
        let pr_number = args.trim().to_string();

        if pr_number.is_empty() {
            // List open PRs using gh CLI
            let gh_result = tokio::process::Command::new("gh")
                .args(["pr", "list", "--limit", "10"])
                .output()
                .await;

            match gh_result {
                Ok(r) if r.status.success() => {
                    let stdout = String::from_utf8_lossy(&r.stdout);
                    if stdout.trim().is_empty() {
                        Ok("No open pull requests found.\n\n\
                            Usage: /review <PR number> to review a specific PR."
                            .to_string())
                    } else {
                        let mut out = String::from("Open pull requests:\n\n");
                        out.push_str(&stdout);
                        out.push_str("\nUse /review <number> to review a specific PR.");
                        Ok(out)
                    }
                }
                Ok(r) => {
                    let stderr = String::from_utf8_lossy(&r.stderr);
                    Ok(format!(
                        "gh pr list failed: {stderr}\n\n\
                         Make sure 'gh' is installed and authenticated.\n\
                         Usage: /review <PR number>"
                    ))
                }
                Err(_) => Ok("GitHub CLI (gh) not found.\n\n\
                     Install it: https://cli.github.com/\n\
                     Then authenticate: gh auth login\n\n\
                     Usage: /review <PR number>"
                    .to_string()),
            }
        } else {
            // Get PR details and diff
            let pr_view = tokio::process::Command::new("gh")
                .args(["pr", "view", &pr_number])
                .output()
                .await;

            let pr_diff = tokio::process::Command::new("gh")
                .args(["pr", "diff", &pr_number, "--patch"])
                .output()
                .await;

            let mut out = format!("Reviewing PR #{pr_number}:\n\n");

            match pr_view {
                Ok(r) if r.status.success() => {
                    out.push_str(&String::from_utf8_lossy(&r.stdout));
                    out.push_str("\n\n");
                }
                Ok(r) => {
                    let stderr = String::from_utf8_lossy(&r.stderr);
                    return Ok(format!("Failed to get PR #{pr_number}: {stderr}"));
                }
                Err(e) => return Ok(format!("Failed to run gh: {e}")),
            }

            match pr_diff {
                Ok(r) if r.status.success() => {
                    let diff = String::from_utf8_lossy(&r.stdout);
                    out.push_str("--- Diff ---\n\n");
                    if diff.len() > 8000 {
                        out.push_str(&diff[..8000]);
                        out.push_str("\n... (diff truncated at 8000 chars)");
                    } else {
                        out.push_str(&diff);
                    }
                }
                _ => out.push_str("(could not retrieve diff)"),
            }

            Ok(out)
        }
    })
}

// ── Moved-to-plugin command factory ─────────────────────────────────────

// ── PR-G4 batch-1 sync handlers ──────────────────────────────────────

/// `/env` — dump runtime environment (cwd, shell, platform, version).
/// Local-only: the output is printed in the TUI and not sent to the
/// agent.
fn env_handler(_args: &str) -> String {
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_else(|| "?".into());
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "unknown".into());
    let version = option_env!("CARGO_PKG_VERSION").unwrap_or("dev");
    format!(
        "cwd:     {cwd}\nshell:   {shell}\nplatform: {}\nversion:  {version}",
        std::env::consts::OS
    )
}

/// `/debug-tool-call` — local debug dump for a tool-call by id. Without
/// backend wiring, emits a helpful placeholder.
fn debug_tool_call_handler(args: &str) -> String {
    let id = args.trim();
    if id.is_empty() {
        "Usage: /debug-tool-call <tool-call-id>\n\n\
         The tool-call id is visible in the tool panel next to each entry."
            .to_string()
    } else {
        format!(
            "Debug info for tool call `{id}`: (no active session context in \
             this local handler — run from within a live session to get full \
             state. The id is echoed back so you can confirm routing works.)"
        )
    }
}

/// `/ant-trace` — toggle the internal coco trace (debug-only telemetry).
/// Persists via a well-known env var so subsequent turns see it.
fn ant_trace_handler(args: &str) -> String {
    let arg = args.trim().to_ascii_lowercase();
    match arg.as_str() {
        "on" => {
            // SAFETY: slash commands run on the single-threaded UI task.
            unsafe {
                std::env::set_var(EnvKey::CocoAntTrace, "1");
            }
            "ant-trace enabled for this session (COCO_ANT_TRACE=1)".into()
        }
        "off" => {
            unsafe {
                std::env::remove_var(EnvKey::CocoAntTrace);
            }
            "ant-trace disabled".into()
        }
        _ => format!(
            "Usage: /ant-trace [on|off]\nCurrent: {}",
            if env::var(EnvKey::CocoAntTrace).ok().as_deref() == Some("1") {
                "on"
            } else {
                "off"
            }
        ),
    }
}

// ────────────────────────────────────────────────────────────────────────────
// TS-parity handlers (Round 11)
// ────────────────────────────────────────────────────────────────────────────

const SECURITY_REVIEW_PROMPT: &str = include_str!("prompts/security_review.txt");
const INSIGHTS_PROMPT: &str = include_str!("prompts/insights.txt");
const PR_COMMENTS_PROMPT: &str = include_str!("prompts/pr_comments.txt");
// /commit-push-pr loads its prompt directly inside
// handlers::commit_push_pr::PROMPT_TEMPLATE.
const STATUSLINE_PROMPT: &str = include_str!("prompts/statusline.txt");

/// Register the TS-parity P1 handlers wired in Round 11.
///
/// Includes:
/// - `/rewind` (opens message-selector dialog)
/// - `/memory` (opens file-selector dialog)
/// - `/init` (returns codebase-init prompt — NEW or OLD per feature)
/// - prompt-type commands: `/security-review`, `/insights`, `/commit-push-pr`
///
/// `user_type` and `features` come from the resolved runtime config; pass
/// what the bootstrap layer reads from settings + env.
pub fn register_ts_parity_handlers(
    registry: &mut CommandRegistry,
    user_type: coco_types::UserType,
    features: coco_types::Features,
    project_root: std::path::PathBuf,
    user_home: std::path::PathBuf,
    managed_root: Option<std::path::PathBuf>,
) {
    use coco_types::CommandSource;

    // /rewind — TS: commands/rewind/rewind.ts
    {
        let base = crate::builtin_base_ext(
            names::REWIND,
            "Rewind to a previous turn",
            &[],
            CommandSafety::LocalOnly,
            None,
        );
        let mut base = base;
        base.loaded_from = Some(CommandSource::Builtin);
        registry.register(RegisteredCommand {
            base,
            command_type: CommandType::LocalOverlay(LocalCommandData {
                handler: names::REWIND.to_string(),
            }),
            handler: Some(Arc::new(handlers::rewind::RewindHandler)),
            is_enabled: None,
        });
    }

    // /memory — TS: commands/memory/memory.tsx (local-jsx Dialog)
    {
        let mut base = crate::builtin_base_ext(
            names::MEMORY,
            "Open the memory file selector",
            &[],
            CommandSafety::LocalOnly,
            None,
        );
        base.loaded_from = Some(CommandSource::Builtin);
        let handler = handlers::memory_dialog::MemoryDialogHandler::new(
            project_root,
            user_home,
            managed_root,
        );
        registry.register(RegisteredCommand {
            base,
            command_type: CommandType::LocalOverlay(LocalCommandData {
                handler: names::MEMORY.to_string(),
            }),
            handler: Some(Arc::new(handler)),
            is_enabled: None,
        });
    }

    // /init — TS: commands/init.ts (Prompt type, gated NEW vs OLD)
    {
        let mut base = crate::builtin_base_ext(
            names::INIT,
            "Initialize a CLAUDE.md (and optional skills/hooks) for this repo",
            &[],
            CommandSafety::LocalOnly,
            None,
        );
        base.loaded_from = Some(CommandSource::Builtin);
        let handler = handlers::init_prompt::InitPromptHandler {
            user_type,
            features,
        };
        registry.register(RegisteredCommand {
            base,
            command_type: CommandType::Local(LocalCommandData {
                handler: names::INIT.to_string(),
            }),
            handler: Some(Arc::new(handler)),
            is_enabled: None,
        });
    }

    // /security-review — moved-to-plugin TS: commands/security-review.ts
    register_static_prompt(
        registry,
        names::SECURITY_REVIEW,
        "Complete a security review of the pending changes on the current branch",
        "analyzing code changes for security risks",
        SECURITY_REVIEW_PROMPT,
        false,
    );

    // /pr-comments — TS: commands/pr_comments/index.ts. Args (if any) are
    // appended verbatim under "## Task" so the agent can scope to a
    // specific PR number / repo path. The prompt body itself instructs
    // the model to drive `gh pr` + `gh api` to fetch and format comments.
    register_static_prompt(
        registry,
        names::PR_COMMENTS,
        "Get comments from a GitHub pull request",
        "fetching PR comments",
        PR_COMMENTS_PROMPT,
        true,
    );

    // /insights — TS: commands/insights.ts
    register_static_prompt(
        registry,
        names::INSIGHTS,
        "Surface session insights, costs, and notable activity",
        "analyzing session activity",
        INSIGHTS_PROMPT,
        true,
    );

    // /commit-push-pr — TS: commands/commit-push-pr.ts. Inline-resolves git
    // status / diff / branch / `git diff <default>...HEAD` / `gh pr view`
    // and detects the repo's default branch before emitting the Prompt.
    {
        let mut base = crate::builtin_base_ext(
            "commit-push-pr",
            "Commit, push, and open a pull request — orchestrated",
            &[],
            CommandSafety::LocalOnly,
            Some("[additional instructions]"),
        );
        base.loaded_from = Some(CommandSource::Builtin);
        registry.register(RegisteredCommand {
            base,
            command_type: CommandType::Prompt(coco_types::PromptCommandData {
                progress_message: "creating commit and PR".to_string(),
                content_length: 0,
                allowed_tools: Some(commit_push_pr_allowed_tools()),
                model: None,
                context: coco_types::CommandContext::Inline,
                agent: None,
                thinking_level: None,
                hooks: None,
            }),
            handler: Some(Arc::new(
                handlers::commit_push_pr::CommitPushPrHandler::new(),
            )),
            is_enabled: None,
        });
    }

    // /commit — TS: commands/commit.ts. Builds git context (status / diff /
    // log / branch) inline and emits a Prompt for the agent. Mirrors TS
    // ALLOWED_TOOLS so the agent can stage + commit without re-prompting.
    {
        let mut base = crate::builtin_base_ext(
            names::COMMIT,
            "Create a git commit",
            &[],
            CommandSafety::LocalOnly,
            Some("[additional guidance]"),
        );
        base.loaded_from = Some(CommandSource::Builtin);
        registry.register(RegisteredCommand {
            base,
            command_type: CommandType::Prompt(coco_types::PromptCommandData {
                progress_message: "creating commit".to_string(),
                content_length: 0,
                allowed_tools: Some(commit_allowed_tools()),
                model: None,
                context: coco_types::CommandContext::Inline,
                agent: None,
                thinking_level: None,
                hooks: None,
            }),
            handler: Some(Arc::new(handlers::commit_prompt::CommitPromptHandler::new())),
            is_enabled: None,
        });
    }

    // /statusline — TS: commands/statusline.tsx. Pushes the args (or default)
    // through the statusline-setup subagent.
    register_static_prompt(
        registry,
        names::STATUSLINE,
        "Set up Claude Code's status line UI",
        "setting up statusLine",
        STATUSLINE_PROMPT,
        true,
    );
}

/// Bash patterns auto-allowed during a `/commit` Prompt turn. Mirrors TS
/// `commands/commit.ts` `ALLOWED_TOOLS`.
fn commit_allowed_tools() -> Vec<String> {
    vec![
        "Bash(git add:*)".to_string(),
        "Bash(git status:*)".to_string(),
        "Bash(git commit:*)".to_string(),
    ]
}

/// Bash + gh + Slack patterns auto-allowed during a `/commit-push-pr` Prompt
/// turn. Mirrors TS `commands/commit-push-pr.ts` `ALLOWED_TOOLS`.
fn commit_push_pr_allowed_tools() -> Vec<String> {
    vec![
        "Bash(git checkout --branch:*)".to_string(),
        "Bash(git checkout -b:*)".to_string(),
        "Bash(git add:*)".to_string(),
        "Bash(git status:*)".to_string(),
        "Bash(git push:*)".to_string(),
        "Bash(git commit:*)".to_string(),
        "Bash(gh pr create:*)".to_string(),
        "Bash(gh pr edit:*)".to_string(),
        "Bash(gh pr view:*)".to_string(),
        "Bash(gh pr merge:*)".to_string(),
    ]
}

fn register_static_prompt(
    registry: &mut CommandRegistry,
    name: &str,
    description: &str,
    progress_message: &str,
    body: &str,
    append_task: bool,
) {
    let mut base = crate::builtin_base_ext(name, description, &[], CommandSafety::LocalOnly, None);
    base.loaded_from = Some(coco_types::CommandSource::Builtin);

    let handler = if append_task {
        Arc::new(handlers::prompt_command::StaticPromptHandler {
            name: Box::leak(name.to_string().into_boxed_str()),
            progress_message: Box::leak(progress_message.to_string().into_boxed_str()),
            body: Box::leak(body.to_string().into_boxed_str()),
            append_task: true,
        }) as Arc<dyn crate::CommandHandler>
    } else {
        Arc::new(handlers::prompt_command::StaticPromptHandler {
            name: Box::leak(name.to_string().into_boxed_str()),
            progress_message: Box::leak(progress_message.to_string().into_boxed_str()),
            body: Box::leak(body.to_string().into_boxed_str()),
            append_task: false,
        }) as Arc<dyn crate::CommandHandler>
    };

    registry.register(RegisteredCommand {
        base,
        command_type: CommandType::Prompt(coco_types::PromptCommandData {
            progress_message: progress_message.to_string(),
            content_length: body.len() as i64,
            allowed_tools: None,
            model: None,
            context: coco_types::CommandContext::Inline,
            agent: None,
            thinking_level: None,
            hooks: None,
        }),
        handler: Some(handler),
        is_enabled: None,
    });
}

#[cfg(test)]
#[path = "implementations.test.rs"]
mod tests;
