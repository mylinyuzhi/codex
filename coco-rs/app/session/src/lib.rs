//! Session persistence, history, and state aggregation.
//!
//! TS: bootstrap/state.ts + session management + history.ts

pub mod error;
pub mod history;
pub mod recovery;
pub mod storage;
pub mod title_generator;

pub use error::SessionError;
pub use history::HistoryEntry;
pub use history::PromptHistory;
pub use storage::AgentMetadata;
pub use storage::ContentReplacementRecord;
pub use storage::Entry;
pub use storage::MetadataEntry;
pub use storage::ModelCostEntry;
pub use storage::RestoredCostSummary;
pub use storage::TranscriptEntry;
pub use storage::TranscriptMetadata;
pub use storage::TranscriptStore;
pub use storage::TranscriptUsage;
pub use storage::build_file_history_snapshot_chain;
pub use storage::restore_cost_from_transcript;

use serde::Deserialize;
use serde::Serialize;
use std::path::Path;
use std::path::PathBuf;

/// Crate-local Result alias. Default error type is `SessionError` but the
/// generic stays open so `Result::ok` / 2-arg `Result<T, E>` callsites
/// still resolve against `std::result::Result`.
pub type Result<T, E = SessionError> = std::result::Result<T, E>;

/// A session record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    pub model: String,
    pub working_dir: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Number of messages in the session.
    #[serde(default)]
    pub message_count: i32,
    /// Total tokens used.
    #[serde(default)]
    pub total_tokens: i64,
    /// Searchable tags applied via `/tag`. Persisted alongside title for
    /// session browsing/filtering. TS: session metadata `tags?: string[]`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

/// Session manager — create, load, save, list, resume sessions.
pub struct SessionManager {
    pub sessions_dir: PathBuf,
}

impl SessionManager {
    pub fn new(sessions_dir: PathBuf) -> Self {
        Self { sessions_dir }
    }

    /// Create a new session.
    pub fn create(&self, model: &str, cwd: &Path) -> crate::Result<Session> {
        let id = uuid::Uuid::new_v4().to_string();
        let session = Session {
            id,
            created_at: timestamp_now(),
            updated_at: None,
            model: model.to_string(),
            working_dir: cwd.to_path_buf(),
            title: None,
            message_count: 0,
            total_tokens: 0,
            tags: Vec::new(),
        };
        self.save(&session)?;
        Ok(session)
    }

    /// Set the session title (`/rename <name>`). Loads the session,
    /// updates `title`, bumps `updated_at`, and writes it back. Errors
    /// when the session id isn't on disk.
    pub fn set_title(&self, id: &str, title: &str) -> crate::Result<Session> {
        let mut session = self.load(id)?;
        session.title = Some(title.to_string());
        session.updated_at = Some(timestamp_now());
        self.save(&session)?;
        Ok(session)
    }

    /// Toggle a tag on/off (`/tag <name>`). If the tag is present, it's
    /// removed; otherwise appended. Mirrors the TS toggle semantics
    /// where re-running `/tag X` removes a previously-added X.
    pub fn toggle_tag(&self, id: &str, tag: &str) -> crate::Result<(Session, bool)> {
        let mut session = self.load(id)?;
        let added = if let Some(idx) = session.tags.iter().position(|t| t == tag) {
            session.tags.remove(idx);
            false
        } else {
            session.tags.push(tag.to_string());
            true
        };
        session.updated_at = Some(timestamp_now());
        self.save(&session)?;
        Ok((session, added))
    }

    /// Save/update a session.
    pub fn save(&self, session: &Session) -> crate::Result<()> {
        std::fs::create_dir_all(&self.sessions_dir)?;
        let session_file = self.sessions_dir.join(format!("{}.json", session.id));
        let json = serde_json::to_string_pretty(session)?;
        std::fs::write(session_file, json)?;
        Ok(())
    }

    /// Load a session by ID.
    pub fn load(&self, id: &str) -> crate::Result<Session> {
        let session_file = self.sessions_dir.join(format!("{id}.json"));
        let content = std::fs::read_to_string(&session_file)?;
        let session: Session = serde_json::from_str(&content)?;
        Ok(session)
    }

    /// Resume a session — loads it and updates the timestamp.
    pub fn resume(&self, id: &str) -> crate::Result<Session> {
        let mut session = self.load(id)?;
        session.updated_at = Some(timestamp_now());
        self.save(&session)?;
        Ok(session)
    }

    /// List all sessions, newest first.
    pub fn list(&self) -> crate::Result<Vec<Session>> {
        let mut sessions = Vec::new();
        if !self.sessions_dir.exists() {
            return Ok(sessions);
        }
        for entry in std::fs::read_dir(&self.sessions_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json")
                && let Ok(content) = std::fs::read_to_string(&path)
                && let Ok(session) = serde_json::from_str::<Session>(&content)
            {
                sessions.push(session);
            }
        }
        sessions.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(sessions)
    }

    /// Delete a session.
    pub fn delete(&self, id: &str) -> crate::Result<()> {
        let session_file = self.sessions_dir.join(format!("{id}.json"));
        match std::fs::remove_file(session_file) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// Get the most recent session.
    pub fn most_recent(&self) -> crate::Result<Option<Session>> {
        let sessions = self.list()?;
        Ok(sessions.into_iter().next())
    }

    /// Clean up old sessions beyond a limit.
    pub fn cleanup(&self, keep_count: usize) -> crate::Result<i32> {
        let sessions = self.list()?;
        let mut removed = 0;
        for session in sessions.iter().skip(keep_count) {
            self.delete(&session.id)?;
            removed += 1;
        }
        Ok(removed)
    }

    /// TS-aligned mtime-based retention: delete every session file whose
    /// on-disk mtime is older than `older_than`. Mirrors TS
    /// `utils/cleanup.ts` behavior (`DEFAULT_CLEANUP_PERIOD_DAYS = 30`).
    ///
    /// Walks the sessions dir directly (stat-only, no JSON parsing) so a
    /// corrupt session file doesn't prevent cleanup.
    ///
    /// Returns the number of sessions removed.
    pub fn cleanup_older_than(&self, older_than: std::time::Duration) -> crate::Result<i32> {
        let cutoff = std::time::SystemTime::now()
            .checked_sub(older_than)
            .ok_or(SessionError::DurationOverflow)?;
        if !self.sessions_dir.exists() {
            return Ok(0);
        }
        let mut removed = 0;
        for entry in std::fs::read_dir(&self.sessions_dir)? {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let Ok(mtime) = meta.modified() else { continue };
            if mtime >= cutoff {
                continue;
            }
            match std::fs::remove_file(&path) {
                Ok(()) => removed += 1,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(removed)
    }
}

/// Default cleanup retention period — matches TS
/// `utils/cleanup.ts:DEFAULT_CLEANUP_PERIOD_DAYS = 30`.
pub const DEFAULT_CLEANUP_PERIOD_DAYS: u64 = 30;

/// [`DEFAULT_CLEANUP_PERIOD_DAYS`] as a `Duration` (convenience).
pub const fn default_cleanup_period() -> std::time::Duration {
    std::time::Duration::from_secs(DEFAULT_CLEANUP_PERIOD_DAYS * 24 * 60 * 60)
}

pub fn timestamp_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", now.as_secs())
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
