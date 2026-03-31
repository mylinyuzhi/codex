//! Filesystem-based mailbox for inter-agent communication.
//!
//! Messages are stored as JSONL files at `{base_dir}/{team_name}/mailbox/{agent_id}.jsonl`.
//! Writes use atomic rename (write-to-temp + rename) for crash safety.
//! Per-recipient locks prevent concurrent read-modify-write races.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use snafu::ResultExt;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::error::team_error;
use crate::types::AgentMessage;

/// Filesystem-backed mailbox for inter-agent messaging.
///
/// Thread-safe: uses per-recipient locks to prevent concurrent
/// read-modify-write races on the same mailbox file.
#[derive(Debug, Clone)]
pub struct Mailbox {
    /// Base directory (e.g., `~/.cocode/teams/`).
    base_dir: PathBuf,
    /// Per-recipient write locks keyed by mailbox file path.
    /// Prevents concurrent senders from interleaving read-modify-write.
    locks: Arc<Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>>,
    /// Team names whose mailbox directories have already been created.
    ensured_dirs: Arc<Mutex<HashSet<String>>>,
}

impl Mailbox {
    /// Create a new mailbox rooted at the given base directory.
    pub fn new(base_dir: PathBuf) -> Self {
        Self {
            base_dir,
            locks: Arc::new(Mutex::new(HashMap::new())),
            ensured_dirs: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Send a message to a recipient's mailbox.
    ///
    /// Acquires a per-recipient lock, then does atomic read-modify-write
    /// (read existing, append, write-to-temp, rename).
    pub async fn send(&self, team_name: &str, msg: &AgentMessage) -> Result<()> {
        let path = self.mailbox_path(team_name, &msg.to);
        self.ensure_mailbox_dir(team_name).await?;

        let lock = self.get_lock(&path).await;
        let _guard = lock.lock().await;

        let mut messages = self.read_all_raw(&path).await?;
        messages.push(msg.clone());
        self.write_all(&path, &messages).await
    }

    /// Send with dual-write: fast path first, then filesystem for persistence.
    ///
    /// If the recipient has an in-process channel via [`FastPath`], the message
    /// is delivered immediately over the channel. The filesystem write always
    /// happens for audit trail and crash recovery.
    pub async fn send_with_fast_path(
        &self,
        team_name: &str,
        msg: &AgentMessage,
        fast_path: &crate::fast_path::FastPath,
    ) -> Result<()> {
        // Fast path: try in-process channel (non-blocking, best-effort).
        fast_path.try_send(team_name, &msg.to, msg.clone()).await;

        // Always persist to filesystem for audit trail.
        self.send(team_name, msg).await
    }

    /// Broadcast with dual-write: fast path + filesystem.
    pub async fn broadcast_with_fast_path(
        &self,
        team_name: &str,
        msg: &AgentMessage,
        members: &[String],
        fast_path: &crate::fast_path::FastPath,
    ) -> Result<()> {
        // Fast path for all in-process recipients.
        fast_path.broadcast(team_name, msg, members).await;

        // Always persist to filesystem for audit trail.
        self.broadcast(team_name, msg, members).await
    }

    /// Read unread messages for an agent.
    pub async fn read_unread(&self, team_name: &str, agent_id: &str) -> Result<Vec<AgentMessage>> {
        let path = self.mailbox_path(team_name, agent_id);
        let messages = self.read_all_raw(&path).await?;
        Ok(messages.into_iter().filter(|m| !m.read).collect())
    }

    /// Read unread messages and mark them as read in a single file operation.
    ///
    /// Avoids the double file read of calling `read_unread` then `mark_read`.
    pub async fn take_unread(&self, team_name: &str, agent_id: &str) -> Result<Vec<AgentMessage>> {
        let path = self.mailbox_path(team_name, agent_id);

        let lock = self.get_lock(&path).await;
        let _guard = lock.lock().await;

        let mut messages = self.read_all_raw(&path).await?;
        let unread: Vec<AgentMessage> = messages.iter().filter(|m| !m.read).cloned().collect();

        if !unread.is_empty() {
            for msg in &mut messages {
                if !msg.read {
                    msg.read = true;
                }
            }
            self.write_all(&path, &messages).await?;
        }

        Ok(unread)
    }

    /// Mark specific messages as read.
    pub async fn mark_read(
        &self,
        team_name: &str,
        agent_id: &str,
        message_ids: &[String],
    ) -> Result<()> {
        let path = self.mailbox_path(team_name, agent_id);

        let lock = self.get_lock(&path).await;
        let _guard = lock.lock().await;

        let mut messages = self.read_all_raw(&path).await?;
        let mut changed = false;
        for msg in &mut messages {
            if message_ids.contains(&msg.id) && !msg.read {
                msg.read = true;
                changed = true;
            }
        }
        if changed {
            self.write_all(&path, &messages).await?;
        }
        Ok(())
    }

    /// Broadcast a message to multiple recipients.
    pub async fn broadcast(
        &self,
        team_name: &str,
        msg: &AgentMessage,
        members: &[String],
    ) -> Result<()> {
        for member_id in members {
            if *member_id == msg.from {
                continue; // Don't send to self
            }
            let mut member_msg = msg.clone();
            member_msg.to = member_id.clone();
            // Each recipient gets their own message ID
            member_msg.id = uuid::Uuid::new_v4().to_string();
            self.send(team_name, &member_msg).await?;
        }
        Ok(())
    }

    /// Get the count of pending (unread) messages for an agent.
    pub async fn pending_count(&self, team_name: &str, agent_id: &str) -> Result<usize> {
        self.read_unread(team_name, agent_id).await.map(|v| v.len())
    }

    /// Clear all messages for an agent.
    pub async fn clear(&self, team_name: &str, agent_id: &str) -> Result<()> {
        let path = self.mailbox_path(team_name, agent_id);
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).context(team_error::MailboxSnafu {
                message: format!("removing mailbox: {}", path.display()),
            }),
        }
    }

