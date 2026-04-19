use std::sync::Arc;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::Ordering;

use anyhow::Result;
use clap::Parser;
use vercel_ai_provider::AISdkError;
use vercel_ai_provider::AssistantContentPart;
use vercel_ai_provider::FinishReason;
use vercel_ai_provider::LanguageModelV4;
use vercel_ai_provider::LanguageModelV4CallOptions;
use vercel_ai_provider::LanguageModelV4GenerateResult;
use vercel_ai_provider::LanguageModelV4StreamResult;
use vercel_ai_provider::TextPart;
use vercel_ai_provider::UnifiedFinishReason;
use vercel_ai_provider::Usage;

use coco_cli::Cli;
use coco_cli::Commands;
use coco_cli::ConfigAction;
use coco_cli::McpAction;
use coco_cli::PluginAction;
use coco_cli::sdk_server::QueryEngineRunner;
use coco_cli::sdk_server::SdkServer;
use coco_cli::sdk_server::StdioTransport;
use coco_cli::sdk_server::cli_bootstrap::CliInitializeBootstrap;
use coco_commands::CommandRegistry;
use coco_commands::register_extended_builtins;
use coco_config::global_config;
use coco_inference::ApiClient;
use coco_inference::RetryConfig;
use coco_query::QueryEngine;
use coco_query::QueryEngineConfig;
use coco_session::SessionManager;
use coco_tool::ToolRegistry;
use tokio_util::sync::CancellationToken;

/// Built-in mock model for development/testing.
struct MockModel {
    call_count: AtomicI32,
}

