//! Multi-session management.
//!
//! [`SessionManager`] handles creating, tracking, and persisting multiple sessions.

use std::collections::HashMap;
use std::path::PathBuf;

use std::sync::Arc;

use cocode_config::Config;
use cocode_protocol::ModelSpec;
use cocode_protocol::ProviderApi;
use cocode_protocol::RoleSelection;
use cocode_protocol::RoleSelections;
use tokio::fs;
use tracing::debug;
use tracing::info;
use tracing::warn;

use crate::persistence::default_sessions_dir;
use crate::persistence::delete_session_file;
use crate::persistence::load_session_from_file;
use crate::persistence::save_session_to_file;
use crate::persistence::session_exists;
use crate::persistence::session_file_path;
use crate::session::Session;
use crate::state::SessionState;

/// Summary information about a session for listing.
#[derive(Debug, Clone)]
pub struct SessionSummary {
    /// Session ID.
    pub id: String,
    /// Session title (if set).
    pub title: Option<String>,
    /// Session name for resume-by-name (if set).
    pub name: Option<String>,
    /// Model used.
    pub model: String,
    /// Provider used.
    pub provider: String,
    /// Creation timestamp (ISO 8601).
    pub created_at: String,
    /// Last activity timestamp (ISO 8601).
    pub last_activity_at: String,
    /// Number of turns completed.
    pub turn_count: i32,
}

/// Multi-session manager for creating, tracking, and persisting sessions.
///
/// The manager maintains a map of active sessions and handles persistence
/// to the `~/.cocode/sessions/` directory.
///
/// # Example
///
/// ```ignore
/// use cocode_session::SessionManager;
/// use cocode_config::{ConfigManager, ConfigOverrides};
/// use cocode_protocol::{ProviderApi, ModelSpec, RoleSelection};
/// use std::sync::Arc;
/// use std::path::PathBuf;
///
/// let manager_cfg = ConfigManager::from_default()?;
/// let config = Arc::new(manager_cfg.build_config(ConfigOverrides::default())?);
/// let mut manager = SessionManager::new();
///
/// // Create a new session
/// let session_id = manager.create_session(
///     PathBuf::from("."),
///     "gpt-5",
///     ProviderApi::Openai,
///     config.clone(),
/// ).await?;
///
/// // Get the session
/// if let Some(state) = manager.get_session(&session_id) {
///     let result = state.run_turn("Hello!").await?;
/// }
///
/// // Save the session
/// manager.save_session(&session_id).await?;
/// ```
pub struct SessionManager {
    /// Active sessions (session_id -> SessionState).
    sessions: HashMap<String, SessionState>,

    /// Directory for session storage.
    storage_dir: PathBuf,
}

