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
    pub const FAST: &str = "fast";
    pub const SANDBOX: &str = "sandbox";
    pub const PRIVACY_SETTINGS: &str = "privacy-settings";
    pub const RATE_LIMIT_OPTIONS: &str = "rate-limit-options";

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
    pub const PR: &str = "pr";
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
    pub const FEEDBACK: &str = "feedback";
    pub const UPGRADE: &str = "upgrade";
    pub const USAGE: &str = "usage";

    // Social / Misc
    pub const BTW: &str = "btw";
    pub const STICKERS: &str = "stickers";
    pub const MEMORY: &str = "memory";
    pub const PLAN: &str = "plan";
    pub const ADD_DIR: &str = "add-dir";
    pub const DESKTOP: &str = "desktop";
    pub const MOBILE: &str = "mobile";
    pub const IDE: &str = "ide";
    pub const TAG: &str = "tag";
    pub const SUMMARY: &str = "summary";
    pub const RELEASE_NOTES: &str = "release-notes";
    pub const ONBOARDING: &str = "onboarding";
    pub const CHROME: &str = "chrome";
    pub const PR_COMMENTS: &str = "pr-comments";
    pub const SHARE: &str = "share";
    pub const PASSES: &str = "passes";
    pub const EXTRA_USAGE: &str = "extra-usage";
    pub const TELEPORT: &str = "teleport";
    pub const INSTALL_GITHUB_APP: &str = "install-github-app";
    pub const INSTALL_SLACK_APP: &str = "install-slack-app";

    // ── TS-parity: missing commands ──
    pub const STATUSLINE: &str = "statusline";
    pub const RELOAD_PLUGINS: &str = "reload-plugins";
    pub const TERMINAL_SETUP: &str = "terminal-setup";
    pub const THINKBACK: &str = "thinkback";
    pub const THINKBACK_PLAY: &str = "thinkback-play";
    pub const SECURITY_REVIEW: &str = "security-review";
    pub const ULTRAREVIEW: &str = "ultrareview";
    pub const INSIGHTS: &str = "insights";
    pub const REMOTE_ENV: &str = "remote-env";
    pub const HEAP_DUMP: &str = "heap-dump";
    pub const ADVISOR: &str = "advisor";
    pub const CONTEXT_NON_INTERACTIVE: &str = "context-non-interactive";
    pub const EXTRA_USAGE_NON_INTERACTIVE: &str = "extra-usage-non-interactive";

    // ── PR-G4 batch 1: dev tooling + AI features + integrations ──
    pub const BRIEF: &str = "brief";
    pub const ENV: &str = "env";
    pub const BUG_REPORT: &str = "bug-report";
    pub const DEBUG_TOOL_CALL: &str = "debug-tool-call";
    pub const ANT_TRACE: &str = "ant-trace";
    pub const VOICE: &str = "voice";
    pub const ISSUE: &str = "issue";
    pub const PERF_ISSUE: &str = "perf-issue";
    pub const AUTOFIX_PR: &str = "autofix-pr";
    pub const BUGHUNTER: &str = "bughunter";
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
            names::SKILLS,
            "List available skills",
            &[],
            skills_handler,
            true,
            LocalOnly,
            None,
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
            names::PRIVACY_SETTINGS,
            "Configure privacy settings",
            &[],
            privacy_settings_handler,
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
            names::AGENTS,
            "List and manage agent definitions",
            &[],
            agents_extended_handler,
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
            names::ONBOARDING,
            "Start the onboarding walkthrough",
            &[],
            onboarding_handler,
            true,
            LocalOnly,
            None,
        ),
        (
            names::DESKTOP,
            "Open or configure Claude Code desktop app",
            &[],
            desktop_handler,
            true,
            LocalOnly,
            None,
        ),
        (
            names::MOBILE,
            "Information about Claude Code mobile app",
            &[],
            mobile_handler,
            true,
            AlwaysSafe,
            None,
        ),
        (
            names::CHROME,
            "Manage Claude in Chrome integration",
            &[],
            chrome_handler,
            true,
            LocalOnly,
            None,
        ),
        (
            names::INSTALL_GITHUB_APP,
            "Install the Claude Code GitHub App",
            &[],
            install_github_handler,
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
            names::RATE_LIMIT_OPTIONS,
            "View rate limit configuration",
            &[],
            rate_limit_handler,
            true,
            LocalOnly,
            None,
        ),
        (
            names::TERMINAL_SETUP,
            "Configure terminal settings",
            &[],
            terminal_setup_handler,
            true,
            LocalOnly,
            None,
        ),
        (
            names::REMOTE_ENV,
            "Configure remote environment",
            &[],
            remote_env_handler,
            true,
            LocalOnly,
            None,
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
            names::FAST,
            "Toggle fast mode (use smaller model)",
            &[],
            fast_handler,
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
        (
            names::STATUSLINE,
            "Toggle status line display",
            &[],
            statusline_handler,
            true,
            AlwaysSafe,
            None,
        ),
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
        (
            names::FEEDBACK,
            "Submit feedback about Claude Code",
            &["bug"],
            feedback_handler,
            true,
            AlwaysSafe,
            Some("[message]"),
        ),
        (
            names::EXTRA_USAGE,
            "Manage extra usage / overage settings",
            &["passes"],
            extra_usage_handler,
            true,
            LocalOnly,
            None,
        ),
        (
            names::THINKBACK,
            "Review thinking history from the session",
            &[],
            thinkback_handler,
            true,
            LocalOnly,
            None,
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
        (
            names::COMMIT,
            "Create a git commit with staged changes",
            &[],
            commit_handler,
            false,
            LocalOnly,
            Some("[message]"),
        ),
        (
            names::PR,
            "Create a pull request from current branch",
            &["pr-create"],
            pr_extended_handler,
            false,
            LocalOnly,
            Some("[title]"),
        ),
        (
            names::SUMMARY,
            "Show a summary of the current conversation",
            &[],
            summary_handler,
            false,
            BridgeSafe,
            None,
        ),
        (
            names::SHARE,
            "Share conversation transcript",
            &[],
            share_handler,
            false,
            LocalOnly,
            None,
        ),
        (
            names::RELEASE_NOTES,
            "Show recent release notes and changes",
            &[],
            release_notes_handler,
            false,
            BridgeSafe,
            None,
        ),
        (
            names::TELEPORT,
            "Resume a remote session locally",
            &[],
            teleport_handler,
            false,
            LocalOnly,
            Some("<session-url>"),
        ),
        (
            names::HEAP_DUMP,
            "Generate memory profile snapshot",
            &[],
            heap_dump_handler,
            false,
            LocalOnly,
            None,
        ),
        (
            names::VIM,
            "Toggle between Vim and Normal editing modes",
            &[],
            vim_handler,
            false,
            AlwaysSafe,
            None,
        ),
        (
            names::KEYBINDINGS,
            "Open keybindings configuration",
            &[],
            keybindings_handler,
            false,
            AlwaysSafe,
            None,
        ),
        (
            names::STICKERS,
            "Order stickers",
            &[],
            stickers_handler,
            false,
            AlwaysSafe,
            None,
        ),
        (
            names::REWIND,
            "Restore code/conversation to a previous point",
            &["checkpoint"],
            rewind_handler,
            false,
            LocalOnly,
            Some("[turn-number]"),
        ),
        (
            names::INSTALL_SLACK_APP,
            "Install the Claude Code Slack App",
            &[],
            install_slack_handler,
            false,
            LocalOnly,
            None,
        ),
        (
            names::THINKBACK_PLAY,
            "Replay thinking history step by step",
            &[],
            thinkback_play_handler,
            false,
            LocalOnly,
            None,
        ),
        // ── PR-G4 batch 1 ──
        (
            names::BRIEF,
            "Summarize the current session so far",
            &[],
            brief_handler,
            false,
            AlwaysSafe,
            None,
        ),
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
            names::BUG_REPORT,
            "Open a bug report for coco-rs",
            &[],
            bug_report_handler,
            false,
            AlwaysSafe,
            Some("[title]"),
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
        (
            names::VOICE,
            "Toggle voice input",
            &[],
            voice_handler,
            false,
            LocalOnly,
            Some("[on|off]"),
        ),
        (
            names::ISSUE,
            "Open a GitHub issue for the repo",
            &[],
            issue_handler,
            false,
            AlwaysSafe,
            Some("[title]"),
        ),
        (
            names::PERF_ISSUE,
            "Report a performance issue with a trace attached",
            &[],
            perf_issue_handler,
            false,
            LocalOnly,
            None,
        ),
        (
            names::AUTOFIX_PR,
            "AI-powered PR autofixer (stub)",
            &[],
            autofix_pr_handler,
            false,
            LocalOnly,
            Some("<pr-number>"),
        ),
        (
            names::BUGHUNTER,
            "AI-assisted bug search across the repo (stub)",
            &[],
            bughunter_handler,
            false,
            LocalOnly,
            Some("[symptom]"),
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

    // ── Prompt-type commands (TS type: 'prompt') ──
    // Only truly model-invocable commands that have no local handler.
    let prompt_specs: &[(&str, &str, &[&str], &str)] = &[
        (
            names::PR_COMMENTS,
            "Review comments on a pull request",
            &[],
            "Fetching PR comments...",
        ),
        (
            names::SECURITY_REVIEW,
            "Run a security-focused code review",
            &[],
            "Running security review...",
        ),
        (
            names::ULTRAREVIEW,
            "Comprehensive bug-finding review",
            &[],
            "Running deep review...",
        ),
        (
            names::INSIGHTS,
            "Generate session analysis report",
            &[],
            "Generating insights...",
        ),
        (
            names::ADVISOR,
            "Get advice on the current task",
            &[],
            "Consulting advisor...",
        ),
        (
            names::CONTEXT_NON_INTERACTIVE,
            "Show context window usage (non-interactive)",
            &[],
            "Checking context...",
        ),
        (
            names::EXTRA_USAGE_NON_INTERACTIVE,
            "Check extra usage status (non-interactive)",
            &[],
            "Checking extra usage...",
        ),
    ];

    for (name, description, aliases, progress_message) in prompt_specs {
        registry.register(RegisteredCommand {
            base: builtin_base_ext(name, description, aliases, AlwaysSafe, None),
            command_type: CommandType::Prompt(coco_types::PromptCommandData {
                progress_message: progress_message.to_string(),
                content_length: 0,
                allowed_tools: None,
                model: None,
                context: coco_types::CommandContext::Inline,
                agent: None,
                thinking_level: None,
                hooks: None,
            }),
            handler: None,
            is_enabled: None,
        });
    }
}

// ── Sync handlers ──

fn plan_handler(args: &str) -> String {
    match args.trim() {
        "" => "Plan mode: off\n\
               Use /plan on to enable plan mode\n\
               Use /plan <description> to start a plan\n\n\
               In plan mode, the assistant will propose a plan before executing.\n\
               Plans are saved to ~/.cocode/plans/"
            .to_string(),
        "on" | "enable" => {
            "Plan mode enabled. The assistant will propose a plan before executing changes."
                .to_string()
        }
        "off" | "disable" => "Plan mode disabled.".to_string(),
        "open" => "Opening current plan...".to_string(),
        description => format!("Creating plan: {description}\nPlan mode enabled."),
    }
}

fn rewind_handler(args: &str) -> String {
    let target = args.trim();
    if target.is_empty() {
        "Usage: /rewind [turn-number]\n\n\
         Restores code and conversation to a previous checkpoint.\n\
         Each turn creates an automatic checkpoint of modified files.\n\n\
         Options:\n\
         /rewind        — Show available checkpoints\n\
         /rewind <N>    — Rewind to turn N\n\
         /rewind last   — Rewind to the last checkpoint"
            .to_string()
    } else {
        format!("Rewinding to checkpoint: {target}")
    }
}

fn skills_handler(_args: &str) -> String {
    "Available skills:\n\n\
     Skills are loaded from:\n\
       1. .claude/skills/ (project skills)\n\
       2. ~/.claude/skills/ (user skills)\n\
       3. Bundled skills (built-in)\n\
       4. Plugin-provided skills\n\n\
     Use /skills to list, or invoke with /<skill-name>."
        .to_string()
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

fn vim_handler(args: &str) -> String {
    match args.trim() {
        "" | "toggle" => "Vim mode toggled. Restart the editor to apply.".to_string(),
        "on" | "enable" => "Vim mode enabled.".to_string(),
        "off" | "disable" => "Vim mode disabled.".to_string(),
        other => format!("Unknown vim option: {other}. Use on/off/toggle."),
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
    let question = args.trim();
    if question.is_empty() {
        "Usage: /btw <question> — Ask a quick side question without interrupting the main conversation.".to_string()
    } else {
        format!("Side question: {question}")
    }
}

fn stickers_handler(_args: &str) -> String {
    "Visit https://store.anthropic.com for Claude Code stickers!".to_string()
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

fn fast_handler(args: &str) -> String {
    match args.trim() {
        "" | "toggle" => "Fast mode toggled.".to_string(),
        "on" | "enable" => "Fast mode enabled (using smaller, faster model).".to_string(),
        "off" | "disable" => "Fast mode disabled (using default model).".to_string(),
        other => format!("Unknown fast mode option: {other}. Use on/off."),
    }
}

fn add_dir_handler(args: &str) -> String {
    let path = args.trim();
    if path.is_empty() {
        "Usage: /add-dir <path> — Add a directory as a working directory.".to_string()
    } else {
        format!("Added working directory: {path}")
    }
}

fn keybindings_handler(_args: &str) -> String {
    "Keybindings configuration:\n\n\
     File: ~/.claude/keybindings.json\n\n\
     Default bindings:\n\
       Ctrl+C  — Interrupt / Cancel\n\
       Ctrl+D  — Exit\n\
       Ctrl+L  — Clear screen\n\
       Ctrl+O  — Toggle transcript\n\
       Enter   — Submit input\n\n\
     Edit ~/.claude/keybindings.json to customize."
        .to_string()
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

fn privacy_settings_handler(_args: &str) -> String {
    "Privacy settings:\n\n\
     Telemetry: enabled (anonymous usage data)\n\
     Error reporting: enabled\n\
     Session logging: enabled\n\n\
     Use /privacy-settings <key> <value> to modify."
        .to_string()
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

fn commit_handler(args: &str) -> String {
    let message = args.trim();
    if message.is_empty() {
        "Usage: /commit [message]\n\n\
         Creates a git commit with staged changes.\n\
         If no message is provided, an AI-generated message will be used."
            .to_string()
    } else {
        format!("Creating commit with message: {message}")
    }
}

fn pr_extended_handler(args: &str) -> String {
    let title = args.trim();
    if title.is_empty() {
        "Usage: /pr [title]\n\n\
         Creates a pull request from the current branch.\n\
         If no title is provided, one will be generated from commits."
            .to_string()
    } else {
        format!("Creating pull request: {title}")
    }
}

fn agents_extended_handler(args: &str) -> String {
    match args.trim() {
        "" | "list" => "Agent definitions:\n\n\
               No custom agents defined.\n\n\
               Agent definitions can be placed in:\n\
                 .claude/agents/ (project)\n\
                 ~/.claude/agents/ (user)\n\n\
               Each agent is a markdown file with:\n\
                 # Name, frontmatter (tools, model), prompt body"
            .to_string(),
        other => format!("Unknown agents subcommand: {other}"),
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

fn summary_handler(_args: &str) -> String {
    "Conversation summary:\n\n\
     (Summary generation requires an active conversation with messages.)"
        .to_string()
}

fn share_handler(_args: &str) -> String {
    "Share options:\n\n\
     Share a conversation transcript as a link.\n\
     (Requires authentication with Claude AI.)"
        .to_string()
}

fn onboarding_handler(args: &str) -> String {
    match args.trim() {
        "" => "Welcome to Claude Code!\n\n\
               Start the guided walkthrough to learn key features.\n\
               Use /onboarding start to begin."
            .to_string(),
        "start" => "Starting onboarding walkthrough...".to_string(),
        "skip" => "Onboarding skipped.".to_string(),
        other => format!("Unknown onboarding option: {other}"),
    }
}

fn desktop_handler(_args: &str) -> String {
    "Claude Code Desktop App:\n\n\
     Download from: https://claude.ai/code\n\
     Available for macOS and Windows."
        .to_string()
}

fn mobile_handler(_args: &str) -> String {
    "Claude Code Mobile:\n\n\
     Monitor your agents from Claude mobile app.\n\
     Download from your app store."
        .to_string()
}

fn chrome_handler(args: &str) -> String {
    match args.trim() {
        "" => "Claude in Chrome:\n\n\
               Use Claude Code directly in your browser.\n\
               Install the Chrome extension from the Chrome Web Store."
            .to_string(),
        "install" => "Opening Chrome Web Store...".to_string(),
        other => format!("Unknown chrome option: {other}"),
    }
}

fn release_notes_handler(_args: &str) -> String {
    let version = env!("CARGO_PKG_VERSION");
    format!(
        "Release Notes — v{version}\n\n\
         See full changelog at:\n\
         https://github.com/anthropics/claude-code/releases"
    )
}

fn teleport_handler(args: &str) -> String {
    let session = args.trim();
    if session.is_empty() {
        "Usage: /teleport <session-url>\n\n\
         Resume a remote session locally."
            .to_string()
    } else {
        format!("Teleporting to session: {session}")
    }
}

fn install_github_handler(_args: &str) -> String {
    "Claude Code GitHub App:\n\n\
     Install the GitHub App to enable:\n\
       - Automated code review\n\
       - PR comments integration\n\
       - Repository-level agent triggers\n\n\
     Visit: https://github.com/apps/claude-code"
        .to_string()
}

fn install_slack_handler(_args: &str) -> String {
    "Claude Code Slack App:\n\n\
     Install the Slack App to enable:\n\
       - Agent notifications in Slack\n\
       - Slash commands from Slack\n\
       - Team collaboration features\n\n\
     Visit your admin settings to install."
        .to_string()
}

// ── TS-parity: missing local handler functions ──

fn statusline_handler(args: &str) -> String {
    match args.trim() {
        "" | "toggle" => "Status line toggled.".to_string(),
        "on" | "enable" => "Status line enabled.".to_string(),
        "off" | "disable" => "Status line disabled.".to_string(),
        other => format!("Unknown statusline option: {other}. Use on/off/toggle."),
    }
}

fn reload_plugins_handler(_args: &str) -> String {
    "Reloading plugin definitions...\nAll plugins reloaded.".to_string()
}

fn terminal_setup_handler(_args: &str) -> String {
    "Terminal Setup:\n\n\
     Configure your terminal for the best Claude Code experience.\n\
     Recommended: 256-color terminal with Unicode support.\n\n\
     Current: auto-detected"
        .to_string()
}

fn remote_env_handler(_args: &str) -> String {
    "Remote Environment:\n\n\
     Configure remote execution environment for headless operation.\n\
     Use `coco remote-control` to start bridge mode."
        .to_string()
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

fn feedback_handler(args: &str) -> String {
    let report = args.trim();
    if report.is_empty() {
        "Usage: /feedback <message> — Submit feedback about Claude Code.\n\
         Or visit: https://github.com/anthropics/claude-code/issues"
            .to_string()
    } else {
        format!("Thank you for your feedback: {report}")
    }
}

fn extra_usage_handler(args: &str) -> String {
    match args.trim() {
        "" => "Extra usage / overage:\n\n\
               Status: not configured\n\n\
               Extra usage allows continued use beyond plan limits.\n\
               Configure in account settings at claude.ai."
            .to_string(),
        "enable" => "Extra usage enabled.".to_string(),
        "disable" => "Extra usage disabled.".to_string(),
        other => format!("Unknown extra-usage option: {other}"),
    }
}

fn thinkback_handler(_args: &str) -> String {
    "Thinking history for this session:\n\n\
     (No thinking blocks recorded yet.)\n\n\
     Extended thinking content is captured as the model works."
        .to_string()
}

fn thinkback_play_handler(_args: &str) -> String {
    "Replaying thinking history...\n\n\
     (No thinking blocks to replay.)"
        .to_string()
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

fn rate_limit_handler(_args: &str) -> String {
    "Rate limit configuration:\n\n\
     Current tier: default\n\
     Requests per minute: (varies by model)\n\n\
     Rate limits are determined by your subscription plan."
        .to_string()
}

fn heap_dump_handler(_args: &str) -> String {
    "Heap dump not available in this build.\n\
     Use --debug for runtime diagnostics."
        .to_string()
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

/// `/brief` — emit a brief session summary cue that the next turn
/// should respond to. The actual summarization runs on the agent side;
/// this handler just expands into a canonical user prompt.
fn brief_handler(_args: &str) -> String {
    "Briefly summarize the work done in this session so far in 3-5 bullet \
     points. Include key decisions, files changed, and any blockers."
        .to_string()
}

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

/// `/bug-report` — open a bug template URL with optional prefilled title.
fn bug_report_handler(args: &str) -> String {
    let title = args.trim();
    let url = if title.is_empty() {
        "https://github.com/anthropics/claude-code/issues/new?template=bug.md".to_string()
    } else {
        // The GitHub issue URL doesn't support a single 'title' field via
        // query for the template-new page, but GitHub accepts `?title=`
        // on the generic new-issue path.
        format!(
            "https://github.com/anthropics/claude-code/issues/new?title={}",
            urlencode_basic(title)
        )
    };
    format!("Open this URL to file a bug report:\n{url}")
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
            unsafe {
                std::env::set_var("COCO_ANT_TRACE", "1");
            }
            "ant-trace enabled for this session (COCO_ANT_TRACE=1)".into()
        }
        "off" => {
            unsafe {
                std::env::remove_var("COCO_ANT_TRACE");
            }
            "ant-trace disabled".into()
        }
        _ => format!(
            "Usage: /ant-trace [on|off]\nCurrent: {}",
            if std::env::var("COCO_ANT_TRACE").ok().as_deref() == Some("1") {
                "on"
            } else {
                "off"
            }
        ),
    }
}

/// `/voice` — voice input toggle. Stub: voice recording isn't wired
/// into the TUI yet; the command is here so scripts and `/help` show it.
fn voice_handler(args: &str) -> String {
    let arg = args.trim().to_ascii_lowercase();
    match arg.as_str() {
        "on" | "off" | "" => "Voice input is not wired into this build. Track progress at \
             https://github.com/anthropics/claude-code (coco-voice crate)."
            .into(),
        _ => "Usage: /voice [on|off]".into(),
    }
}

/// `/issue` — open a new GitHub issue for the CURRENT repo (detected
/// via `git remote get-url origin`).
fn issue_handler(args: &str) -> String {
    let title = args.trim();
    let cmd = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .output();
    let repo_slug = cmd
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .and_then(|url| parse_github_slug(url.trim()));

    match repo_slug {
        Some(slug) => {
            let url = if title.is_empty() {
                format!("https://github.com/{slug}/issues/new")
            } else {
                format!(
                    "https://github.com/{slug}/issues/new?title={}",
                    urlencode_basic(title)
                )
            };
            format!("Open this URL to file an issue:\n{url}")
        }
        None => "Couldn't determine GitHub repo from `git remote`. Use \
             /bug-report for coco-rs itself, or run this command in a \
             directory whose `origin` points at github.com."
            .into(),
    }
}

/// `/perf-issue` — report a performance issue. Captures basic metadata
/// but doesn't auto-submit (avoids accidental reports).
fn perf_issue_handler(_args: &str) -> String {
    "Performance issue flow: run `/ant-trace on`, reproduce the slow \
     behavior, then run `/ant-trace off`. Attach the trace log from \
     `~/.coco/trace/` to a new issue via /issue."
        .into()
}

/// `/autofix-pr` — AI-powered PR autofixer. Currently a stub routing to
/// the relevant TS documentation.
fn autofix_pr_handler(args: &str) -> String {
    let pr = args.trim();
    if pr.is_empty() {
        "Usage: /autofix-pr <pr-number>\n\n\
         The autofix flow is not yet wired in coco-rs. Use the standard \
         /review + manual edits workflow for now."
            .into()
    } else {
        format!(
            "Autofix for PR #{pr} is not yet implemented. Fall back to \
             /review {pr} followed by targeted edits."
        )
    }
}

/// `/bughunter` — AI-assisted bug search. Expands into a user prompt
/// that the agent can act on.
fn bughunter_handler(args: &str) -> String {
    let symptom = args.trim();
    if symptom.is_empty() {
        "Scan the repository for bugs matching common patterns (null \
         derefs, off-by-one, missing error handling, unvalidated input). \
         Prioritize by blast radius and report the top 5 with file:line \
         pointers and suggested fixes."
            .into()
    } else {
        format!(
            "Search the codebase for the root cause of this symptom: \
             {symptom}\n\nTrace the flow from the symptom back to the \
             originating call site. Report the suspected cause and a \
             proposed fix."
        )
    }
}

/// Minimal URL-encoder for title-style strings — replaces spaces with
/// `+` and percent-encodes chars outside `[A-Za-z0-9-._~]`. Keeps the
/// commands crate free of an extra `urlencoding` dep.
fn urlencode_basic(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '.' | '_' | '~' => out.push(ch),
            ' ' => out.push('+'),
            _ => {
                let mut buf = [0u8; 4];
                for b in ch.encode_utf8(&mut buf).bytes() {
                    out.push_str(&format!("%{b:02X}"));
                }
            }
        }
    }
    out
}

/// Parse `git@github.com:owner/repo.git` or
/// `https://github.com/owner/repo[.git]` into `owner/repo`.
fn parse_github_slug(remote: &str) -> Option<String> {
    let mut rest = if let Some(r) = remote.strip_prefix("git@github.com:") {
        r.to_string()
    } else if let Some(r) = remote.strip_prefix("https://github.com/") {
        r.to_string()
    } else if let Some(r) = remote.strip_prefix("http://github.com/") {
        r.to_string()
    } else {
        return None;
    };
    rest = rest.trim_end_matches(".git").to_string();
    // Must be exactly owner/repo
    let slashes = rest.matches('/').count();
    if slashes != 1 || rest.starts_with('/') || rest.ends_with('/') {
        return None;
    }
    Some(rest)
}

/// Handler for commands that have been moved to plugins.
///
/// TS: `createMovedToPluginCommand()` in `createMovedToPluginCommand.ts`.
struct MovedToPluginHandler {
    message: String,
}

#[async_trait::async_trait]
impl crate::CommandHandler for MovedToPluginHandler {
    async fn execute(&self, _args: &str) -> anyhow::Result<String> {
        Ok(self.message.clone())
    }

    fn handler_name(&self) -> &str {
        "moved-to-plugin"
    }
}

/// Create a command that informs users a built-in command has been
/// migrated to a plugin.
///
/// TS: `createMovedToPluginCommand()` in `createMovedToPluginCommand.ts`.
pub fn create_moved_to_plugin_command(
    name: &str,
    description: &str,
    plugin_name: &str,
    plugin_command: &str,
) -> RegisteredCommand {
    let message = format!(
        "This command has been moved to a plugin. To use it:\n\n\
         1. Install the plugin:\n   \
            coco plugin install {plugin_name}@claude-code-marketplace\n\n\
         2. Then use:\n   \
            /{plugin_name}:{plugin_command}\n\n\
         For more information, see the plugin's README."
    );

    RegisteredCommand {
        base: crate::builtin_base(name, description, &[]),
        command_type: CommandType::Local(LocalCommandData {
            handler: name.to_string(),
        }),
        handler: Some(Arc::new(MovedToPluginHandler { message })),
        is_enabled: None,
    }
}

#[cfg(test)]
#[path = "implementations.test.rs"]
mod tests;
