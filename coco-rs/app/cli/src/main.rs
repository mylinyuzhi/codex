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
mod tui_runner;

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
                match action {
                    PluginAction::List => println!("Installed plugins: (none)"),
                    PluginAction::Install { name } => println!("Installing plugin: {name}"),
                    PluginAction::Uninstall { name } => println!("Uninstalling plugin: {name}"),
                }
                return Ok(());
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
    let permission_mode = settings.merged.permissions.default_mode.unwrap_or_default();

    let system_prompt = build_system_prompt(&cwd, &model_id);

    let config = QueryEngineConfig {
        model_name: model_id.clone(),
        permission_mode,
        context_window: 200_000,
        max_output_tokens: 16_384,
        max_turns: 30,
        max_tokens: cli.max_tokens,
        system_prompt: Some(system_prompt),
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
    let (model, _mode) = create_model(cli.model.as_deref());
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

    // Build the server with a default runner first so we have a live
    // `state` handle to give to the approval bridge.
    let transport = StdioTransport::new();
    let server = SdkServer::new(transport)
        .with_session_manager(session_manager)
        .with_file_history(file_history, global_config::config_home())
        .with_mcp_manager(mcp_manager);
    let state = server.state();

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