impl SessionManager {
    /// Create a new session manager with the default storage directory.
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            storage_dir: default_sessions_dir(),
        }
    }

    /// Create a new session manager with a custom storage directory.
    pub fn with_storage_dir(storage_dir: PathBuf) -> Self {
        Self {
            sessions: HashMap::new(),
            storage_dir,
        }
    }

    /// Create a new session and add it to the manager.
    ///
    /// Returns the session ID.
    pub async fn create_session(
        &mut self,
        working_dir: PathBuf,
        model: &str,
        api: ProviderApi,
        config: Arc<Config>,
    ) -> anyhow::Result<String> {
        // Create ModelSpec with the provider API's name and the model
        let provider_name = api.to_string();
        let spec = ModelSpec::with_type(&provider_name, api, model);
        let selection = RoleSelection::new(spec);
        let session = Session::new(working_dir, selection);
        let session_id = session.id.clone();

        info!(
            session_id = %session_id,
            model = model,
            provider = %api,
            "Creating new session"
        );

        let state = SessionState::new(session, config).await?;
        self.sessions.insert(session_id.clone(), state);

        Ok(session_id)
    }

    /// Create a session with a specific provider name.
    pub async fn create_session_with_provider(
        &mut self,
        working_dir: PathBuf,
        model: &str,
        provider: &str,
        config: Arc<Config>,
    ) -> anyhow::Result<String> {
        // Resolve provider API from Config snapshot
        let api = config
            .provider_api(provider)
            .ok_or_else(|| anyhow::anyhow!("Provider '{provider}' not found in config"))?;

        // Create ModelSpec with explicit provider API
        let spec = ModelSpec::with_type(provider, api, model);
        let selection = RoleSelection::new(spec);
        let session = Session::new(working_dir, selection);

        let session_id = session.id.clone();

        info!(
            session_id = %session_id,
            model = model,
            provider = provider,
            "Creating new session with provider"
        );

        let state = SessionState::new(session, config).await?;
        self.sessions.insert(session_id.clone(), state);

        Ok(session_id)
    }

    /// Create a session with full role selections.
    ///
    /// Use this when you have pre-configured role selections
    /// (e.g., from configuration with multiple models per role).
    pub async fn create_session_with_selections(
        &mut self,
        working_dir: PathBuf,
        selections: RoleSelections,
        config: Arc<Config>,
    ) -> anyhow::Result<String> {
        let session = Session::with_selections(working_dir, selections);
        let session_id = session.id.clone();

        info!(
            session_id = %session_id,
            model = ?session.model(),
            provider = ?session.provider(),
            "Creating new session with selections"
        );

        let state = SessionState::new(session, config).await?;
        self.sessions.insert(session_id.clone(), state);

        Ok(session_id)
    }

    /// Get a session by ID.
    pub fn get_session(&mut self, id: &str) -> Option<&mut SessionState> {
        self.sessions.get_mut(id)
    }

    /// Check if a session exists.
    pub fn has_session(&self, id: &str) -> bool {
        self.sessions.contains_key(id)
    }

    /// Remove a session from the manager (does not delete the file).
    pub fn remove_session(&mut self, id: &str) -> Option<SessionState> {
        self.sessions.remove(id)
    }

    /// List all active sessions.
    pub fn list_active(&self) -> Vec<SessionSummary> {
        self.sessions
            .values()
            .map(|state| SessionSummary {
                id: state.session.id.clone(),
                title: state.session.title.clone(),
                name: state.session.name.clone(),
                model: state.session.model().unwrap_or("").to_string(),
                provider: state.session.provider().unwrap_or("").to_string(),
                created_at: state.session.created_at.to_rfc3339(),
                last_activity_at: state.session.last_activity_at.to_rfc3339(),
                turn_count: state.total_turns(),
            })
            .collect()
    }

    /// List all persisted sessions from the storage directory.
    pub async fn list_persisted(&self) -> anyhow::Result<Vec<SessionSummary>> {
        let mut summaries = Vec::new();

        // Ensure directory exists
        if !self.storage_dir.exists() {
            return Ok(summaries);
        }

        let mut entries = fs::read_dir(&self.storage_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                match load_session_from_file(&path).await {
                    Ok((session, _history, _snapshots)) => {
                        // Extract model/provider before moving session fields
                        let model = session.model().unwrap_or("").to_string();
                        let provider = session.provider().unwrap_or("").to_string();
                        summaries.push(SessionSummary {
                            id: session.id,
                            title: session.title,
                            name: session.name,
                            model,
                            provider,
                            created_at: session.created_at.to_rfc3339(),
                            last_activity_at: session.last_activity_at.to_rfc3339(),
                            turn_count: 0, // Not tracked in persisted session
                        });
                    }
                    Err(e) => {
                        warn!(path = %path.display(), error = %e, "Failed to load session");
                    }
                }
            }
        }

        // Sort by last activity (most recent first)
        summaries.sort_by(|a, b| b.last_activity_at.cmp(&a.last_activity_at));

        Ok(summaries)
    }

    /// Save a session to disk (including snapshot stack for rewind support).
    pub async fn save_session(&self, id: &str) -> anyhow::Result<()> {
        let state = self
            .sessions
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("Session not found: {id}"))?;

        if state.session.ephemeral {
            debug!(session_id = id, "Skipping save for ephemeral session");
            return Ok(());
        }

        // Serialize snapshot stack (empty vec if no snapshot manager).
        let snapshots = if let Some(sm) = state.snapshot_manager() {
            let json = sm.serialize_snapshots().await?;
            serde_json::from_str(&json)?
        } else {
            Vec::new()
        };

        let path = self.storage_dir.join(format!("{id}.json"));
        save_session_to_file(&state.session, &state.message_history, snapshots, &path).await
    }

    /// Save all active sessions.
    pub async fn save_all(&self) -> anyhow::Result<()> {
        for id in self.sessions.keys() {
            if let Err(e) = self.save_session(id).await {
                warn!(session_id = id, error = %e, "Failed to save session");
            }
        }
        Ok(())
    }

    /// Load a session from disk (including snapshot stack for rewind support).
    pub async fn load_session(&mut self, id: &str, config: Arc<Config>) -> anyhow::Result<()> {
        let path = self.storage_dir.join(format!("{id}.json"));

        if !session_exists(&path).await {
            return Err(anyhow::anyhow!(
                "Session file not found: {}",
                path.display()
            ));
        }

        let (session, history, snapshots) = load_session_from_file(&path).await?;

        info!(
            session_id = %session.id,
            model = ?session.model(),
            snapshots = snapshots.len(),
            "Loading session"
        );

        let mut state = SessionState::new(session, config).await?;
        // Restore the message history
        state.message_history = history;

        // Restore snapshot stack for rewind support
        if !snapshots.is_empty()
            && let Some(sm) = state.snapshot_manager()
        {
            let json = serde_json::to_string(&snapshots)?;
            if let Err(e) = sm.restore_snapshots(&json).await {
                tracing::warn!("Failed to restore snapshots: {e}");
            }
        }

        self.sessions.insert(id.to_string(), state);

        Ok(())
    }

    /// Delete a session from disk.
    pub async fn delete_session(&mut self, id: &str) -> anyhow::Result<()> {
        // Remove from active sessions
        self.sessions.remove(id);

        // Delete file if exists
        let path = session_file_path(id);
        if session_exists(&path).await {
            delete_session_file(&path).await?;
        }

        Ok(())
    }

    /// Remove sessions whose last activity is older than `retention_days`.
    ///
    /// Deletes both the session JSON file and the associated file-backup
    /// directory. Runs best-effort: individual failures are logged and skipped.
    pub async fn cleanup_expired_sessions(&self, retention_days: i64) -> anyhow::Result<u32> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(retention_days);
        let mut cleaned = 0u32;

        if !self.storage_dir.exists() {
            return Ok(0);
        }

        let mut entries = fs::read_dir(&self.storage_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }

            match load_session_from_file(&path).await {
                Ok((session, _, _)) => {
                    if session.last_activity_at < cutoff {
                        info!(
                            session_id = %session.id,
                            last_activity = %session.last_activity_at,
                            "Cleaning up expired session"
                        );
                        // Delete session file
                        if let Err(e) = fs::remove_file(&path).await {
                            warn!(path = %path.display(), "Failed to delete session file: {e}");
                            continue;
                        }
                        // Delete backup directory
                        let backup_dir = self.storage_dir.join(&session.id);
                        if backup_dir.exists()
                            && let Err(e) = fs::remove_dir_all(&backup_dir).await
                        {
                            warn!(path = %backup_dir.display(), "Failed to delete backup dir: {e}");
                        }
                        cleaned += 1;
                    }
                }
                Err(e) => {
                    warn!(path = %path.display(), "Failed to load session for cleanup: {e}");
                }
            }
        }

        if cleaned > 0 {
            info!(cleaned, "Expired sessions cleaned up");
        }
        Ok(cleaned)
    }

    /// Get the number of active sessions.
    pub fn active_count(&self) -> usize {
        self.sessions.len()
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "manager.test.rs"]
mod tests;
