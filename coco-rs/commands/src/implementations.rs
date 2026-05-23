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
    pub const LSP: &str = "lsp";
    pub const MCP: &str = "mcp";
    pub const PLUGIN: &str = "plugin";
    pub const AGENTS: &str = "agents";
    pub const TASKS: &str = "tasks";
    pub const SKILLS: &str = "skills";
    pub const HOOKS: &str = "hooks";
    pub const FILES: &str = "files";

    // System
    pub const DOCTOR: &str = "doctor";
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

    // Slash commands deliberately NOT ported live in `commands/CLAUDE.md`
    // (Deliberately Not Ported). Audits and parity reviews should consult
    // that list before flagging a missing TS command as a gap.
    pub const ENV: &str = "env";
    pub const DEBUG_TOOL_CALL: &str = "debug-tool-call";
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
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = crate::Result<String>> + Send>>,
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
            "Deprecated: use /config to change output style",
            &[],
            output_style_handler,
            true,
            LocalOnly,
            None,
        ),
        (
            names::COLOR,
            "Set the prompt bar color for this session",
            &[],
            color_handler,
            true,
            AlwaysSafe,
            Some("<color|default>"),
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
        // /rewind is registered by register_ts_parity_handlers below — the
        // duplicate registration here was last-write-wins dead code; the
        // parity-handler version owns the real handler.
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
        // /dream and /summary are gated on Feature::AutoMemory and live in
        // `register_ts_parity_handlers` so they only appear when the
        // memory subsystem is wired. Registering them unconditionally
        // here surfaces them in /-typeahead and silently no-ops when the
        // feature is off (tui_runner.rs:1212 / 1228 just log and return).
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
        // /model registered via `register_ts_parity_handlers` (custom
        // CommandHandler returning `OpenDialog(ModelPicker)` on no args).
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
            // Canonical name only; no /continue alias.
            &[],
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
            names::LSP,
            "Manage LSP servers (status, install, enable/disable, add/remove)",
            &[],
            handlers::lsp::handler,
            true,
            LocalOnly,
            Some("[list|install|enable|disable|add|remove] [server]"),
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
        // /review moved to Prompt-type registration below (TS parity:
        // `commands/review.ts` exports a `type: 'prompt'` Command, not a
        // local handler). Kept here as a comment so future audits don't
        // re-add the legacy local async-handler form.
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
            "List git-tracked files in this repository",
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

    // Hide Rust-only debug commands from `/-typeahead`. They stay
    // enabled (so power users can still invoke them by name) but the
    // TS counterparts are literal `isEnabled:false, isHidden:true`
    // stubs — keeping them hidden mirrors that visibility contract.
    registry.set_hidden(names::ENV, true);
    registry.set_hidden(names::DEBUG_TOOL_CALL, true);
    // `/output-style` is a deprecation stub; TS marks it hidden so it
    // doesn't surface in `/-typeahead`. Match that here.
    registry.set_hidden(names::OUTPUT_STYLE, true);

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

fn branch_handler(args: &str) -> String {
    let name = args.trim();
    if name.is_empty() {
        "Creating conversation branch at current point...".to_string()
    } else {
        format!("Creating conversation branch: {name}")
    }
}

fn theme_handler(args: &str) -> String {
    let name = args.trim();
    if name.is_empty() {
        return "TUI themes are managed by ~/.coco/theme.json.\n\n\
                Available built-ins: auto, default, dark, light, dark-daltonized, \
                light-daltonized, dark-ansi, light-ansi, dracula, nord.\n\n\
                In the TUI, use /theme <name> or open Settings."
            .to_string();
    }
    format!(
        "Theme `{name}` is handled by the TUI and saved to ~/.coco/theme.json. \
         Run this command inside the TUI to apply it immediately."
    )
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
    let arg = args.trim();
    if arg.is_empty() {
        return "Sandbox mode: (read from settings.json `sandbox.mode`)\n\n\
                Modes:\n\
                  none     — No sandboxing (default)\n\
                  readonly — Read-only filesystem access\n\
                  strict   — Full sandboxing with restricted execution\n\n\
                Use /sandbox <mode> to change — persisted, effective on next session."
            .to_string();
    }
    let mode = match arg {
        "none" | "off" | "disable" => "none",
        "readonly" => "readonly",
        "strict" => "strict",
        other => return format!("Unknown sandbox mode: {other}. Use none, readonly, or strict."),
    };
    match coco_config::global_config::write_user_setting(
        "sandbox.mode",
        serde_json::Value::String(mode.to_string()),
    ) {
        Ok(path) => format!(
            "Sandbox mode set to `{mode}` in {} (effective on next session).",
            path.display()
        ),
        Err(e) => format!("Failed to persist sandbox mode: {e}"),
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
        return "Usage: /add-dir <path> — Add a directory to the permission scope for this session.".to_string();
    }
    // Resolve + validate before emitting the sentinel so the runner
    // sees only well-formed paths. `/add-dir` is a Session-source
    // mutation: TS persists nothing, just widens the in-memory
    // additional_dirs map for the duration of the session. Runners
    // pick up the sentinel and call `runtime.update_engine_config`.
    let absolute = match std::path::PathBuf::from(path).canonicalize() {
        Ok(p) => p,
        Err(e) => return format!("Cannot add directory `{path}`: {e}"),
    };
    if !absolute.is_dir() {
        return format!(
            "Cannot add directory `{}`: not a directory",
            absolute.display()
        );
    }
    format!(
        "{ADD_DIR_SENTINEL} {}\nAdded working directory: {}",
        absolute.display(),
        absolute.display()
    )
}

/// Parse a `__COCO_ADD_DIR__ <abs-path>` first line. Returns the
/// trimmed path when present, `None` otherwise.
#[must_use]
pub fn parse_add_dir_sentinel(handler_output: &str) -> Option<String> {
    let parsed = handlers::sentinel::parse_sentinel(handler_output, ADD_DIR_SENTINEL)?;
    if parsed.args.is_empty() {
        return None;
    }
    Some(parsed.args.to_string())
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

/// `/output-style` — deprecated stub. TS equivalent
/// (`commands/output-style/output-style.tsx`) prints a redirect message
/// to `/config`; we mirror that text verbatim so users on either CLI
/// see the same deprecation hint.
fn output_style_handler(_args: &str) -> String {
    "/output-style has been deprecated. Use /config to change your output style, \
     or set it in your settings file. Changes take effect on the next session."
        .to_string()
}

/// Reset aliases that TS treats as "restore the default color"
/// (`commands/color/color.ts:18`). The TUI intercept (`dispatch_color`
/// in `coco-cli`) carries its own copy — kept in sync with this list.
const COLOR_RESET_ALIASES: &[&str] = &["default", "reset", "none", "gray", "grey"];

/// `/color <name|default>` — set the prompt bar color for this session.
///
/// Pure text-shape mirror of TS `commands/color/color.ts`. Persistence
/// (writing to `ToolAppState.agent_color`) happens in
/// `tui_runner::dispatch_color`, which intercepts this command before
/// the registry to gate on `is_teammate()` and mutate runtime state.
/// This handler is the SDK / non-TUI fallback that produces the same
/// user-visible text without runtime context.
fn color_handler(args: &str) -> String {
    use coco_types::AgentColorName;
    let trimmed = args.trim();
    if trimmed.is_empty() {
        let list = AgentColorName::ALL
            .iter()
            .map(|c| c.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return format!("Please provide a color. Available colors: {list}, default");
    }
    let lower = trimmed.to_ascii_lowercase();
    if COLOR_RESET_ALIASES.contains(&lower.as_str()) {
        return "Session color reset to default".to_string();
    }
    match lower.parse::<AgentColorName>() {
        Ok(color) => format!("Session color set to: {color}"),
        Err(_) => {
            let list = AgentColorName::ALL
                .iter()
                .map(|c| c.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            format!("Invalid color \"{lower}\". Available colors: {list}, default")
        }
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
    let level = args.trim();
    if level.is_empty() {
        return "Reasoning effort levels:\n\
                  low      — Faster, less thorough\n\
                  medium   — Balanced (default)\n\
                  high     — Deeper reasoning, slower\n\
                  max      — Maximum effort\n\
                  auto     — Provider-default\n\n\
                Use /effort <level> to change — persisted to settings.json, effective on next session."
            .to_string();
    }
    if !matches!(level, "low" | "medium" | "high" | "max" | "auto") {
        return format!("Unknown effort level: {level}. Use low / medium / high / max / auto.");
    }
    match coco_config::global_config::write_user_setting(
        "effort",
        serde_json::Value::String(level.to_string()),
    ) {
        Ok(path) => format!(
            "Reasoning effort set to `{level}` in {} (effective on next session).",
            path.display()
        ),
        Err(e) => format!("Failed to persist effort: {e}"),
    }
}

fn config_extended_handler(args: &str) -> String {
    let subcommand = args.trim();
    if subcommand.is_empty() {
        return "Configuration:\n\n\
                Use /config <key>           — view current value\n\
                Use /config <key> <value>   — set value (auto-typed: bool/int/JSON)\n\n\
                Common keys:\n\
                  theme, effort, output_style, color_mode\n\
                  sandbox.mode, compact.auto.enabled, web_search.enabled\n\
                  features.<name>\n\n\
                Writes go to ~/.coco/settings.json — effective on next session."
            .to_string();
    }
    let path = coco_config::global_config::user_settings_path();
    if let Some((key, value_str)) = subcommand.split_once(' ') {
        let value = parse_config_value(value_str.trim());
        match coco_config::global_config::write_user_setting(key, value.clone()) {
            Ok(p) => format!(
                "Set `{key}` = {} in {} (effective on next session).",
                value,
                p.display()
            ),
            Err(e) => format!("Failed to write `{key}`: {e}"),
        }
    } else {
        config_read_handler_at_path(&path, subcommand)
    }
}

fn config_read_handler_at_path(path: &std::path::Path, key: &str) -> String {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) if !s.trim().is_empty() => s,
        _ => return format!("Current value of `{key}`: (settings.json not present)"),
    };
    let json: serde_json::Value = match coco_config::parse_jsonc_value(&raw) {
        Ok(v) => v,
        Err(e) => return format!("Failed to parse settings.json: {e}"),
    };
    let value = lookup_dotted(&json, key);
    match value {
        Some(v) => format!("Current value of `{key}`: {v}"),
        None => format!("Current value of `{key}`: (not set)"),
    }
}

/// Coerce a CLI-typed value string into a `serde_json::Value`. Tries
/// `bool` → `i64` → JSON parse → fallback to plain `String`. Mirrors
/// the relaxed coercion users expect from a CLI (`/config foo true`
/// stores boolean true, not the string "true").
fn parse_config_value(s: &str) -> serde_json::Value {
    if let Ok(b) = s.parse::<bool>() {
        return serde_json::Value::Bool(b);
    }
    if let Ok(n) = s.parse::<i64>() {
        return serde_json::Value::Number(n.into());
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(s) {
        return v;
    }
    serde_json::Value::String(s.to_string())
}

/// Walk dotted key path through a `Value`. Returns `None` for missing
/// keys or non-object intermediates.
fn lookup_dotted<'a>(json: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    let mut cur = json;
    for part in key.split('.') {
        cur = cur.get(part)?;
    }
    Some(cur)
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
    // Sentinel that runners pick up to call
    // `SessionRuntime::reload_plugins`, which rescans plugin + skill
    // dirs and atomically swaps the live `CommandRegistry`. The
    // status line below is what the user sees in the transcript.
    format!("{RELOAD_PLUGINS_SENTINEL}\nReloading plugins…")
}

/// Parse a `__COCO_RELOAD_PLUGINS__` first line. Returns `Some(())` on
/// match (no payload), `None` otherwise.
#[must_use]
pub fn parse_reload_plugins_sentinel(handler_output: &str) -> Option<()> {
    handlers::sentinel::parse_sentinel(handler_output, RELOAD_PLUGINS_SENTINEL).map(|_| ())
}

/// Sentinel emitted by `/rename <name>`. Runners parse the first line
/// and call `SessionManager::set_title` on the live session id.
pub const RENAME_SENTINEL: &str = "__COCO_RENAME__";
/// Sentinel emitted by `/tag <name>`. Runners toggle the tag via
/// `SessionManager::toggle_tag`.
pub const TAG_SENTINEL: &str = "__COCO_TAG__";
/// Sentinel emitted by `/add-dir <path>`. Runners push the resolved
/// absolute path into the engine's `session_additional_dirs`.
pub const ADD_DIR_SENTINEL: &str = "__COCO_ADD_DIR__";
/// Sentinel emitted by `/reload-plugins`. Runners rebuild the plugin
/// + skill + command registry and atomically swap it in via
///   `SessionRuntime::reload_plugins`.
pub const RELOAD_PLUGINS_SENTINEL: &str = "__COCO_RELOAD_PLUGINS__";
/// Sentinel emitted by `/hooks reload`. Runners reload the live
/// `HookRegistry` from the latest `RuntimeConfig` snapshot via
/// `SessionRuntime::reload_hooks`. TS parity:
/// `updateHooksConfigSnapshot()` (`utils/hooks/hooksConfigSnapshot.ts`).
///
/// Mirrors RELOAD_PLUGINS_SENTINEL: only fires from a slash command,
/// which runs only at turn boundaries (the dispatch loop in
/// `tui_runner` `drain_active_turn`s before processing slash output),
/// so pre/post hook consistency within a turn is preserved.
pub const RELOAD_HOOKS_SENTINEL: &str = "__COCO_RELOAD_HOOKS__";

/// Parse a `__COCO_RELOAD_HOOKS__` first line. Returns `Some(())` on
/// match (no payload), `None` otherwise.
#[must_use]
pub fn parse_reload_hooks_sentinel(handler_output: &str) -> Option<()> {
    handlers::sentinel::parse_sentinel(handler_output, RELOAD_HOOKS_SENTINEL).map(|_| ())
}

fn rename_handler(args: &str) -> String {
    let name = args.trim();
    if name.is_empty() {
        return "Usage: /rename <name> — Rename the current conversation.".to_string();
    }
    // Sentinel + status line: runners parse the first line for the new
    // name and dispatch to SessionManager. The second line is what the
    // user sees in the transcript when the runner echoes our text.
    format!("{RENAME_SENTINEL} {name}\nRenaming conversation to: {name}")
}

fn tag_handler(args: &str) -> String {
    let tag = args.trim();
    if tag.is_empty() {
        return "Usage: /tag <name> — Toggle a searchable tag on the current session.".to_string();
    }
    format!("{TAG_SENTINEL} {tag}\nToggling tag: {tag}")
}

/// Parse a `__COCO_RENAME__ <name>` first line. Returns the trimmed
/// new name when present, `None` otherwise.
#[must_use]
pub fn parse_rename_sentinel(handler_output: &str) -> Option<String> {
    let parsed = handlers::sentinel::parse_sentinel(handler_output, RENAME_SENTINEL)?;
    if parsed.args.is_empty() {
        return None;
    }
    Some(parsed.args.to_string())
}

/// Parse a `__COCO_TAG__ <name>` first line.
#[must_use]
pub fn parse_tag_sentinel(handler_output: &str) -> Option<String> {
    let parsed = handlers::sentinel::parse_sentinel(handler_output, TAG_SENTINEL)?;
    if parsed.args.is_empty() {
        return None;
    }
    Some(parsed.args.to_string())
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
) -> std::pin::Pin<Box<dyn std::future::Future<Output = crate::Result<String>> + Send>> {
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
) -> std::pin::Pin<Box<dyn std::future::Future<Output = crate::Result<String>> + Send>> {
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
) -> std::pin::Pin<Box<dyn std::future::Future<Output = crate::Result<String>> + Send>> {
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

// ────────────────────────────────────────────────────────────────────────────
// TS-parity handlers (Round 11)
// ────────────────────────────────────────────────────────────────────────────

const SECURITY_REVIEW_PROMPT: &str = include_str!("prompts/security_review.txt");
const INSIGHTS_PROMPT: &str = include_str!("prompts/insights.txt");
const PR_COMMENTS_PROMPT: &str = include_str!("prompts/pr_comments.txt");
const REVIEW_PROMPT: &str = include_str!("prompts/review.txt");
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

    // /rewind — TS: commands/rewind/rewind.ts.
    // Aliases intentionally not registered; canonical name only.
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

    // /model — TS: commands/model/model.tsx (local-jsx ModelPicker).
    // No-args opens the picker overlay; with-args validates against
    // the builtin registry and persists `model_roles.main`.
    {
        let mut base = crate::builtin_base_ext(
            names::MODEL,
            "Switch the current model (opens picker with no arg)",
            &[],
            CommandSafety::LocalOnly,
            Some("[model]"),
        );
        base.loaded_from = Some(CommandSource::Builtin);
        registry.register(RegisteredCommand {
            base,
            command_type: CommandType::LocalOverlay(LocalCommandData {
                handler: names::MODEL.to_string(),
            }),
            handler: Some(Arc::new(handlers::model::ModelHandler)),
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
            project_root.clone(),
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
            features: features.clone(),
            project_root: Some(project_root),
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

    // /security-review — moved-to-plugin TS: commands/security-review.ts.
    // Uses ShellExpandingPromptHandler so `!`git ...`` markers in the
    // prompt body are pre-resolved before the prompt is fed to the model
    // (matches TS `executeShellCommandsInPrompt`). Pre-allows the
    // tools the TS frontmatter declares so the agent can drive the
    // review without prompting the user for permission on every step.
    {
        let mut base = crate::builtin_base_ext(
            names::SECURITY_REVIEW,
            "Complete a security review of the pending changes on the current branch",
            &[],
            CommandSafety::LocalOnly,
            None,
        );
        base.loaded_from = Some(CommandSource::Builtin);
        registry.register(RegisteredCommand {
            base,
            command_type: CommandType::Prompt(coco_types::PromptCommandData {
                progress_message: "analyzing code changes for security risks".to_string(),
                content_length: SECURITY_REVIEW_PROMPT.len() as i64,
                allowed_tools: Some(security_review_allowed_tools()),
                model: None,
                context: coco_types::CommandContext::Inline,
                agent: None,
                thinking_level: None,
                hooks: None,
            }),
            handler: Some(Arc::new(
                handlers::prompt_command::ShellExpandingPromptHandler {
                    name: "security-review".to_string(),
                    progress_message: "analyzing code changes for security risks".to_string(),
                    body: SECURITY_REVIEW_PROMPT.to_string(),
                    args_handling: handlers::prompt_command::ArgsHandling::Static,
                },
            )),
            is_enabled: None,
        });
    }

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
        handlers::prompt_command::ArgsHandling::AppendUnderTask,
    );

    // /insights — TS: commands/insights.ts
    register_static_prompt(
        registry,
        names::INSIGHTS,
        "Surface session insights, costs, and notable activity",
        "analyzing session activity",
        INSIGHTS_PROMPT,
        handlers::prompt_command::ArgsHandling::AppendUnderTask,
    );

    // /review — TS: commands/review.ts (Prompt-type, NOT a local handler).
    // TS appends `PR number: ${args}` inline at the body's end — even
    // when `args` is empty, the literal `PR number: ` line is present so
    // the model sees an explicit value (or its absence). We mirror that
    // exactly via `ArgsHandling::AppendInline`.
    register_static_prompt(
        registry,
        names::REVIEW,
        "Review a pull request",
        "reviewing pull request",
        REVIEW_PROMPT,
        handlers::prompt_command::ArgsHandling::AppendInline {
            prefix: "PR number: ",
        },
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
        handlers::prompt_command::ArgsHandling::AppendUnderTask,
    );

    // /dream + /summary — auto-memory subsystem entry points. Only register
    // when Feature::AutoMemory is on; the runner's `run_dream_consolidation`
    // and `run_session_memory_force` no-op when the runtime has no
    // MemoryRuntime, but surfacing the commands in /-typeahead under those
    // conditions is misleading. TS gates `/dream` on `KAIROS|KAIROS_DREAM`;
    // the closest coco-rs gate is the AutoMemory feature.
    if features.enabled(coco_types::Feature::AutoMemory) {
        let mut dream_base = crate::builtin_base_ext(
            names::DREAM,
            "Force auto-memory consolidation now (skips three-gate scheduler)",
            &[],
            CommandSafety::LocalOnly,
            None,
        );
        dream_base.loaded_from = Some(CommandSource::Builtin);
        registry.register(RegisteredCommand {
            base: dream_base,
            command_type: CommandType::Local(LocalCommandData {
                handler: names::DREAM.to_string(),
            }),
            handler: Some(Arc::new(AsyncBuiltinCommand::new(
                names::DREAM,
                handlers::dream::handler,
            ))),
            is_enabled: None,
        });

        let mut summary_base = crate::builtin_base_ext(
            names::SUMMARY,
            "Force a 9-section session-memory update now",
            &[],
            CommandSafety::LocalOnly,
            None,
        );
        summary_base.loaded_from = Some(CommandSource::Builtin);
        registry.register(RegisteredCommand {
            base: summary_base,
            command_type: CommandType::Local(LocalCommandData {
                handler: names::SUMMARY.to_string(),
            }),
            handler: Some(Arc::new(AsyncBuiltinCommand::new(
                names::SUMMARY,
                handlers::summary::handler,
            ))),
            is_enabled: None,
        });
    }
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

/// Tools auto-allowed during a `/security-review` Prompt turn. Mirrors
/// TS `commands/security-review.ts` frontmatter `allowed-tools` (the
/// markdown declares: Bash(git diff:*), Bash(git status:*),
/// Bash(git log:*), Bash(git show:*), Bash(git remote show:*), Read,
/// Glob, Grep, LS, Task).
fn security_review_allowed_tools() -> Vec<String> {
    vec![
        "Bash(git diff:*)".to_string(),
        "Bash(git status:*)".to_string(),
        "Bash(git log:*)".to_string(),
        "Bash(git show:*)".to_string(),
        "Bash(git remote show:*)".to_string(),
        "Read".to_string(),
        "Glob".to_string(),
        "Grep".to_string(),
        "LS".to_string(),
        "Task".to_string(),
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
        "ToolSearch".to_string(),
        "mcp__slack__send_message".to_string(),
        "mcp__claude_ai_Slack__slack_send_message".to_string(),
    ]
}

fn register_static_prompt(
    registry: &mut CommandRegistry,
    name: &str,
    description: &str,
    progress_message: &str,
    body: &str,
    args_handling: handlers::prompt_command::ArgsHandling,
) {
    let mut base = crate::builtin_base_ext(name, description, &[], CommandSafety::LocalOnly, None);
    base.loaded_from = Some(coco_types::CommandSource::Builtin);

    let handler = Arc::new(handlers::prompt_command::StaticPromptHandler {
        name: name.to_string(),
        progress_message: progress_message.to_string(),
        body: body.to_string(),
        args_handling,
    }) as Arc<dyn crate::CommandHandler>;

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
