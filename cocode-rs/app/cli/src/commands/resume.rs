//! Resume command - resume a previous session.

use std::sync::Arc;

use cocode_config::ConfigManager;
use cocode_config::ConfigOverrides;
use cocode_session::SessionManager;
use cocode_session::persistence::session_file_path;

use crate::repl::Repl;

/// Resolve a session identifier (ID or name) to a concrete session ID.
///
/// Tries direct ID lookup first, then falls back to name-based search
/// across persisted sessions.
async fn resolve_session_id(id_or_name: &str) -> anyhow::Result<String> {
    let session_path = session_file_path(id_or_name);
    if session_path.exists() {
        return Ok(id_or_name.to_string());
    }

    // Fall back to name-based lookup
    let manager = SessionManager::new();
    let sessions = manager.list_persisted().await?;
    sessions
        .iter()
        .find(|s| s.name.as_deref() == Some(id_or_name))
        .map(|s| s.id.clone())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Session not found: {id_or_name}\n\nUse 'cocode sessions' to list available sessions."
            )
        })
}

/// Run the resume command.
///
/// Accepts either a session ID (UUID) or a session name for lookup.
pub async fn run(session_id: &str, config: &ConfigManager) -> anyhow::Result<()> {
    let resolved_id = resolve_session_id(session_id).await?;

    println!("Resuming session: {resolved_id}");
    println!();

    // Build Config snapshot from ConfigManager
    let snapshot = Arc::new(config.build_config(ConfigOverrides::default())?);

    let mut manager = SessionManager::new();

    // Load the session
    manager.load_session(&resolved_id, snapshot).await?;

    // Get the session state
    let state = manager
        .get_session(&resolved_id)
        .ok_or_else(|| anyhow::anyhow!("Failed to get session after loading"))?;

    println!("Model:    {}/{}", state.provider(), state.model());
    println!("Turns:    {}", state.total_turns());
    println!();

    // Start REPL with the resumed session
    let mut repl = Repl::new(state);
    repl.run().await?;

    // Save session on exit
    manager.save_session(&resolved_id).await?;

    Ok(())
}

/// Resume the most recent session (for `--continue` flag).
///
/// Finds the session with the latest activity timestamp and resumes it.
pub async fn run_most_recent(config: &ConfigManager) -> anyhow::Result<()> {
    let manager = SessionManager::new();
    let sessions = manager.list_persisted().await?;

    let most_recent = sessions.iter().max_by_key(|s| &s.last_activity_at);

    match most_recent {
        Some(s) => run(&s.id, config).await,
        None => Err(anyhow::anyhow!(
            "No sessions found to continue.\n\nStart a new session with 'cocode' first."
        )),
    }
}

/// Fork a session (for `--fork-session` flag).
///
/// Loads the specified session, assigns a new session ID (deep clone),
/// and starts a fresh REPL from the forked state.
pub async fn run_fork(session_id: &str, config: &ConfigManager) -> anyhow::Result<()> {
    let resolved_id = resolve_session_id(session_id).await?;

    println!("Forking session: {resolved_id}");

    let snapshot = Arc::new(config.build_config(ConfigOverrides::default())?);
    let mut manager = SessionManager::new();

    // Load the original session
    manager.load_session(&resolved_id, snapshot).await?;

    // Fork: generate new ID, keep conversation history
    let fork_id = manager.fork_session(&resolved_id).await?;

    println!("New session: {fork_id}");
    println!();

    let state = manager
        .get_session(&fork_id)
        .ok_or_else(|| anyhow::anyhow!("Failed to get forked session"))?;

    let mut repl = Repl::new(state);
    repl.run().await?;

    manager.save_session(&fork_id).await?;

    Ok(())
}

#[cfg(test)]
#[path = "resume.test.rs"]
mod tests;
