//! IDE selection state tracking.
//!
//! Tracks the user's current text selection or active file in the IDE,
//! updated via `selection_changed` MCP notifications.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;
use tokio::sync::RwLock;
use tracing::debug;

/// A snapshot of the user's current selection in the IDE.
#[derive(Debug, Clone)]
pub struct IdeSelection {
    /// Path to the file containing the selection.
    pub file_path: PathBuf,
    /// Selected text (if any).
    pub text: Option<String>,
    /// 0-based start line of the selection.
    pub line_start: i32,
    /// Number of selected lines (0 = file-only context, no selection).
    pub line_count: i32,
}

impl IdeSelection {
    /// Whether this represents a text selection (vs just file focus).
    pub fn has_selection(&self) -> bool {
        self.line_count > 0
    }
}

/// Thread-safe container for the current IDE selection.
#[derive(Debug, Clone)]
pub struct IdeSelectionState {
    current: Arc<RwLock<Option<IdeSelection>>>,
}

impl Default for IdeSelectionState {
    fn default() -> Self {
        Self::new()
    }
}

impl IdeSelectionState {
    /// Create a new empty selection state.
    pub fn new() -> Self {
        Self {
            current: Arc::new(RwLock::new(None)),
        }
    }

    /// Get the current selection.
    pub async fn get(&self) -> Option<IdeSelection> {
        self.current.read().await.clone()
    }

    /// Update the selection from a `selection_changed` MCP notification.
    pub async fn update_from_notification(&self, params: &serde_json::Value) {
        let notification: SelectionChangedParams = match serde_json::from_value(params.clone()) {
            Ok(n) => n,
            Err(e) => {
                debug!("Failed to parse selection_changed params: {e}");
                return;
            }
        };

        // Require a non-empty file path for any selection update
        let file_path = match &notification.file_path {
            Some(fp) if !fp.is_empty() => PathBuf::from(fp),
            _ => {
                // No valid file path — clear selection
                let mut current = self.current.write().await;
                *current = None;
                return;
            }
        };

        let selection = if let Some(sel) = &notification.selection {
            // Has a range selection. Clamp to non-negative for defensive handling
            // of reversed ranges (end < start).
            let mut line_count = (sel.end.line - sel.start.line + 1).max(0);
            // If cursor is at the start of the last line with no characters
            // selected, don't count that line
            if sel.end.character == 0 && line_count > 1 {
                line_count -= 1;
            }

            IdeSelection {
                file_path,
                text: notification.text,
                line_start: sel.start.line,
                line_count,
            }
        } else {
            // File-only context (no selection range)
            IdeSelection {
                file_path,
                text: notification.text,
                line_start: 0,
                line_count: 0,
            }
        };

        let mut current = self.current.write().await;
        *current = Some(selection);
    }

    /// Clear the current selection.
    pub async fn clear(&self) {
        let mut current = self.current.write().await;
        *current = None;
    }
}

/// Params from the `selection_changed` MCP notification.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SelectionChangedParams {
    #[serde(default)]
    selection: Option<SelectionRange>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    file_path: Option<String>,
}

/// Selection range with start/end positions.
#[derive(Debug, Deserialize)]
struct SelectionRange {
    start: Position,
    end: Position,
}

/// 0-based line/character position.
#[derive(Debug, Deserialize)]
struct Position {
    line: i32,
    #[serde(default)]
    character: i32,
}

#[cfg(test)]
#[path = "selection.test.rs"]
mod tests;
