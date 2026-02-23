//! Session persistence for saving and loading sessions to/from files.
//!
//! Sessions are stored as JSON files in the `~/.cocode/sessions/` directory.

use std::path::Path;

use cocode_file_backup::TurnSnapshot;
use cocode_message::MessageHistory;
use serde::Deserialize;
use serde::Serialize;
use tokio::fs;
use tracing::debug;
use tracing::info;

use crate::session::Session;

/// Persisted session data.
#[derive(Debug, Serialize, Deserialize)]
pub struct PersistedSession {
    /// Session metadata.
    pub session: Session,

    /// Message history.
    pub history: MessageHistory,

    /// Snapshot stack for rewind support.
    ///
    /// Persisted so that rewind works across session resume. When absent
    /// (e.g., files saved by older versions), defaults to an empty stack.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub snapshots: Vec<TurnSnapshot>,

    /// File format version for future compatibility.
    #[serde(default = "default_version")]
    pub version: i32,
}

fn default_version() -> i32 {
    1
}

impl PersistedSession {
    /// Create a new persisted session.
    pub fn new(session: Session, history: MessageHistory, snapshots: Vec<TurnSnapshot>) -> Self {
        Self {
            session,
            history,
            snapshots,
            version: 1,
        }
    }
}

/// Save a session, its history, and snapshot stack to a JSON file.
pub async fn save_session_to_file(
    session: &Session,
    history: &MessageHistory,
    snapshots: Vec<TurnSnapshot>,
    path: &Path,
) -> anyhow::Result<()> {
    info!(
        session_id = %session.id,
        path = %path.display(),
        "Saving session"
    );

    // Create parent directory if needed
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }

    let persisted = PersistedSession::new(session.clone(), history.clone(), snapshots);
    let json = serde_json::to_string_pretty(&persisted)?;

    fs::write(path, json).await?;

    debug!(
        session_id = %session.id,
        bytes = persisted.session.id.len(),
        "Session saved"
    );

    Ok(())
}

/// Load a session, its history, and snapshot stack from a JSON file.
pub async fn load_session_from_file(
    path: &Path,
) -> anyhow::Result<(Session, MessageHistory, Vec<TurnSnapshot>)> {
    info!(path = %path.display(), "Loading session");

    let content = fs::read_to_string(path).await?;
    let persisted: PersistedSession = serde_json::from_str(&content)?;

    debug!(
        session_id = %persisted.session.id,
        version = persisted.version,
        snapshots = persisted.snapshots.len(),
        "Session loaded"
    );

    Ok((persisted.session, persisted.history, persisted.snapshots))
}

/// Check if a session file exists.
pub async fn session_exists(path: &Path) -> bool {
    fs::metadata(path).await.is_ok()
}

/// Delete a session file.
pub async fn delete_session_file(path: &Path) -> anyhow::Result<()> {
    info!(path = %path.display(), "Deleting session file");
    fs::remove_file(path).await?;
    Ok(())
}

/// Get the default sessions directory.
pub fn default_sessions_dir() -> std::path::PathBuf {
    cocode_config::find_cocode_home().join("sessions")
}

/// Get the path for a session file by ID.
pub fn session_file_path(session_id: &str) -> std::path::PathBuf {
    default_sessions_dir().join(format!("{session_id}.json"))
}

#[cfg(test)]
#[path = "persistence.test.rs"]
mod tests;
