//! Native-scrollback surface substrate.

// The pure paint engine (terminal, history_insert, history_reflow,
// compatibility) now lives in `coco_tui_ui::engine`. This module keeps the
// AppState→Line projection + emission layer that drives it.
pub(crate) mod controller;
pub(crate) mod history_driver;
pub(crate) mod history_emitter;
pub(crate) mod history_lines;
pub(crate) mod line_fingerprint;
pub(crate) mod modal;
pub(crate) mod stream;
pub(crate) mod viewport;
