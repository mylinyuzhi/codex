//! Terminal UI for cocode.
//!
//! This crate provides a terminal-based user interface using ratatui and crossterm.
//! It follows The Elm Architecture (TEA) pattern with async event handling.
//!
//! ## Architecture
//!
//! - **Model**: Application state ([`state::AppState`])
//! - **Message**: Events that trigger state changes ([`event::TuiEvent`])
//! - **Update**: Pure functions that update state based on messages
//! - **View**: Renders state to terminal using ratatui widgets
//!
//! ## Key Features
//!
//! - Async event handling with tokio
//! - Real-time streaming content display
//! - Tool execution visualization
//! - Permission prompt overlays
//! - Keyboard shortcuts (Tab: plan mode, Ctrl+T: thinking, Ctrl+M: model)
//!
//! ## Example
//!
//! ```ignore
//! use cocode_tui::Tui;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let tui = Tui::new()?;
//!     tui.run().await
//! }
//! ```

#![warn(missing_docs)]
#![warn(clippy::unwrap_used)]

pub mod event;
pub mod state;
pub mod terminal;
pub mod widgets;

// Re-export commonly used types
pub use event::{TuiCommand, TuiEvent};
pub use state::{AppState, FocusTarget, Overlay, RunningState};
pub use terminal::{Tui, restore_terminal, setup_terminal};

#[cfg(test)]
mod tests {
    #[test]
    fn test_crate_compiles() {
        // Basic smoke test to ensure the crate compiles
        assert!(true);
    }
}
