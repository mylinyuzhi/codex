//! Pure, domain-free ratatui widgets. Each is constructed from plain values /
//! view models (never `AppState`) and implements `ratatui::widgets::Widget` or
//! renders into a `Buffer` / returns `Line`s.

pub mod diff_display;
pub mod notification;
pub mod spinner_verbs;
pub mod status_indicator;
pub mod textarea;

pub use status_indicator::StatusIndicator;
pub use status_indicator::StatusIndicatorView;
pub use textarea::BolBehavior;
pub use textarea::EolBehavior;
pub use textarea::TextArea;