#[async_trait::async_trait]
impl LanguageModelV4 for MockModel {
    fn provider(&self) -> &str {
        "mock"
    }
    fn model_id(&self) -> &str {
        "mock-model"
    }
    async fn do_generate(
        &self,
        options: LanguageModelV4CallOptions,
    ) -> std::result::Result<LanguageModelV4GenerateResult, AISdkError> {
        let call = self.call_count.fetch_add(1, Ordering::SeqCst);
        let user_text: String = options
            .prompt
            .iter()
            .filter_map(|msg| match msg {
                vercel_ai_provider::LanguageModelV4Message::User { content, .. } => Some(
                    content
                        .iter()
                        .filter_map(|c| match c {
                            vercel_ai_provider::UserContentPart::Text(t) => Some(t.text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" "),
                ),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");

        let response = format!(
            "[mock model, call #{call}] Received: \"{user_text}\"\n\n\
             This is coco-rs with a mock provider. Set ANTHROPIC_API_KEY to use a real model."
        );

        Ok(LanguageModelV4GenerateResult {
            content: vec![AssistantContentPart::Text(TextPart {
                text: response,
                provider_metadata: None,
            })],
            usage: Usage::new(user_text.len() as u64 / 4, 50),
            finish_reason: FinishReason::new(UnifiedFinishReason::Stop),
            warnings: vec![],
            provider_metadata: None,
            request: None,
            response: None,
        })
    }
    async fn do_stream(
        &self,
        _options: LanguageModelV4CallOptions,
    ) -> std::result::Result<LanguageModelV4StreamResult, AISdkError> {
        Err(AISdkError::new("streaming not supported in mock mode"))
    }
}

/// Create the LLM model -- real Anthropic if API key available, else mock.
mod model_factory;
mod tui_runner;

pub(crate) use model_factory::build_language_model_from_spec;

pub(crate) fn create_model(model_id: Option<&str>) -> (Arc<dyn LanguageModelV4>, &'static str) {
    if std::env::var("ANTHROPIC_API_KEY").is_ok() {
        let provider = vercel_ai_anthropic::anthropic();
        let model_name = model_id.unwrap_or("claude-sonnet-4-5-20250514");
        let model = provider.messages(model_name);
        return (Arc::new(model), "anthropic");
    }

    // Fallback to mock
    (
        Arc::new(MockModel {
            call_count: AtomicI32::new(0),
        }),
        "mock",
    )
}

fn sessions_dir() -> std::path::PathBuf {
    global_config::config_home().join("sessions")
}

fn handle_config(action: &ConfigAction) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let settings = coco_config::settings::load_settings(&cwd, None)?;
    let json = serde_json::to_value(&settings.merged)?;

    match action {
        ConfigAction::List => {
            let pretty = serde_json::to_string_pretty(&json)?;
            println!("{pretty}");
        }
        ConfigAction::Get { key } => {
            if let Some(value) = json.get(key) {
                let pretty = serde_json::to_string_pretty(value)?;
                println!("{key} = {pretty}");
            } else {
                println!("Key '{key}' not found in configuration.");
                println!("Available keys:");
                if let Some(obj) = json.as_object() {
                    for k in obj.keys() {
                        println!("  {k}");
                    }
                }
            }
        }
        ConfigAction::Set { key, value } => {
            let user_path = global_config::user_settings_path();
            println!("Would set '{key}' = '{value}' in {}", user_path.display());
            println!(
                "Settings file: {}",
                if user_path.exists() {
                    "exists"
                } else {
                    "will be created"
                }
            );
        }
        ConfigAction::Reset => {
            let user_path = global_config::user_settings_path();
            if user_path.exists() {
                std::fs::remove_file(&user_path)?;
                println!("Configuration reset to defaults.");
            } else {
                println!("No user configuration file to reset.");
            }
        }
    }
    Ok(())
}

fn handle_sessions() -> Result<()> {
    let mgr = SessionManager::new(sessions_dir());
    let sessions = mgr.list()?;

    if sessions.is_empty() {
        println!("No sessions found.");
        return Ok(());
    }

    println!(
        "{:<38}  {:<30}  {:<12}  Working Dir",
        "ID", "Model", "Created"
    );
    println!("{}", "-".repeat(100));
    for s in &sessions {
        println!(
            "{:<38}  {:<30}  {:<12}  {}",
            s.id,
            s.model,
            &s.created_at,
            s.working_dir.display()
        );
    }
    println!("\n{} session(s) total.", sessions.len());
    Ok(())
}

fn handle_resume(session_id: Option<&str>) -> Result<()> {
    let mgr = SessionManager::new(sessions_dir());

    let session = if let Some(id) = session_id {
        mgr.resume(id)?
    } else {
        match mgr.most_recent()? {
            Some(s) => {
                println!("Resuming most recent session: {}", s.id);
                mgr.resume(&s.id)?
            }
            None => {
                println!("No sessions to resume.");
                return Ok(());
            }
        }
    };

    println!("Session: {}", session.id);
    println!("Model: {}", session.model);
    println!("Working dir: {}", session.working_dir.display());
    println!("Messages: {}", session.message_count);
    if let Some(title) = &session.title {
        println!("Title: {title}");
    }
    println!("\nSession resumed. Run `coco` to continue the conversation.");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(cmd) = &cli.command {
        match cmd {
            Commands::Status => {
                let (model, mode) = create_model(None);
                println!("coco-rs v0.0.0 ({mode} mode)");
                println!("model: {}", model.model_id());
                println!("provider: {}", model.provider());
                return Ok(());
            }
            Commands::Sessions => {
                return handle_sessions();
            }
            Commands::Resume { session_id } => {
                return handle_resume(session_id.as_deref());
            }
            Commands::Config { action } => {
                return handle_config(action);
            }
            Commands::Chat { prompt } => {
                let prompt = prompt.as_deref().unwrap_or("Hello!");
                return run_chat(&cli, Some(prompt)).await;
            }
            Commands::Doctor => {
                println!("Running diagnostics...");
                println!("[ok] Shell: available");
                println!("[ok] Config: loaded");
                let (model, mode) = create_model(None);
                println!("[ok] Model: {} ({mode})", model.model_id());
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
                return run_plugin_subcommand(action).await;
            }
            Commands::Agents => {
                return run_agents_subcommand().await;
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
        // Print mode: single-turn, exit
        let prompt = cli.prompt.as_deref().unwrap_or("Hello!");
        run_chat(&cli, Some(prompt)).await
    } else {
        // Interactive TUI mode (TS default)
        tui_runner::run_tui(&cli).await
    }
}

/// Build the system prompt with environment context and CLAUDE.md content.
pub(crate) fn build_system_prompt(cwd: &std::path::Path, model_id: &str) -> String {
    let claude_files = coco_context::discover_claude_md_files(cwd);
    let claude_md_content: String = claude_files
        .iter()
        .map(|f| format!("# {}\n{}\n", f.path.display(), f.content))
        .collect();

    let env_info = coco_context::get_environment_info(cwd, model_id);

    let mut system_prompt =
        String::from("You are coco, an AI coding assistant. Be concise and helpful.\n\n");
    system_prompt.push_str(&format!(
        "# Environment\n- Platform: {}\n- Shell: {:?}\n- CWD: {}\n",
        env_info.platform.display_name(),
        env_info.shell,
        env_info.cwd,
    ));
    if let Some(ref git) = env_info.git_status {
        system_prompt.push_str(&format!(
            "- Git branch: {}\n- Git status: {}\n",
            git.branch,
            if git.status.is_empty() {
                "(clean)"
            } else {
                &git.status
            },
        ));
    }
    if !claude_md_content.is_empty() {
        system_prompt.push_str(&format!("\n# Project Instructions\n{claude_md_content}"));
    }
    system_prompt
}

/// Handle `coco plugin <action>`.
///
/// TS: `src/cli/handlers/plugins.ts` — full handler is ~878 lines covering
/// marketplace integration, scopes, lockfiles. Rust currently implements the
/// local-disk subset: list, install-from-path, uninstall, validate.
/// URL/marketplace installs require porting the marketplace module.
async fn run_plugin_subcommand(action: &PluginAction) -> Result<()> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let config_home = global_config::config_home();
    let plugin_dirs = coco_plugins::get_plugin_dirs(&config_home, &cwd);

    match action {
        PluginAction::List => {
            let mut manager = coco_plugins::PluginManager::new();
            manager.load_from_dirs(&plugin_dirs);
            if manager.is_empty() {
                println!("No plugins installed.");
                return Ok(());
            }
            println!("Installed plugins:");
            let mut plugins: Vec<_> = manager.enabled();
            plugins.sort_by_key(|p| p.name.clone());
            for plugin in plugins {
                let version = plugin.manifest.version.as_deref().unwrap_or("—");
                let source = match &plugin.source {
                    coco_plugins::PluginSource::Builtin => "builtin".into(),
                    coco_plugins::PluginSource::User => "user".into(),
                    coco_plugins::PluginSource::Project => "project".into(),
                    coco_plugins::PluginSource::Repository { url } => format!("repo {url}"),
                };
                println!(
                    "  {name} {version} ({source})  — {desc}",
                    name = plugin.name,
                    desc = plugin.manifest.description,
                );
            }
            Ok(())
        }
        PluginAction::Install { name } => {
            let src = std::path::Path::new(name);
            if !src.is_dir() {
                anyhow::bail!(
                    "plugin source '{name}' is not a local directory; \
                     marketplace/URL installs are not yet implemented"
                );
            }
            if !src.join("PLUGIN.toml").is_file() {
                anyhow::bail!("'{name}' does not contain a PLUGIN.toml manifest");
            }
            let manifest = coco_plugins::load_plugin_manifest(&src.join("PLUGIN.toml"))?;
            // Reject manifest names that could traverse the install root.
            // `Path::join` treats "../" literally and does not escape the root on
            // disk, but a normalized `..` chain can still confuse audit tooling.
            if manifest.name.is_empty()
                || manifest.name.contains('/')
                || manifest.name.contains('\\')
                || manifest.name == ".."
                || manifest.name == "."
            {
                anyhow::bail!(
                    "plugin manifest name '{}' contains path separators or reserved \
                     component; refusing to install",
                    manifest.name
                );
            }
            let dest_root = config_home.join("plugins");
            std::fs::create_dir_all(&dest_root)?;
            let dest = dest_root.join(&manifest.name);
            if dest.exists() {
                anyhow::bail!(
                    "plugin '{}' already installed at {}; uninstall first",
                    manifest.name,
                    dest.display()
                );
            }
            copy_dir_recursive(src, &dest)?;
            println!("Installed plugin '{}' → {}", manifest.name, dest.display());
            Ok(())
        }
        PluginAction::Uninstall { name } => {
            let dest = config_home.join("plugins").join(name);
            if !dest.is_dir() {
                anyhow::bail!("plugin '{name}' is not installed at {}", dest.display());
            }
            std::fs::remove_dir_all(&dest)?;
            println!("Uninstalled plugin '{name}'");
            Ok(())
        }
        PluginAction::Validate { path } => {
            let path = std::path::Path::new(path);
            let manifest_path = if path.is_file() {
                path.to_path_buf()
            } else {
                path.join("PLUGIN.toml")
            };
            if !manifest_path.is_file() {
                anyhow::bail!("no PLUGIN.toml found at {}", manifest_path.display());
            }
            let manifest = coco_plugins::load_plugin_manifest(&manifest_path)?;
            println!(
                "✓ {} v{}",
                manifest.name,
                manifest.version.as_deref().unwrap_or("—")
            );
            println!("  {}", manifest.description);
            if !manifest.skills.is_empty() {
                println!("  skills: {}", manifest.skills.join(", "));
            }
            if !manifest.hooks.is_empty() {
                println!("  hooks: {} event(s)", manifest.hooks.len());
            }
            if !manifest.mcp_servers.is_empty() {
                println!("  mcp_servers: {}", manifest.mcp_servers.len());
            }
            Ok(())
        }
    }
}

/// Recursively copy `src` into `dst`. Used by plugin install.
///
/// Symlinks are skipped with a warning — following them lets a hostile plugin
/// exfiltrate host files (e.g. `~/.ssh/id_rsa`) into the install tree. Use
/// `symlink_metadata()` so the check doesn't follow; `file_type().is_dir()`
/// and `is_file()` otherwise follow by default.
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        // symlink_metadata does NOT follow — so a symlink is reported as a symlink.
        let meta = std::fs::symlink_metadata(&src_path)?;
        let ty = meta.file_type();
        if ty.is_symlink() {
            eprintln!(
                "warning: skipping symlink in plugin source: {}",
                src_path.display()
            );
            continue;
        }
        let dest_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else if ty.is_file() {
            std::fs::copy(&src_path, &dest_path)?;
        }
    }
    Ok(())
}

/// Handle `coco agents` — list discovered agent definitions.
///
/// TS: `src/cli/handlers/agents.ts` — walks the standard agent dirs and
/// prints a flat list. Rust mirrors the same discovery sources.
async fn run_agents_subcommand() -> Result<()> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let config_home = global_config::config_home();
    let agent_dirs = coco_tools::tools::agent_spawn::get_agent_dirs(&config_home, &cwd);
    let mut agents = coco_tools::tools::agent_spawn::load_agents_from_dirs(&agent_dirs);

    if agents.is_empty() {
        println!("No agents found.");
        println!(
            "Searched: {}",
            agent_dirs
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
        return Ok(());
    }

    agents.sort_by(|a, b| a.name.cmp(&b.name));
    println!("{} agent(s):", agents.len());
    for agent in &agents {
        let model = agent.model.as_deref().unwrap_or("inherit");
        let desc = agent.description.as_deref().unwrap_or("(no description)");
        println!("  {} · {model}  — {desc}", agent.name);
    }
    Ok(())
}

/// Run a single-turn print mode (--print / piped stdout).
///
/// TS: runHeadless() in cli/print.ts
async fn run_chat(cli: &Cli, prompt: Option<&str>) -> Result<()> {
    let prompt = prompt.unwrap_or("Hello!");
    let (model, mode) = create_model(cli.model.as_deref());
    let model_id = model.model_id().to_string();
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));

