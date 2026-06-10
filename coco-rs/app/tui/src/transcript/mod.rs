//! The TUI v2 transcript pipeline (`docs/coco-rs/ui/tui-v2-design.md` §6.4):
//!
//! - `cells` — engine-message grouping and the tool-commit boundary over
//!   derived `RenderedCell`s.
//! - `render` — the committed history renderer and the replay cache.
//! - `stream` — the source-backed stable/tail splitter, render key, and
//!   scrollback watermark for the in-flight assistant stream.
//! - `emission` — exactly-once emission tracking plus the anchored finalize
//!   (suffix-append vs replay decision).
//!
//! `surface/` holds the per-frame drivers and terminal I/O that consume this
//! logic; `state/` still owns the cell model itself (`RenderedCell`,
//! `message_to_cells`) until v2 Stage 2+ moves it here.

pub(crate) mod cells;
pub(crate) mod emission;
pub(crate) mod render;
pub(crate) mod stream;
