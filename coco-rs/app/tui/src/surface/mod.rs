//! Native-scrollback surface substrate.
//!
//! The current production TUI still uses `terminal::Tui` and fullscreen
//! ratatui. This module is the migration target for the retained bottom
//! viewport and terminal-native finalized history.

pub(crate) mod controller;
pub(crate) mod history_driver;
pub(crate) mod history_emitter;
pub(crate) mod history_insert;
pub(crate) mod history_lines;
pub(crate) mod history_reflow;
pub(crate) mod overlay;
pub(crate) mod terminal;
pub(crate) mod viewport;