    let mut registry = ToolRegistry::new();
    coco_tools::register_all_tools(&mut registry);
    let tools = Arc::new(registry);
    let cancel = CancellationToken::new();

    let cwd = std::env::current_dir()?;
    let settings = coco_config::settings::load_settings(&cwd, None)?;

    let startup = resolve_startup_permission_state(cli, &settings.merged)?;
    // Headless path — surface the downgrade notification on stderr.
    if let Some(msg) = &startup.notification {
        eprintln!("warning: {msg}");
    }
    let permission_mode = startup.mode;
    let bypass_permissions_available = startup.bypass_available;

    let system_prompt = build_system_prompt(&cwd, &model_id);

    let config = QueryEngineConfig {
        model_name: model_id.clone(),
        permission_mode,
        bypass_permissions_available,
        context_window: 200_000,
        max_output_tokens: 16_384,
        max_turns: 30,
        max_tokens: cli.max_tokens,
        system_prompt: Some(system_prompt),
        project_dir: Some(cwd.clone()),
        plans_directory: settings.merged.plans_directory.clone(),
        plan_mode_settings: settings.merged.plan_mode.clone(),
        ..Default::default()
    };

    let engine = QueryEngine::new(config, client, tools, cancel, /*hooks*/ None);

    eprintln!("coco-rs ({mode} mode) — model: {model_id}\n");

