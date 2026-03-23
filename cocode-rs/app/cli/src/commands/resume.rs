//! Resume command - resume a previous session.

use std::sync::Arc;

use cocode_config::ConfigManager;
use cocode_config::ConfigOverrides;
use cocode_session::SessionManager;
use cocode_session::persistence::session_file_path;

use crate::repl::Repl;

/// Run the resume command.
///
/// Accepts either a session ID (UUID) or a session name for lookup.
pub async fn run(session_id: &str, config: &ConfigManager) -> anyhow::Result<()> {
    // Try direct ID lookup first
    let session_path = session_file_path(session_id);
    let resolved_id = if session_path.exists() {
        session_id.to_string()
    } else {
        // Fall back to name-based lookup
        let manager = SessionManager::new();
        let sessions = manager.list_persisted().await?;
        let matched = sessions
            .iter()
            .find(|s| s.name.as_deref() == Some(session_id));
        match matched {
            Some(s) => s.id.clone(),
            None => {
                return Err(anyhow::anyhow!(
                    "Session not found: {session_id}\n\nUse 'cocode sessions' to list available sessions."
                ));
            }
        }
    };

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

#[cfg(test)]
#[path = "resume.test.rs"]
mod tests;
