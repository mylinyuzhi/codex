//! File search autocomplete (@path mentions).
//!
//! Debounced async file search using fuzzy matching against the file index.

use std::path::PathBuf;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::widgets::suggestion_popup::SuggestionItem;

/// Debounce delay for file search.
const DEBOUNCE: Duration = Duration::from_millis(100);

/// Maximum suggestions returned.
const MAX_SUGGESTIONS: usize = 15;

/// Events from file search to TUI.
#[derive(Debug, Clone)]
pub enum FileSearchEvent {
    /// Search results ready.
    SearchResult {
        query: String,
        start_pos: i32,
        suggestions: Vec<SuggestionItem>,
    },
}

/// Manages debounced file search.
pub struct FileSearchManager {
    cwd: PathBuf,
    pending: Option<JoinHandle<()>>,
    event_tx: mpsc::Sender<FileSearchEvent>,
}

impl FileSearchManager {
    /// Create a new file search manager.
    pub fn new(cwd: PathBuf, event_tx: mpsc::Sender<FileSearchEvent>) -> Self {
        Self {
            cwd,
            pending: None,
            event_tx,
        }
    }

    /// Schedule a debounced search.
    pub fn search(&mut self, query: String, start_pos: i32) {
        // Cancel pending search
        if let Some(handle) = self.pending.take() {
            handle.abort();
        }

        let cwd = self.cwd.clone();
        let tx = self.event_tx.clone();

        self.pending = Some(tokio::spawn(async move {
            tokio::time::sleep(DEBOUNCE).await;

            // Simple file matching — in production this would use nucleo or similar
            let suggestions = search_files_simple(&cwd, &query);

            let _ = tx
                .send(FileSearchEvent::SearchResult {
                    query,
                    start_pos,
                    suggestions,
                })
                .await;
        }));
    }

    /// Cancel any pending search.
    pub fn cancel(&mut self) {
        if let Some(handle) = self.pending.take() {
            handle.abort();
        }
    }

    /// Update the working directory.
    pub fn set_cwd(&mut self, cwd: PathBuf) {
        self.cwd = cwd;
    }
}

/// Create the file search channel.
pub fn create_file_search_channel() -> (
    mpsc::Sender<FileSearchEvent>,
    mpsc::Receiver<FileSearchEvent>,
) {
    mpsc::channel(16)
}

/// Simple file search (placeholder — real impl uses nucleo fuzzy matching).
fn search_files_simple(cwd: &PathBuf, query: &str) -> Vec<SuggestionItem> {
    let query_lower = query.to_lowercase();
    let mut results = Vec::new();

    if let Ok(entries) = std::fs::read_dir(cwd) {
        for entry in entries.take(100).flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.to_lowercase().contains(&query_lower) {
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                let suffix = if is_dir { "/" } else { "" };
                results.push(SuggestionItem {
                    label: format!("{name}{suffix}"),
                    description: None,
                });
                if results.len() >= MAX_SUGGESTIONS {
                    break;
                }
            }
        }
    }

    results
}