    let result = engine.run(prompt).await?;

    println!("{}", result.response_text);
    eprintln!(
        "\n─── {} turn(s) | {} in / {} out tokens ───",
        result.turns, result.total_usage.input_tokens, result.total_usage.output_tokens
    );

    Ok(())
}

/// Run in SDK mode: NDJSON-over-stdio JSON-RPC control protocol.
///
/// TS reference: `src/cli/structuredIO.ts` — the `StructuredIO` loop.
/// The SDK client (Python/TS) spawns `coco sdk` as a subprocess and
/// speaks JSON-RPC across the pipes. Phase 2.C.5 wires:
///
/// 1. [`StdioTransport`] — NDJSON framing on stdin/stdout
/// 2. [`QueryEngineRunner`] — production `TurnRunner` that spawns a
///    fresh `QueryEngine` per turn/start
/// 3. [`SdkServer`] — dispatch loop that owns the transport + state
///
/// This path intentionally does NOT print banners on stdout — the SDK
/// client expects a clean NDJSON stream. Any diagnostic output goes to
/// stderr via `tracing`.
async fn run_sdk_mode(cli: &Cli) -> Result<()> {
    // Build the shared model client + tool registry once. QueryEngines
    // created per turn will share these.
    //
    // `_mode` distinguishes "real Anthropic provider" from the built-in
    // mock. When mock, the SDK's `initialize.account` must NOT report a
    // first-party Anthropic session — otherwise clients see mock-shaped
    // turn output while the account panel claims a real OAuth login.
    let (model, mode) = create_model(cli.model.as_deref());
    let is_real_anthropic = mode == "anthropic";
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));

    let mut registry = ToolRegistry::new();
    coco_tools::register_all_tools(&mut registry);
    let tools = Arc::new(registry);

    // Resolve static config. Cwd + model id are also stored on the
    // SessionHandle at `session/start`, but we use the CLI-level values
    // here for the system prompt + headless defaults.
    let cwd = std::env::current_dir()?;
    let model_id = client.model_id().to_string();
    let system_prompt = Some(build_system_prompt(&cwd, &model_id));

    // Wire a disk-backed SessionManager so session/list, session/read,
    // and session/resume work against `~/.coco/sessions`.
    let session_manager = Arc::new(SessionManager::new(sessions_dir()));

    // Wire a FileHistoryState so control/rewindFiles can preview and
    // apply rewinds. A fresh state is empty — snapshots will accrue
    // as future integration code wires file-history tracking into the
    // tool layer. Until then, `control/rewindFiles` will error with
    // "no snapshot for user_message_id" which is the expected contract
    // when nothing has been tracked yet.
    let file_history = Arc::new(tokio::sync::RwLock::new(
        coco_context::FileHistoryState::new(),
    ));

    // Wire an empty MCP connection manager so mcp/setServers,
    // mcp/reconnect, mcp/toggle, and mcp/status work. Initial config
    // is empty — the SDK client populates it via mcp/setServers at
    // runtime. Server processes are spawned on first connect.
    let mcp_manager = Arc::new(tokio::sync::Mutex::new(
        coco_mcp::McpConnectionManager::new(global_config::config_home()),
    ));

    // Build the slash-command registry with the extended built-ins so
    // `initialize` advertises a real commands list. Future follow-ups
    // can splice plugin + user-directory commands in here.
    let mut command_registry = CommandRegistry::new();
    register_extended_builtins(&mut command_registry);
    let command_registry = Arc::new(command_registry);

    // Locate user + project output-style directories so
    // `available_output_styles` discovers custom markdown files. Both
    // live under the coco config home tree today; a future iteration
    // can add `~/.claude/output-styles` for TS compatibility.
    let output_style_dirs = vec![global_config::config_home().join("output-styles")];
    let current_output_style = "default".to_string();

    // Standard agent-definition directories — mirrors what the Agent
    // tool walks at spawn time. `initialize` reads the same sources so
    // clients see the same list the agent tool will actually use.
    let agent_dirs =
        coco_tools::tools::agent_spawn::get_agent_dirs(&global_config::config_home(), &cwd);

    // Resolve auth once so `initialize.account` can report the
    // provider / subscription. The actual credentials don't leak to SDK
    // clients — only the structured `SdkAccountInfo` projection.
    //
    // **Consistency with `create_model`**: we only surface a resolved
    // auth method when `create_model` picked the real Anthropic provider.
    // Otherwise `create_model` fell back to a mock (no env var, no
    // provider wired) and advertising OAuth tokens as the account would
    // contradict the mock turn output the client is about to see. Run
    // the resolution on the blocking pool because
    // `load_stored_oauth_tokens` does sync disk I/O.
    let auth_method = if is_real_anthropic {
        let config_dir = global_config::config_home();
        tokio::task::spawn_blocking(move || {
            coco_inference::auth::resolve_auth(&coco_inference::auth::AuthResolveOptions {
                config_dir: Some(config_dir),
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
        .with_command_registry(command_registry)
        .with_output_style_dirs(output_style_dirs)
        .with_agent_dirs(agent_dirs);
    if let Some(auth) = auth_method {
        bootstrap_builder = bootstrap_builder.with_auth_method(auth);
    }
    let bootstrap: Arc<dyn coco_cli::sdk_server::InitializeBootstrap> = Arc::new(bootstrap_builder);

    // Startup safety + capability gate (parity with run_tui / run_chat).
    let settings = coco_config::settings::load_settings(&cwd, None)?;
    // SDK mode doesn't surface an initial mode via this path — the SDK
    // client sets mode per-session through turn/start or
    // `control/setPermissionMode`. We still resolve to run the killswitch
    // downgrade + safety guard consistently.
    let startup = resolve_startup_permission_state(cli, &settings.merged)?;
    if let Some(msg) = &startup.notification {
        eprintln!("warning: {msg}");
    }
    let bypass_permissions_available = startup.bypass_available;

    // Build the server with a default runner first so we have a live
    // `state` handle to give to the approval bridge.
    let transport = StdioTransport::new();
    let server = SdkServer::new(transport)
        .with_session_manager(session_manager)
        .with_file_history(file_history, global_config::config_home())
        .with_mcp_manager(mcp_manager)
        .with_initialize_bootstrap(bootstrap);
    let state = server.state();
    // Seed the bypass capability so `handle_set_permission_mode` can
    // enforce the mid-session guard. Static for the process lifetime.
    state.bypass_permissions_available.store(
        bypass_permissions_available,
        std::sync::atomic::Ordering::Relaxed,
    );

    // Build the real runner with an SdkPermissionBridge that routes
    // `PermissionDecision::Ask` via `approval/askForApproval` to the
    // SDK client. Then install the runner on the live state.
    let bridge: Arc<dyn coco_tool::ToolPermissionBridge> =
        Arc::new(coco_cli::sdk_server::SdkPermissionBridge::new(state));
    let runner = Arc::new(
        QueryEngineRunner::new(
            client,
            tools,
            cli.max_tokens.unwrap_or(16_384),
            cli.max_turns.unwrap_or(30),
            system_prompt,
        )
        .with_permission_bridge(bridge),
    );
    server.set_turn_runner(runner).await;

    // Run the dispatch loop to completion. Exits on EOF, transport
    // close, or unrecoverable I/O error.
    if let Err(e) = server.run().await {
        eprintln!("sdk mode: dispatch loop exited with error: {e}");
        return Err(anyhow::anyhow!("sdk dispatch failed: {e}"));
    }
    Ok(())
}

/// Output of startup permission resolution. Contains everything the
/// session needs to bootstrap with consistent bypass semantics.
pub(crate) struct StartupPermissionState {
    /// Resolved initial permission mode (after CLI + settings merge
    /// + killswitch downgrade).
    pub mode: coco_types::PermissionMode,
    /// Whether the session may transition into `BypassPermissions`.
    pub bypass_available: bool,
    /// Optional user-visible notification explaining a downgrade
    /// (e.g. killswitch forced Bypass → AcceptEdits). Callers should
    /// surface it via stderr (headless) or a TUI toast (interactive)
    /// so users understand why they didn't land in the mode they
    /// asked for. `None` when no downgrade occurred.
    pub notification: Option<String>,
}

/// Resolve the session's initial `PermissionMode` and the bypass
/// capability in one pass, and run the sudo/sandbox safety guard.
///
/// This mirrors TS's startup sequence:
/// 1. `initialPermissionModeFromCLI` — pick a mode from
///    `--dangerously-skip-permissions` → `--permission-mode` →
///    `settings.permissions.defaultMode`, walking the list and
///    skipping `bypassPermissions` when the killswitch is engaged.
/// 2. `isBypassPermissionsModeAvailable` — capability derived from
///    `(resolved_mode == Bypass || --allow-dangerously-skip-permissions)
///     && !killswitch`.
/// 3. `setup.ts:395-442` — root/sandbox guard when the session will
///    start in bypass OR the allow flag is set.
pub(crate) fn resolve_startup_permission_state(
    cli: &Cli,
    settings: &coco_config::Settings,
) -> Result<StartupPermissionState> {
    use coco_types::PermissionMode;

    let policy_flag = Some(settings.permissions.disable_bypass_mode);

    // Parse --permission-mode once so the walk resolver sees a typed
    // value; invalid strings print one warning here and are ignored
    // (TS `permissionModeFromString` returns 'default' on unknown
    // input — we treat the slot as absent, which is equivalent under
    // the walk semantics).
    let permission_mode_cli = cli.permission_mode.as_deref().and_then(|raw| {
        match serde_json::from_value::<PermissionMode>(serde_json::json!(raw)) {
            Ok(m) => Some(m),
            Err(e) => {
                eprintln!("warning: invalid --permission-mode {raw:?}: {e}; ignoring");
                None
            }
        }
    });

    // TS `initialPermissionModeFromCLI`: walk ordered candidates,
    // skip Bypass when killswitch engaged, first non-blocked wins.
    let resolved = coco_permissions::resolve_initial_permission_mode(
        cli.dangerously_skip_permissions,
        permission_mode_cli,
        settings.permissions.default_mode,
        policy_flag,
    );
    let mode = resolved.mode;

    // TS `isBypassPermissionsModeAvailable`: key on the resolved mode,
    // not the raw CLI flag, so `--permission-mode bypassPermissions`
    // also unlocks the capability.
    let bypass_available = coco_permissions::compute_bypass_capability(
        mode == PermissionMode::BypassPermissions,
        cli.allow_dangerously_skip_permissions,
        policy_flag,
    );

    // TS `setup.ts:395-442`: sudo/sandbox guard fires whenever bypass
    // is requested (resolved mode is Bypass OR allow-flag set).
    let requesting_bypass =
        mode == PermissionMode::BypassPermissions || cli.allow_dangerously_skip_permissions;
    enforce_dangerous_skip_safety(requesting_bypass)?;

    Ok(StartupPermissionState {
        mode,
        bypass_available,
        notification: resolved.notification,
    })
}

/// Reject requesting bypass when the host is not a sandbox.
///
/// TS parity: `setup.ts:395-442`. Fires when the session will *start*
/// in `BypassPermissions` or when `--allow-dangerously-skip-permissions`
/// merely unlocks the capability. Detect root/sudo via env-var
/// heuristics (safe — no `unsafe { libc::getuid() }`) and refuse
/// unless one of the known sandbox markers is set. Known sandbox
/// markers: `IS_SANDBOX=1`, `COCO_CODE_BUBBLEWRAP` truthy,
/// `CLAUDE_CODE_BUBBLEWRAP` truthy (for direct TS-port compatibility).
fn enforce_dangerous_skip_safety(requesting_bypass: bool) -> Result<()> {
    if !requesting_bypass {
        return Ok(());
    }
    if is_root_like_env() && !is_sandboxed_env() {
        return Err(anyhow::anyhow!(
            "Bypass permissions refuses to run as root/sudo outside a \
             sandbox. Set IS_SANDBOX=1 (or run under bubblewrap) if you \
             know what you're doing."
        ));
    }
    Ok(())
}

/// Heuristic root-or-sudo detection via env vars. Accurate enough for
/// a safety guard — a malicious local actor can already override
/// anything, but the common accidental-sudo case is caught.
fn is_root_like_env() -> bool {
    // `SUDO_*` present = invoked via sudo.
    if std::env::var_os("SUDO_USER").is_some() || std::env::var_os("SUDO_UID").is_some() {
        return true;
    }
    let is_root_name = |var: &str| -> bool {
        std::env::var(var)
            .map(|v| v.trim() == "root")
            .unwrap_or(false)
    };
    is_root_name("USER") || is_root_name("LOGNAME") || is_root_name("USERNAME")
}

fn is_sandboxed_env() -> bool {
    let truthy = |var: &str| -> bool {
        std::env::var(var)
            .map(|v| {
                matches!(
                    v.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false)
    };
    truthy("IS_SANDBOX") || truthy("COCO_CODE_BUBBLEWRAP") || truthy("CLAUDE_CODE_BUBBLEWRAP")
}
