//! Source-backed streaming render for the active assistant turn.
//!
//! Reveal pacing lives on [`crate::state::ui::StreamingState`] (one complete
//! line per spinner tick); this module owns the render seam that turns the
//! revealed source into stable/mutable-tail `Line`s.

pub(crate) mod render_controller;
