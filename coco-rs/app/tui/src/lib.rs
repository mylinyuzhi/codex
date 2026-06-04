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

pub mod app;
pub mod autocomplete;
pub mod command;
pub mod completion;
pub mod cursor;
pub mod display_settings;
pub mod events;
mod frame_layout;
mod frame_requester;
pub mod git_index_watcher;
pub mod i18n;
pub mod job_control;
pub mod keybinding_bridge;
pub mod keybinding_dispatch;
pub mod keybinding_resolver;
pub mod keybinding_setup;
pub mod keymap;
mod perf;
mod presentation;
pub mod server_notification_handler;
pub mod state;
pub(crate) mod status_bar;
pub mod streaming;
pub(crate) mod surface;
mod surface_content;
mod system_theme_probe;
pub mod terminal;
pub mod theme;
pub mod tool_display;
pub mod update;
mod update_rewind;
pub mod vim;
pub(crate) mod widgets;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

// Legacy model kept for backward compatibility with existing tests
pub mod model;

// ── Public API ──
pub use app::App;
pub use app::create_channels;
pub use coco_tui_ui::display::SyntaxHighlighting;
pub use coco_tui_ui::paste::ImageData;
pub use coco_tui_ui::paste::PasteManager;
pub use coco_tui_ui::paste::ResolvedInput;
pub use coco_tui_ui::style::UiStyles;
pub use command::SystemPushKind;
pub use command::UserCommand;
pub use display_settings::DisplaySettings;
pub use events::TuiCommand;
pub use events::TuiEvent;
pub use frame_layout::FrameLayout;
pub use server_notification_handler::handle_core_event;
pub use server_notification_handler::handle_event_for_test;
pub use state::AppState;
pub use state::FocusTarget;
pub use state::RunningState;
pub use terminal::Tui;
pub use terminal::restore_terminal;
pub use theme::Theme;
pub use theme::ThemeName;