    /// Read all messages for an agent (including read ones).
    pub async fn read_all(&self, team_name: &str, agent_id: &str) -> Result<Vec<AgentMessage>> {
        let path = self.mailbox_path(team_name, agent_id);
        self.read_all_raw(&path).await
    }

    // === Internal helpers ===

    /// Get or create a per-recipient lock for the given mailbox path.
    ///
    /// Also prunes stale entries (strong_count == 1 means only the map holds
    /// a reference, so the lock is no longer in use).
    async fn get_lock(&self, path: &Path) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.locks.lock().await;
        // Prune stale locks (only the HashMap holds a reference).
        locks.retain(|_, v| Arc::strong_count(v) > 1);
        locks
            .entry(path.to_path_buf())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    fn mailbox_path(&self, team_name: &str, agent_id: &str) -> PathBuf {
        self.base_dir
            .join(team_name)
            .join("mailbox")
            .join(format!("{agent_id}.jsonl"))
    }

    async fn ensure_mailbox_dir(&self, team_name: &str) -> Result<()> {
        {
            let ensured = self.ensured_dirs.lock().await;
            if ensured.contains(team_name) {
                return Ok(());
            }
        }
        let dir = self.base_dir.join(team_name).join("mailbox");
        tokio::fs::create_dir_all(&dir)
            .await
            .context(team_error::MailboxSnafu {
                message: format!("creating mailbox dir: {}", dir.display()),
            })?;
        self.ensured_dirs.lock().await.insert(team_name.to_string());
        Ok(())
    }

    async fn read_all_raw(&self, path: &Path) -> Result<Vec<AgentMessage>> {
        let content = match tokio::fs::read_to_string(path).await {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(e).context(team_error::MailboxSnafu {
                    message: format!("reading: {}", path.display()),
                });
            }
        };

        let mut messages = Vec::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match serde_json::from_str::<AgentMessage>(trimmed) {
                Ok(msg) => messages.push(msg),
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        line = trimmed,
                        "Skipping corrupted mailbox line"
                    );
                }
            }
        }
        Ok(messages)
    }

    async fn write_all(&self, path: &Path, messages: &[AgentMessage]) -> Result<()> {
        let mut lines = String::new();
        for msg in messages {
            let json = serde_json::to_string(msg).context(team_error::SerdeSnafu {
                message: "serializing message",
            })?;
            lines.push_str(&json);
            lines.push('\n');
        }

        // Atomic write: write to temp file, then rename
        let tmp_path = path.with_extension("jsonl.tmp");
        tokio::fs::write(&tmp_path, lines.as_bytes())
            .await
            .context(team_error::MailboxSnafu {
                message: format!("writing temp: {}", tmp_path.display()),
            })?;

        tokio::fs::rename(&tmp_path, path)
            .await
            .context(team_error::MailboxSnafu {
                message: format!("renaming to: {}", path.display()),
            })?;

        Ok(())
    }
}

#[cfg(test)]
#[path = "mailbox.test.rs"]
mod tests;
