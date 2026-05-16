//! Cursor decision — single point of truth.
//!
//! The active surface renderer returns a [`FrameLayout`]; this module turns
//! that plus `AppState` into an optional [`CursorClaim`]. `SurfaceTerminal`
//! applies the claim after drawing the retained viewport. No widget calls
//! `Frame::set_cursor_position` directly.
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
use crate::widgets::InputRenderModel;

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
    let is_streaming = state.is_streaming();
    let command_palette_filter: Option<&str> = match state.ui.overlay.as_ref() {
        Some(Overlay::CommandPalette(cp)) => Some(cp.filter.as_str()),
        _ => None,
    };
    let model = InputRenderModel::build(
        &state.ui.input,
        is_streaming,
        state.is_plan_mode(),
        state.session.prompt_suggestions.last().map(String::as_str),
        !state.session.queued_commands.is_empty(),
        command_palette_filter,
    );

    // The indicator span ("❯ " / "! " / "~ ") is always 2 cols.
    let indicator_width: u16 = 2;

    let raw_cursor: i32 = if let Some(filter) = model.command_palette_filter.as_deref() {
        // 1 col for the leading `/` + visible filter width.
        1 + UnicodeWidthStr::width(filter) as i32
    } else {
        // Display column = width of the visible text up to the cursor's
        // byte offset. Handles CJK ("你好" with cursor at end → col 4).
        let text = state.ui.input.text();
        let visible_text = &text[model.prefix_consumed..];
        let cursor_byte = state
            .ui
            .input
            .textarea
            .cursor()
            .saturating_sub(model.prefix_consumed);
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
