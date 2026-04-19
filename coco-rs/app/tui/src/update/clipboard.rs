//! Clipboard-related command handlers.
//!
//! [`copy_last_message`] backs `Ctrl+O` / `/copy`.
//! [`paste_from_clipboard`] backs `Ctrl+V` — the image-paste path for
//! screenshot → multimodal-agent flows. Plain-text paste is owned by the
//! terminal's bracketed-paste (Cmd+V / Ctrl+Shift+V), so this handler is
//! exclusively about pulling an image off the system clipboard.

use crate::clipboard_copy;
use crate::i18n::t;
use crate::paste::ImageData;
use crate::state::AppState;
use crate::state::ui::Toast;

/// Copy the last agent response (raw markdown) to the system clipboard and
/// surface the result as a toast. Mirrors codex-rs
/// `ChatWidget::copy_last_agent_markdown`.
pub(super) fn copy_last_message(state: &mut AppState) {
    copy_last_message_with(state, clipboard_copy::copy_to_clipboard);
}

/// Inner implementation with an injectable backend — exposed to the update
/// tests so they can stub out real clipboard/tty I/O.
pub(super) fn copy_last_message_with(
    state: &mut AppState,
    copy_fn: impl FnOnce(&str) -> Result<Option<clipboard_copy::ClipboardLease>, String>,
) {
    let Some(markdown) = state.session.last_agent_markdown.clone() else {
        state
            .ui
            .add_toast(Toast::info(t!("toast.no_agent_response").to_string()));
        return;
    };
    if markdown.is_empty() {
        state
            .ui
            .add_toast(Toast::info(t!("toast.no_agent_response").to_string()));
        return;
    }
    let char_count = markdown.chars().count();
    match copy_fn(&markdown) {
        Ok(lease) => {
            // On Linux the arboard path returns a lease whose drop-point
            // defines when the clipboard content disappears; without a lease
            // we either used OSC 52 (terminal-owned, survives exit) or a
            // non-Linux native clipboard (OS-owned, survives exit). Tell the
            // user which one they got so they can plan for app exit.
            let durability = if lease.is_some() {
                t!("toast.copy_durability_until_exit")
            } else {
                t!("toast.copy_durability_persistent")
            };
            state.ui.clipboard_lease = lease;
            state.ui.add_toast(Toast::success(
                t!(
                    "toast.copied_chars",
                    count = char_count,
                    durability = durability
                )
                .to_string(),
            ));
        }
        Err(err) => {
            state.ui.add_toast(Toast::error(
                t!("toast.copy_failed_short", error = err).to_string(),
            ));
        }
    }
}

/// Read an image from the system clipboard and insert a `[Image #N]` pill
/// at the cursor. On submit, `PasteManager::resolve_structured` pulls the
/// bytes out of the pill and attaches them to `UserCommand::SubmitInput.images`
/// so a multimodal agent sees the screenshot alongside the user's prompt.
///
/// Text paste intentionally stays with the terminal's bracketed-paste flow
/// (handled by `TuiEvent::Paste`) — this path is image-only.
pub(super) async fn paste_from_clipboard(state: &mut AppState) {
    paste_from_clipboard_with(state, crate::clipboard::read_clipboard_image).await;
}

/// Inner implementation with an injectable reader so tests can avoid the
/// real `xclip` / `osascript` subprocesses.
pub(super) async fn paste_from_clipboard_with<F, Fut>(state: &mut AppState, read_fn: F)
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<Option<ImageData>>>,
{
    match read_fn().await {
        Ok(Some(image)) => {
            let size_kb = image.bytes.len().div_ceil(1024);
            let pill = state
                .ui
                .paste_manager
                .add_image_data(image.bytes, image.mime);
            for c in pill.chars() {
                state.ui.input.insert_char(c);
            }
            state.ui.add_toast(Toast::success(
                t!("toast.image_attached", size_kb = size_kb).to_string(),
            ));
        }
        Ok(None) => {
            state
                .ui
                .add_toast(Toast::info(t!("toast.no_image_clipboard").to_string()));
        }
        Err(e) => {
            state.ui.add_toast(Toast::error(
                t!("toast.paste_failed", error = e.to_string()).to_string(),
            ));
        }
    }
}

#[cfg(test)]
#[path = "clipboard.test.rs"]
mod tests;
