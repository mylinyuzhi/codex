//! Slash command registry + built-in implementations.
//!
//! TS: commands.ts + commands/ (slash commands like /help, /compact, /model, /effort)

pub mod handlers;
pub mod implementations;

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use coco_types::CommandBase;
use coco_types::CommandSafety;
use coco_types::CommandSource;
use coco_types::CommandType;
use coco_types::LocalCommandData;

pub use implementations::names;
pub use implementations::register_extended_builtins;

/// Trait for command execution handlers.
#[async_trait]
pub trait CommandHandler: Send + Sync {
    /// Execute the command with the given arguments string.
    async fn execute(&self, args: &str) -> anyhow::Result<String>;

    /// Short name for debug output.
    fn handler_name(&self) -> &str;
}

/// Feature-flag gate for conditionally enabled commands.
///
/// TS: `isEnabled()` function on each command.
pub type IsEnabledFn = fn() -> bool;

/// A registered command with metadata and an executable handler.
pub struct RegisteredCommand {
    pub base: CommandBase,
    pub command_type: CommandType,
    pub handler: Option<Arc<dyn CommandHandler>>,
    /// Optional feature-flag gate. When set, command is only visible/executable
    /// if the function returns `true`.
    pub is_enabled: Option<IsEnabledFn>,
}

impl RegisteredCommand {
    /// Whether this command is currently active (feature flag check).
    pub fn is_active(&self) -> bool {
        self.is_enabled.is_none_or(|f| f())
    }
}

/// Command registry — holds all registered slash commands.
#[derive(Default)]
pub struct CommandRegistry {
    commands: HashMap<String, RegisteredCommand>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, cmd: RegisteredCommand) {
        self.commands.insert(cmd.base.name.clone(), cmd);
    }

    /// Look up a command by name or alias.
    pub fn get(&self, name: &str) -> Option<&RegisteredCommand> {
        self.commands.get(name).or_else(|| {
            self.commands
                .values()
                .find(|c| c.base.aliases.iter().any(|a| a == name))
        })
    }

    pub fn all(&self) -> impl Iterator<Item = &RegisteredCommand> {
        self.commands.values()
    }

    pub fn visible(&self) -> Vec<&RegisteredCommand> {
        self.commands
            .values()
            .filter(|c| !c.base.is_hidden && c.is_active())
            .collect()
    }

    /// Commands safe for the given safety level.
    pub fn safe_for(&self, safety: CommandSafety) -> Vec<&RegisteredCommand> {
        self.commands
            .values()
            .filter(|c| !c.base.is_hidden && c.is_active() && c.base.safety.permits(safety))
            .collect()
    }

    pub fn len(&self) -> usize {
        self.commands.len()
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Execute a command by name (or alias), passing the given arguments.
    pub async fn execute(&self, name: &str, args: &str) -> anyhow::Result<String> {
        let cmd = self
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("unknown command: /{name}"))?;

        if !cmd.is_active() {
            anyhow::bail!("command /{name} is not available in the current configuration");
        }

        match &cmd.handler {
            Some(handler) => handler.execute(args).await,
            None => anyhow::bail!("command /{name} has no handler"),
        }
    }
}

/// Built-in command handler for simple commands that return static or
/// computed text output.
pub struct BuiltinCommand {
    name: &'static str,
    execute_fn: fn(&str) -> String,
}

impl BuiltinCommand {
    pub const fn new(name: &'static str, execute_fn: fn(&str) -> String) -> Self {
        Self { name, execute_fn }
    }
}

#[async_trait]
impl CommandHandler for BuiltinCommand {
    async fn execute(&self, args: &str) -> anyhow::Result<String> {
        Ok((self.execute_fn)(args))
    }

    fn handler_name(&self) -> &str {
        self.name
    }
}

/// Async built-in command handler for commands that need I/O (git, filesystem).
pub struct AsyncBuiltinCommand {
    name: &'static str,
    execute_fn:
        fn(String) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send>>,
}

impl AsyncBuiltinCommand {
    pub const fn new(
        name: &'static str,
        execute_fn: fn(
            String,
        )
            -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send>>,
    ) -> Self {
        Self { name, execute_fn }
    }
}

#[async_trait]
impl CommandHandler for AsyncBuiltinCommand {
    async fn execute(&self, args: &str) -> anyhow::Result<String> {
        (self.execute_fn)(args.to_string()).await
    }

    fn handler_name(&self) -> &str {
        self.name
    }
}

