//! Cursor decision — single point of truth.
//!
//! `render::render` returns a [`FrameLayout`]; this module turns that
//! plus `AppState` into an optional [`CursorClaim`]. `Tui::draw` collects
//! the claim from the render closure and applies it via crossterm
//! `queue!(SetCursorStyle, MoveTo, Show)` post-draw. No widget calls
//! `Frame::set_cursor_position` anywhere; ratatui's own cursor handling
//! sees `cursor_position == None` and emits a `Hide`, then our post-draw
//! pin overrides with the final position + style.
//!
//! Why post-draw instead of `Frame::set_cursor_position` inside the
//! closure: ratatui 0.30's `Frame` exposes no `set_cursor_style`, so the
//! cursor shape (bar / block / underline) can only be controlled via raw
//! `crossterm::cursor::SetCursorStyle` queued to stdout. Doing the whole
//! pin (style + position + show/hide) post-draw keeps the policy in one
//! place and makes the focus-gained / suspend-resume re-pin path
//! identical to the normal path.

use crossterm::cursor::SetCursorStyle;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use unicode_width::UnicodeWidthStr;

use crate::state::AppState;
use crate::state::FocusTarget;
use crate::state::Overlay;
use crate::state::PromptMode;

/// What the cursor should look like at the end of this frame.
///
/// `None` returned from [`compute_cursor`] means "no cursor this frame";
/// `Tui::draw` then emits `Hide + MoveTo(0, 0)` so the terminal never
/// holds a stale cursor coordinate that focus-gained could re-show in a
/// status-bar position.
#[derive(Debug, Clone, Copy)]
pub struct CursorClaim {
    pub position: Position,
    pub style: SetCursorStyle,
}

/// Decide where (and whether) the cursor goes for the next frame.
///
/// Single decision point: the input widget is the only base cursor source
/// today. Modal overlays hide that base cursor unless they explicitly mirror
/// input text, as the command palette does. Returning `None` tells
/// `Tui::draw` to hide the cursor explicitly — see module docs for why hide
/// alone isn't enough on iTerm2 / macOS Terminal.
pub fn compute_cursor(state: &AppState, input_area: Rect) -> Option<CursorClaim> {
    if state.ui.focus != FocusTarget::Input {
        return None;
    }
    if state
        .ui
        .overlay
        .as_ref()
        .is_some_and(|overlay| !matches!(overlay, Overlay::CommandPalette(_)))
    {
        return None;
    }
    if input_area.width == 0 || input_area.height == 0 {
        return None;
    }
    let (x, y) = compute_input_xy(state, input_area);
    Some(CursorClaim {
        position: Position { x, y },
        style: SetCursorStyle::DefaultUserShape,
    })
}

/// Compute the cursor's absolute terminal coordinates inside the input
/// widget's area. Empty input is intentionally NOT special-cased:
/// returning a real position even for an empty buffer is what fixes the
/// "cursor floats to the status bar on focus regain" bug — the cursor
/// always has a defined home.
fn compute_input_xy(state: &AppState, area: Rect) -> (u16, u16) {
    // While streaming we render in "queue input" mode and force Normal
    // prompt: the `! ` / `# ` indicators don't apply to queued messages.
    // Mirrors `render_input` at render.rs:671-675.
    let is_streaming = state.is_streaming();
    let prompt_mode = if is_streaming {
        PromptMode::Normal
    } else {
        state.ui.input.prompt_mode()
    };
    let is_empty = state.ui.input.is_empty();

    // The indicator owns 1 char + optional space when in Bash / Memory
    // mode. The visible text starts after that, and the cursor's byte
    // offset is computed against the *visible* slice.
    let prefix_consumed: usize = if is_empty || prompt_mode == PromptMode::Normal {
        0
    } else {
        let body = &state.ui.input.text()[1..];
        1 + if body.starts_with(' ') { 1 } else { 0 }
    };

    // When the command palette is open, the input bar mirrors `/<filter>`
    // and cursor follows the filter end — bypasses the textarea's own
    // cursor position. Same logic as render.rs:714-716.
    let command_palette_filter: Option<&str> = match state.ui.overlay.as_ref() {
        Some(Overlay::CommandPalette(cp)) => Some(cp.filter.as_str()),
        _ => None,
    };

    // The indicator span ("❯ " / "! " / "# " / "~ ") is always 2 cols.
    let indicator_width: u16 = 2;

    let raw_cursor: i32 = if let Some(filter) = command_palette_filter {
        // 1 col for the leading `/` + visible filter width.
        1 + UnicodeWidthStr::width(filter) as i32
    } else {
        // Display column = width of the visible text up to the cursor's
        // byte offset. Handles CJK ("你好" with cursor at end → col 4).
        let text = state.ui.input.text();
        let visible_text = &text[prefix_consumed..];
        let cursor_byte = state
            .ui
            .input
            .textarea
            .cursor()
            .saturating_sub(prefix_consumed);
        let cursor_byte = cursor_byte.min(visible_text.len());
        UnicodeWidthStr::width(&visible_text[..cursor_byte]) as i32
    };

    let max_cursor = area.width.saturating_sub(indicator_width + 1) as i32;
    let cursor_x = area.x + indicator_width + raw_cursor.min(max_cursor) as u16;
    let cursor_y = area.y + 1;
    (cursor_x, cursor_y)
}

#[cfg(test)]
#[path = "cursor.test.rs"]
mod tests;
