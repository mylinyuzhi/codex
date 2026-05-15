//! Presentation primitives for TUI surfaces.
//!
//! This module is intentionally crate-private. Domain state and external
//! commands stay in `state`, `update`, and `command`; presentation owns only
//! view models, layout guards, and rendering helpers.

pub(crate) mod confirm;
pub(crate) mod help;
pub(crate) mod help_slash;
pub(crate) mod information;
pub(crate) mod layout;
pub(crate) mod model_picker;
pub(crate) mod picker;
pub(crate) mod request;
pub(crate) mod rewind;
pub(crate) mod settings;
pub(crate) mod styles;
pub(crate) mod transcript;
