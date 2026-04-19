//! Terminal UI using Elm architecture (TEA) with ratatui.
//!
//! Architecture:
//! ```text
//! Model (AppState) ← Update (handle_command) ← Events ← View (render)
//! ```
//!
//! TS: components/ + screens/ + ink/ + outputStyles/ + services/notifier.ts

// Load locale files at the crate root so the generated `_rust_i18n_t` symbol
// is visible to every `t!()` call across the crate. See `i18n` module for the
// init / locale-detection helpers.
rust_i18n::i18n!("locales", fallback = "en");

pub mod animation;
pub mod app;
pub mod autocomplete;
pub mod clipboard;
pub mod clipboard_copy;
pub mod command;
pub mod constants;
pub mod editor;
pub mod events;
pub mod i18n;
pub mod keybinding_bridge;
pub mod paste;
pub mod render;
mod render_overlays;
pub mod server_notification_handler;
pub mod state;
pub mod streaming;
pub mod terminal;
pub mod theme;
pub mod update;
mod update_rewind;
pub mod vim;
pub mod widgets;

// Legacy model kept for backward compatibility with existing tests
pub mod model;

// ── Public API ──
pub use animation::Animation;
pub use app::App;
pub use app::create_channels;
pub use command::ClearScope;
pub use command::UserCommand;
pub use events::TuiCommand;
pub use events::TuiEvent;
pub use paste::ImageData;
pub use paste::PasteManager;
pub use paste::ResolvedInput;
pub use render::render;
pub use server_notification_handler::handle_core_event;
pub use state::AppState;
pub use state::FocusTarget;
pub use state::Overlay;
pub use state::RunningState;
pub use terminal::Tui;
pub use terminal::restore_terminal;
pub use terminal::setup_terminal;
pub use theme::Theme;
pub use theme::ThemeName;
