//! Symbol search autocomplete (@#symbol mentions).
//!
//! Debounced async search against LSP symbol index.

use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::widgets::suggestion_popup::SuggestionItem;

/// Debounce delay for symbol search.
const DEBOUNCE: Duration = Duration::from_millis(150);

/// Events from symbol search to TUI.
#[derive(Debug, Clone)]
pub enum SymbolSearchEvent {
    /// Search results ready.
    SearchResult {
        query: String,
        start_pos: i32,
        suggestions: Vec<SuggestionItem>,
    },
}

/// Manages debounced symbol search via LSP.
pub struct SymbolSearchManager {
    pending: Option<JoinHandle<()>>,
    event_tx: mpsc::Sender<SymbolSearchEvent>,
}

impl SymbolSearchManager {
    /// Create a new symbol search manager.
    pub fn new(event_tx: mpsc::Sender<SymbolSearchEvent>) -> Self {
        Self {
            pending: None,
            event_tx,
        }
    }

    /// Schedule a debounced search.
    pub fn search(&mut self, query: String, start_pos: i32) {
        if let Some(handle) = self.pending.take() {
            handle.abort();
        }

        let tx = self.event_tx.clone();

        self.pending = Some(tokio::spawn(async move {
            tokio::time::sleep(DEBOUNCE).await;

            // Placeholder — real impl queries LSP workspace/symbol
            let suggestions = vec![SuggestionItem {
                label: format!("@#{query}"),
                description: Some("(LSP lookup pending)".to_string()),
            }];

            let _ = tx
                .send(SymbolSearchEvent::SearchResult {
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
}

/// Create the symbol search channel.
pub fn create_symbol_search_channel() -> (
    mpsc::Sender<SymbolSearchEvent>,
    mpsc::Receiver<SymbolSearchEvent>,
) {
    mpsc::channel(16)
}
