//! Filterable-list picker overlay renderers.

use ratatui::prelude::*;

use crate::presentation::model_picker;
use crate::presentation::picker;
use crate::state::CommandPaletteOverlay;
use crate::state::ExportOverlay;
use crate::state::McpServerSelectOverlay;
use crate::state::MemoryDialogOverlay;
use crate::state::ModelPickerOverlay;
use crate::state::QuickOpenOverlay;
use crate::state::SessionBrowserOverlay;
use crate::theme::Theme;

pub(super) fn render_model_picker(
    frame: &mut Frame,
    area: Rect,
    m: &ModelPickerOverlay,
    theme: &Theme,
) {
    model_picker::render_model_picker(frame, area, m, theme);
}

pub(super) fn model_picker_content(
    m: &ModelPickerOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    model_picker::content(m, theme)
}

pub(super) fn command_palette_content(
    cp: &CommandPaletteOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    picker::command_palette_content(cp, theme)
}

pub(super) fn session_browser_content(
    s: &SessionBrowserOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    picker::session_browser_content(s, theme)
}

pub(super) fn quick_open_content(q: &QuickOpenOverlay, theme: &Theme) -> (String, String, Color) {
    picker::quick_open_content(q, theme)
}

pub(super) fn export_content(e: &ExportOverlay, theme: &Theme) -> (String, String, Color) {
    picker::export_content(e, theme)
}

pub(super) fn memory_dialog_content(
    m: &MemoryDialogOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    picker::memory_dialog_content(m, theme)
}

pub(super) fn mcp_server_select_content(
    ms: &McpServerSelectOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    picker::mcp_server_select_content(ms, theme)
}