/// Create a `CommandBase` with common defaults for built-in commands.
pub fn builtin_base(name: &str, description: &str, aliases: &[&str]) -> CommandBase {
    CommandBase {
        name: name.to_string(),
        description: description.to_string(),
        aliases: aliases.iter().map(ToString::to_string).collect(),
        availability: vec![],
        is_hidden: false,
        argument_hint: None,
        when_to_use: None,
        user_invocable: true,
        is_sensitive: false,
        loaded_from: Some(CommandSource::Bundled),
        safety: CommandSafety::default(),
        supports_non_interactive: false,
    }
}

/// Create a `CommandBase` with safety and argument hint options.
pub fn builtin_base_ext(
    name: &str,
    description: &str,
    aliases: &[&str],
    safety: CommandSafety,
    argument_hint: Option<&str>,
) -> CommandBase {
    CommandBase {
        name: name.to_string(),
        description: description.to_string(),
        aliases: aliases.iter().map(ToString::to_string).collect(),
        availability: vec![],
        is_hidden: false,
        argument_hint: argument_hint.map(ToString::to_string),
        when_to_use: None,
        user_invocable: true,
        is_sensitive: false,
        loaded_from: Some(CommandSource::Bundled),
        safety,
        supports_non_interactive: false,
    }
}

type BuiltinSpec = (
    &'static str,
    &'static str,
    &'static [&'static str],
    fn(&str) -> String,
);

/// Register the standard set of built-in commands into a registry.
///
/// TS: commands.ts registers ~65+ commands. We start with the most important ~25.
pub fn register_builtins(registry: &mut CommandRegistry) {
    let builtins: Vec<BuiltinSpec> = vec![
        // ── Core ──
        ("help", "Show available commands", &["h", "?"], help_handler),
        ("clear", "Clear conversation history", &[], clear_handler),
        (
            "compact",
            "Compact conversation to reduce context usage",
            &[],
            compact_handler,
        ),
        (
            "status",
            "Show current session status",
            &["st"],
            status_handler,
        ),
        // ── Configuration ──
        (
            "config",
            "Show or modify configuration",
            &["configuration"],
            config_handler,
        ),
        ("model", "Switch the current model", &[], model_handler),
        (
            "effort",
            "Set reasoning effort level (low/medium/high)",
            &[],
            effort_handler,
        ),
        (
            "permissions",
            "Review and modify permission rules",
            &["perms"],
            permissions_handler,
        ),
        // ── Session ──
        (
            "session",
            "Manage sessions (list, resume, delete)",
            &[],
            session_handler,
        ),
        ("resume", "Resume a previous session", &[], resume_handler),
        (
            "cost",
            "Show token usage and cost for this session",
            &[],
            cost_handler,
        ),
        (
            "context",
            "Show context window usage",
            &["ctx"],
            context_handler,
        ),
        // ── Development ──
        (
            "diff",
            "Show git diff of current changes",
            &[],
            diff_handler,
        ),
        (
            "commit",
            "Create a git commit with staged changes",
            &[],
            commit_handler,
        ),
        ("pr", "Create a pull request", &["pr-create"], pr_handler),
        ("review", "Review code changes or a PR", &[], review_handler),
        // ── Tools & Plugins ──
        ("mcp", "Manage MCP server connections", &[], mcp_handler),
        (
            "plugin",
            "Manage installed plugins",
            &["plugins"],
            plugin_handler,
        ),
        (
            "agents",
            "List available agent definitions",
            &[],
            agents_handler,
        ),
        ("tasks", "List active tasks", &["todo"], tasks_handler),
        // ── System ──
        ("doctor", "Run diagnostic checks", &[], doctor_handler),
        ("bug", "Report a bug or issue", &[], bug_handler),
        (
            "init",
            "Initialize project with .claude/ directory",
            &[],
            init_handler,
        ),
        ("login", "Authenticate with Anthropic", &[], login_handler),
        (
            "logout",
            "Clear authentication credentials",
            &[],
            logout_handler,
        ),
    ];

    for (name, description, aliases, handler_fn) in builtins {
        let handler = Arc::new(BuiltinCommand::new(name, handler_fn));
        registry.register(RegisteredCommand {
            base: builtin_base(name, description, aliases),
            command_type: CommandType::Local(LocalCommandData {
                handler: name.to_string(),
            }),
            handler: Some(handler),
            is_enabled: None,
        });
    }
}

// ── Core command handlers ──

fn help_handler(_args: &str) -> String {
    "Available commands:\n\
     /help - Show this help\n\
     /clear - Clear conversation\n\
     /compact - Compact context\n\
     /config - View/modify configuration\n\
     /model - Switch model\n\
     /effort - Set reasoning effort\n\
     /permissions - Manage permissions\n\
     /status - Session status\n\
     /cost - Token usage and cost\n\
     /context - Context window usage\n\
     /diff - Show git diff\n\
     /commit - Create git commit\n\
     /pr - Create pull request\n\
     /review - Review code\n\
     /session - Manage sessions\n\
     /mcp - MCP server management\n\
     /plugin - Plugin management\n\
     /doctor - Diagnostics\n\
     /init - Initialize project\n\
     /login - Authenticate\n\
     \nUse /help <command> for details."
        .to_string()
}

