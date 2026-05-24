//! TUI widgets — ratatui components for each UI section.
//!
//! Each widget implements the `ratatui::widgets::Widget` trait
//! and is constructed via builder pattern from immutable state references.

mod activity_panel;
pub(crate) mod activity_summary;
mod background_pills;
mod chat;
#[allow(dead_code)]
pub(crate) mod diff_display;
pub mod error_dialog;
mod input;
pub mod markdown;
pub mod notification;
mod queue_status_widget;
pub mod settings_panel;
pub mod spinner_verbs;
mod stash_notice;
mod status_indicator;
pub mod suggestion_popup;
pub mod textarea;
mod toast;
pub(crate) mod todo_panel;
mod transcript_modal;

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;

pub(crate) use activity_panel::ActivityPanel;
pub(crate) use background_pills::BackgroundPills;
pub(crate) use background_pills::build_view as build_background_pills_view;
pub(crate) use chat::ChatWidget;
pub(crate) use input::InputRenderModel;
pub(crate) use input::InputWidget;
pub(crate) use queue_status_widget::QueueStatusWidget;
pub(crate) use stash_notice::StashNotice;
pub(crate) use status_indicator::StatusIndicator;
pub(crate) use status_indicator::StatusIndicatorView;
pub(crate) use suggestion_popup::SuggestionPopup;
pub(crate) use textarea::BolBehavior;
pub(crate) use textarea::EolBehavior;
pub(crate) use textarea::TextArea;
pub(crate) use toast::ToastWidget;
pub(crate) use transcript_modal::TranscriptLayoutIndex;
pub(crate) use transcript_modal::TranscriptStateWidget;
