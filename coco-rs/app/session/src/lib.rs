//! Session persistence, history, and state aggregation.
//!
//! TS: bootstrap/state.ts + session management + history.ts

pub mod history;
pub mod recovery;
pub mod storage;

pub use history::HistoryEntry;
pub use history::PromptHistory;
pub use storage::Entry;
pub use storage::MetadataEntry;
pub use storage::ModelCostEntry;
pub use storage::RestoredCostSummary;
pub use storage::TranscriptEntry;
pub use storage::TranscriptMetadata;
pub use storage::TranscriptStore;
pub use storage::TranscriptUsage;
pub use storage::restore_cost_from_transcript;

use serde::Deserialize;
use serde::Serialize;
use std::path::Path;
use std::path::PathBuf;

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
    pub fn create(&self, model: &str, cwd: &Path) -> anyhow::Result<Session> {
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
        };
        self.save(&session)?;
        Ok(session)
    }

    /// Save/update a session.
    pub fn save(&self, session: &Session) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.sessions_dir)?;
        let session_file = self.sessions_dir.join(format!("{}.json", session.id));
        let json = serde_json::to_string_pretty(session)?;
        std::fs::write(session_file, json)?;
        Ok(())
    }

    /// Load a session by ID.
    pub fn load(&self, id: &str) -> anyhow::Result<Session> {
        let session_file = self.sessions_dir.join(format!("{id}.json"));
        let content = std::fs::read_to_string(&session_file)?;
        let session: Session = serde_json::from_str(&content)?;
        Ok(session)
    }

    /// Resume a session — loads it and updates the timestamp.
    pub fn resume(&self, id: &str) -> anyhow::Result<Session> {
        let mut session = self.load(id)?;
        session.updated_at = Some(timestamp_now());
        self.save(&session)?;
        Ok(session)
    }

    /// List all sessions, newest first.
    pub fn list(&self) -> anyhow::Result<Vec<Session>> {
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
    pub fn delete(&self, id: &str) -> anyhow::Result<()> {
        let session_file = self.sessions_dir.join(format!("{id}.json"));
        if session_file.exists() {
            std::fs::remove_file(session_file)?;
        }
        Ok(())
    }

    /// Get the most recent session.
    pub fn most_recent(&self) -> anyhow::Result<Option<Session>> {
        let sessions = self.list()?;
        Ok(sessions.into_iter().next())
    }

    /// Clean up old sessions beyond a limit.
    pub fn cleanup(&self, keep_count: usize) -> anyhow::Result<i32> {
        let sessions = self.list()?;
        let mut removed = 0;
        for session in sessions.iter().skip(keep_count) {
            self.delete(&session.id)?;
            removed += 1;
        }
        Ok(removed)
    }
}

fn timestamp_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", now.as_secs())
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
