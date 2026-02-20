//! Symbol search manager with debouncing for @# autocomplete.
//!
//! This module provides debounced symbol search for the `@#SymbolName`
//! mention feature. It builds an in-memory symbol index using tree-sitter
//! and performs fuzzy matching against symbol names.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use cocode_symbol_search::SymbolIndex;
use cocode_symbol_search::SymbolKind;

use crate::state::SymbolSuggestionItem;

/// Debounce delay in milliseconds.
const DEBOUNCE_MS: u64 = 100;

/// Maximum number of suggestions to return.
const MAX_SUGGESTIONS: i32 = 15;

/// Events sent from the symbol search manager to the TUI.
#[derive(Debug, Clone)]
pub enum SymbolSearchEvent {
    /// Symbol index has been built.
    IndexReady {
        /// Number of symbols indexed.
        symbol_count: i32,
    },
    /// Search results are ready.
    SearchResult {
        /// The query that was searched.
        query: String,
        /// The start position of the @# mention.
        start_pos: i32,
        /// The matching suggestions.
        suggestions: Vec<SymbolSuggestionItem>,
    },
}

/// Manages symbol search with debouncing.
///
/// This struct handles:
/// - Background index building using tree-sitter
/// - Debounced search scheduling (100ms delay)
/// - Cancellation of in-flight searches when query changes
pub struct SymbolSearchManager {
    /// Working directory for symbol discovery.
    cwd: PathBuf,
    /// Shared symbol index.
    index: Arc<RwLock<Option<SymbolIndex>>>,
    /// Currently scheduled search (debounce timer).
    pending_search: Option<PendingSearch>,
    /// Event sender to notify TUI of results.
    event_tx: mpsc::Sender<SymbolSearchEvent>,
}

/// A pending search waiting for debounce timeout.
struct PendingSearch {
    /// Handle to cancel the search task.
    handle: JoinHandle<()>,
}

impl SymbolSearchManager {
    /// Create a new symbol search manager.
    pub fn new(cwd: PathBuf, event_tx: mpsc::Sender<SymbolSearchEvent>) -> Self {
        Self {
            cwd,
            index: Arc::new(RwLock::new(None)),
            pending_search: None,
            event_tx,
        }
    }

    /// Start building the symbol index in the background.
    pub fn start_indexing(&self) {
        let index = self.index.clone();
        let cwd = self.cwd.clone();
        let event_tx = self.event_tx.clone();

        tokio::spawn(async move {
            let root = cwd.clone();
            let result = tokio::task::spawn_blocking(move || SymbolIndex::build(&root)).await;

            match result {
                Ok(Ok(built_index)) => {
                    let count = built_index.len();
                    *index.write().await = Some(built_index);
                    let _ = event_tx
                        .send(SymbolSearchEvent::IndexReady {
                            symbol_count: count,
                        })
                        .await;
                }
                Ok(Err(e)) => {
                    tracing::warn!("Failed to build symbol index: {e}");
                }
                Err(e) => {
                    tracing::warn!("Symbol index task panicked: {e}");
                }
            }
        });
    }

    /// Handle a query change from user input.
    ///
    /// This method debounces the search — if called multiple times in quick
    /// succession, only the last query will be searched after the debounce
    /// delay.
    pub fn on_query(&mut self, query: String, start_pos: i32) {
        // Cancel any pending search
        if let Some(pending) = self.pending_search.take() {
            pending.handle.abort();
        }

        // Don't search empty queries
        if query.is_empty() {
            return;
        }

        // Schedule a new debounced search
        let index = self.index.clone();
        let event_tx = self.event_tx.clone();
        let query_clone = query;

        let handle = tokio::spawn(async move {
            // Wait for debounce delay
            tokio::time::sleep(Duration::from_millis(DEBOUNCE_MS)).await;

            // Perform the search
            let suggestions = {
                let guard = index.read().await;
                if let Some(ref idx) = *guard {
                    idx.search(&query_clone, MAX_SUGGESTIONS)
                        .into_iter()
                        .map(|r| SymbolSuggestionItem {
                            name: r.name,
                            kind: r.kind,
                            file_path: r.file_path,
                            line: r.line,
                            score: r.score,
                            match_indices: r.match_indices,
                        })
                        .collect()
                } else {
                    Vec::new()
                }
            };

            // Send results
            let _ = event_tx
                .send(SymbolSearchEvent::SearchResult {
                    query: query_clone,
                    start_pos,
                    suggestions,
                })
                .await;
        });

        self.pending_search = Some(PendingSearch { handle });
    }

    /// Cancel any pending search.
    pub fn cancel(&mut self) {
        if let Some(pending) = self.pending_search.take() {
            pending.handle.abort();
        }
    }
}

/// Create a channel for symbol search events.
pub fn create_symbol_search_channel() -> (
    mpsc::Sender<SymbolSearchEvent>,
    mpsc::Receiver<SymbolSearchEvent>,
) {
    mpsc::channel(16)
}

/// Convert a `SymbolKind` to a short display label.
pub fn symbol_kind_label(kind: SymbolKind) -> &'static str {
    kind.label()
}

#[cfg(test)]
#[path = "symbol_search.test.rs"]
mod tests;
