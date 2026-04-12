//! Prompt history persistence via JSONL.
//!
//! TS: history.ts — JSONL append-only log at ~/.coco/history.jsonl.
//! Entries are project-scoped, session-tagged, newest-first on read.

use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::io::BufRead;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

const MAX_HISTORY_ITEMS: usize = 100;

/// A single history log entry (serialized to JSONL).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryLogEntry {
    /// Display text shown in up-arrow history.
    pub display: String,
    /// Pasted content references (id → content).
    #[serde(default)]
    pub pasted_contents: HashMap<i32, PastedContentRef>,
    /// Unix timestamp (milliseconds).
    pub timestamp: i64,
    /// Project root path.
    pub project: String,
    /// Session ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// A pasted content reference (stored inline for small, hash for large).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PastedContentRef {
    pub id: i32,
    #[serde(rename = "type")]
    pub content_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
}

/// A resolved history entry for display.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub display: String,
    pub timestamp: i64,
    pub pasted_contents: HashMap<i32, String>,
}

/// Prompt history manager.
pub struct PromptHistory {
    history_path: PathBuf,
    project: String,
    session_id: String,
}

impl PromptHistory {
    /// Create a new history manager.
    pub fn new(config_dir: &Path, project: &str, session_id: &str) -> Self {
        Self {
            history_path: config_dir.join("history.jsonl"),
            project: project.to_string(),
            session_id: session_id.to_string(),
        }
    }

    /// Add an entry to prompt history (append to JSONL).
    pub fn add(&self, display: &str) -> anyhow::Result<()> {
        let entry = HistoryLogEntry {
            display: display.to_string(),
            pasted_contents: HashMap::new(),
            timestamp: current_timestamp_ms(),
            project: self.project.clone(),
            session_id: Some(self.session_id.clone()),
        };

        if let Some(parent) = self.history_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.history_path)?;

        let line = serde_json::to_string(&entry)?;
        writeln!(file, "{line}")?;
        Ok(())
    }

    /// Read history entries for the current project, newest first.
    ///
    /// Current session entries come first, then other sessions.
    /// Limited to MAX_HISTORY_ITEMS total.
    pub fn get_history(&self) -> Vec<HistoryEntry> {
        let entries = match self.read_all_entries() {
            Ok(e) => e,
            Err(_) => return Vec::new(),
        };

        let mut current_session = Vec::new();
        let mut other_sessions = Vec::new();

        for entry in entries.into_iter().rev() {
            if entry.project != self.project {
                continue;
            }
            if entry.session_id.as_deref() == Some(&self.session_id) {
                current_session.push(to_history_entry(&entry));
            } else {
                other_sessions.push(to_history_entry(&entry));
            }
            if current_session.len() + other_sessions.len() >= MAX_HISTORY_ITEMS {
                break;
            }
        }

        // Current session first, then others
        current_session.extend(other_sessions);
        current_session.truncate(MAX_HISTORY_ITEMS);
        current_session
    }

    /// Read all log entries from the JSONL file.
    fn read_all_entries(&self) -> anyhow::Result<Vec<HistoryLogEntry>> {
        if !self.history_path.exists() {
            return Ok(Vec::new());
        }
        let file = std::fs::File::open(&self.history_path)?;
        let reader = std::io::BufReader::new(file);
        let mut entries = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(entry) = serde_json::from_str::<HistoryLogEntry>(&line) {
                entries.push(entry);
            }
        }
        Ok(entries)
    }
}

fn to_history_entry(log: &HistoryLogEntry) -> HistoryEntry {
    let mut pasted = HashMap::new();
    for (id, ref_entry) in &log.pasted_contents {
        if let Some(content) = &ref_entry.content {
            pasted.insert(*id, content.clone());
        }
    }
    HistoryEntry {
        display: log.display.clone(),
        timestamp: log.timestamp,
        pasted_contents: pasted,
    }
}

fn current_timestamp_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Format a pasted text reference.
pub fn format_pasted_text_ref(id: i32, num_lines: i32) -> String {
    if num_lines == 0 {
        format!("[Pasted text #{id}]")
    } else {
        format!("[Pasted text #{id} +{num_lines} lines]")
    }
}

/// Format an image reference.
pub fn format_image_ref(id: i32) -> String {
    format!("[Image #{id}]")
}

#[cfg(test)]
#[path = "history.test.rs"]
mod tests;