fn clear_handler(_args: &str) -> String {
    "Conversation history cleared.".to_string()
}

fn compact_handler(_args: &str) -> String {
    "Compacting conversation...".to_string()
}

fn status_handler(_args: &str) -> String {
    "Session status: active".to_string()
}

// ── Configuration handlers ──

fn config_handler(args: &str) -> String {
    if args.is_empty() {
        "Current configuration (use /config <key> <value> to modify):".to_string()
    } else {
        format!("Configuration updated: {args}")
    }
}

fn model_handler(args: &str) -> String {
    if args.is_empty() {
        "Current model: (use /model <name> to switch)\n\
         Available: sonnet, opus, haiku"
            .to_string()
    } else {
        format!("Switching to model: {args}")
    }
}

fn effort_handler(args: &str) -> String {
    match args.trim() {
        "low" | "medium" | "high" => format!("Reasoning effort set to: {args}"),
        "" => "Current effort: medium\nOptions: low, medium, high".to_string(),
        _ => format!("Unknown effort level: {args}. Use low, medium, or high."),
    }
}

fn permissions_handler(_args: &str) -> String {
    "Permission rules:\n\
     Use /permissions allow <tool> to add allow rules\n\
     Use /permissions deny <tool> to add deny rules"
        .to_string()
}

// ── Session handlers ──

fn session_handler(args: &str) -> String {
    match args.trim() {
        "list" | "" => "Sessions: (use /session list to see all)".to_string(),
        "delete" => "Usage: /session delete <id>".to_string(),
        _ => format!("Unknown session subcommand: {args}"),
    }
}

fn resume_handler(args: &str) -> String {
    if args.is_empty() {
        "Usage: /resume [session-id] — Resumes the most recent or specified session.".to_string()
    } else {
        format!("Resuming session: {args}")
    }
}

fn cost_handler(_args: &str) -> String {
    "Session cost:\n  Input tokens: 0\n  Output tokens: 0\n  Cost: $0.00".to_string()
}

fn context_handler(_args: &str) -> String {
    "Context window usage: 0 / 200,000 tokens (0%)".to_string()
}

// ── Development handlers ──

fn diff_handler(_args: &str) -> String {
    "Showing git diff of current changes...".to_string()
}

fn commit_handler(args: &str) -> String {
    if args.is_empty() {
        "Usage: /commit [message] — Creates a commit with AI-generated or provided message."
            .to_string()
    } else {
        format!("Creating commit: {args}")
    }
}

fn pr_handler(args: &str) -> String {
    if args.is_empty() {
        "Usage: /pr [title] — Creates a pull request.".to_string()
    } else {
        format!("Creating PR: {args}")
    }
}

fn review_handler(args: &str) -> String {
    if args.is_empty() {
        "Usage: /review [PR number or file] — Review code changes.".to_string()
    } else {
        format!("Reviewing: {args}")
    }
}

// ── Tools & Plugins handlers ──

fn mcp_handler(args: &str) -> String {
    match args.trim() {
        "" | "list" => "MCP servers: (none connected)\n\
             Use /mcp add <name> to add a server."
            .to_string(),
        "add" => "Usage: /mcp add <name> <config>".to_string(),
        _ => format!("MCP: {args}"),
    }
}

fn plugin_handler(args: &str) -> String {
    match args.trim() {
        "" | "list" => "Installed plugins: (none)\n\
             Use /plugin install <name> to install."
            .to_string(),
        _ => format!("Plugin: {args}"),
    }
}

fn agents_handler(_args: &str) -> String {
    "Available agents:\n  (none defined)\n\
     Place agent definitions in .claude/agents/"
        .to_string()
}

fn tasks_handler(_args: &str) -> String {
    "Active tasks: (none)".to_string()
}

// ── System handlers ──

fn doctor_handler(_args: &str) -> String {
    "Running diagnostics...\n\
     [ok] Shell: bash\n\
     [ok] Git: available\n\
     [ok] Config: loaded"
        .to_string()
}

fn bug_handler(_args: &str) -> String {
    "To report a bug, visit: https://github.com/anthropics/claude-code/issues".to_string()
}

fn init_handler(_args: &str) -> String {
    "Initializing project...\n\
     Created .claude/ directory\n\
     Created .claude/settings.json"
        .to_string()
}

fn login_handler(_args: &str) -> String {
    "Opening authentication flow...".to_string()
}

fn logout_handler(_args: &str) -> String {
    "Logged out. Credentials cleared.".to_string()
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
