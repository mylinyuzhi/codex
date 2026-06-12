//! Error dialog — renders the body text that `surface_content` wraps in
//! a centered error modal.
//!
//! Unlike a toast, an error state blocks input until dismissed — used for
//! `TurnEnded(Failed)` and non-retryable `Error` events so the user must
//! acknowledge them.
//!
//! Exposed as a small library (not a `ratatui::Widget`) because the
//! state framework already owns the block + border + centering layout;
//! we only need to format the body string.

use coco_types::ErrorParams;
use coco_types::ErrorPayload;

use crate::i18n::t;

/// Format a rich error body for the error modal. Includes category,
/// retryability hint, and a footer telling the user how to dismiss.
pub fn format_error_body(message: &str, category: Option<&str>, retryable: bool) -> String {
    let mut body = String::new();
    body.push_str(message.trim());
    body.push_str("\n\n");
    if let Some(cat) = category
        && !cat.is_empty()
    {
        body.push_str(&t!("error_dialog.category", cat = cat));
    }
    body.push_str(&if retryable {
        t!("error_dialog.retryable").to_string()
    } else {
        t!("error_dialog.non_retryable").to_string()
    });
    body.push_str(&t!("error_dialog.dismiss"));
    body
}

/// Body for a `TurnEnded(Failed)` outcome. Turn failures are always
/// treated as non-retryable at the UI level (retry happens inside the
/// agent loop; if it reached this event, retry was exhausted). The
/// `code` field is mapped to a category label for the modal so users
/// can distinguish `network` vs `provider` vs `auth` etc.
pub fn turn_failed_body(error: &ErrorPayload) -> String {
    let category = error_code_label(error.code);
    format_error_body(&error.message, Some(category), false)
}

fn error_code_label(code: coco_types::ErrorCode) -> &'static str {
    match code {
        coco_types::ErrorCode::Common => "common",
        coco_types::ErrorCode::Input => "input",
        coco_types::ErrorCode::Io => "io",
        coco_types::ErrorCode::Network => "network",
        coco_types::ErrorCode::Auth => "auth",
        coco_types::ErrorCode::Config => "config",
        coco_types::ErrorCode::Provider => "provider",
        coco_types::ErrorCode::Resource => "resource",
        coco_types::ErrorCode::SystemReminder => "system_reminder",
        coco_types::ErrorCode::HookBlocked => "hook_blocked",
        coco_types::ErrorCode::Unknown => "unknown",
    }
}

/// Body for an `Error` notification. Uses the event's `category` and
/// `retryable` fields.
pub fn error_body(params: &ErrorParams) -> String {
    format_error_body(
        &params.message,
        params.category.as_deref(),
        params.retryable,
    )
}

#[cfg(test)]
#[path = "error_dialog.test.rs"]
mod tests;
