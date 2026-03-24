//! Chat command - start an interactive chat session.

use std::path::PathBuf;
use std::sync::Arc;

use cocode_config::ConfigManager;
use cocode_config::ConfigOverrides;
use cocode_session::Session;
use cocode_session::SessionState;
use tracing::info;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;

use crate::CliFlags;
use crate::output;
use crate::repl::Repl;

/// Initialize stderr logging for REPL mode.
///
/// If logging is already initialized, this will do nothing and return None.
fn init_repl_logging(config: &ConfigManager, verbose: bool) -> Option<()> {
    // Get logging config
    let logging_config = config.logging_config();
    let common_logging = logging_config
        .map(|c| c.to_common_logging())
        .unwrap_or_default();

    // Override level if verbose flag is set
    let effective_logging = if verbose {
        cocode_utils_common::LoggingConfig {
            level: "info,cocode=debug".to_string(),
            ..common_logging
        }
    } else {
        common_logging
    };

    // Build stderr layer (timezone is handled inside the macro via ConfigurableTimer)
    let stderr_layer = cocode_utils_common::configure_fmt_layer!(
        fmt::layer().with_writer(std::io::stderr).compact(),
        &effective_logging,
        "warn"
    );

    match tracing_subscriber::registry().with(stderr_layer).try_init() {
        Ok(()) => Some(()),
        Err(_) => None, // Already initialized
    }
}

/// Run the chat command in REPL mode.
pub async fn run(
    initial_prompt: Option<String>,
    title: Option<String>,
    name: Option<String>,
    max_turns: Option<i32>,
    config: &ConfigManager,
    flags: CliFlags,
) -> anyhow::Result<()> {
    // Initialize logging for REPL mode (stderr)
    let _ = init_repl_logging(config, flags.verbose);

    info!("Starting REPL mode");

    // Get working directory
    let working_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    // Build Config snapshot from ConfigManager
    let snapshot =
        Arc::new(config.build_config(ConfigOverrides::default().with_cwd(working_dir.clone()))?);

    // Fire-and-forget cleanup of expired sessions (>30 days old)
    {
        let sessions_dir = cocode_session::persistence::default_sessions_dir();
        tokio::spawn(async move {
            let mgr = cocode_session::SessionManager::with_storage_dir(sessions_dir);
            if let Err(e) = mgr.cleanup_expired_sessions(30).await {
                tracing::debug!("Session cleanup failed: {e}");
            }
        });
    }

    // Build all role selections from config
    let mut selections = config.build_all_selections();
    flags.apply_model_overrides(config, &mut selections)?;

    let mut session = Session::with_selections(working_dir, selections);

    if let Some(t) = title {
        session.set_title(t);
    }
    if let Some(n) = name {
        session.name = Some(n);
    }
    if let Some(max) = max_turns {
        session.set_max_turns(Some(max));
    }
    if let Some(usd) = flags.max_budget_usd {
        session.set_max_budget_cents(Some((usd * 100.0).round() as i32));
    }

    // Create session state from config snapshot
    let mut state = SessionState::new(session, snapshot).await?;

    // Apply CLI flags to session state
    apply_cli_flags_to_state(&mut state, &flags).await;

    // Handle initial prompt (non-interactive mode)
    if let Some(prompt) = initial_prompt {
        let result = state.run_turn(&prompt).await?;
        println!("{}", result.final_text);
        output::print_turn_summary(result.usage.input_tokens, result.usage.output_tokens);
        return Ok(());
    }

    // Interactive mode - run REPL
    let mut repl = Repl::new(&mut state);
    repl.run().await?;

    // Save session on exit (if not ephemeral)
    if !state.session.ephemeral {
        let session_id = state.session.id.clone();
        let path = cocode_session::persistence::session_file_path(&session_id);
        let snapshots = if let Some(sm) = state.snapshot_manager() {
            let json = sm.serialize_snapshots().await?;
            serde_json::from_str(&json)?
        } else {
            Vec::new()
        };
        cocode_session::save_session_to_file(&state.session, state.history(), snapshots, &path)
            .await?;
        println!("Session saved: {session_id}");
    }

    Ok(())
}

/// Apply CLI flags to a `SessionState`.
///
/// Sets permission mode, system prompt suffix, and registers CLI agents.
/// Note: env vars are set separately by `set_cli_env_vars()` in main.rs
/// during single-threaded init, before any async tasks spawn.
pub async fn apply_cli_flags_to_state(state: &mut SessionState, flags: &CliFlags) {
    if let Some(ref suffix) = flags.system_prompt_suffix {
        state.set_system_prompt_suffix(suffix.clone());
    }

    if let Some(mode) = flags.permission_mode {
        state.set_permission_mode(mode);
    }

    if !flags.cli_agents.is_empty() {
        let mut mgr = state.subagent_manager().lock().await;
        for agent in &flags.cli_agents {
            mgr.register_agent_type(agent.clone());
        }
    }
}

#[cfg(test)]
#[path = "chat.test.rs"]
mod tests;
