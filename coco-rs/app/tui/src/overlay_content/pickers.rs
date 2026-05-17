//! Filterable-list picker overlay content builders.

use ratatui::prelude::*;

use crate::presentation::model_picker;
use crate::presentation::picker;
use crate::presentation::styles::UiStyles;
use crate::state::ExportOverlay;
use crate::state::McpServerSelectOverlay;
use crate::state::MemoryDialogOverlay;
use crate::state::ModelPickerOverlay;
use crate::state::QuickOpenOverlay;
use crate::state::SessionBrowserOverlay;

pub(super) fn model_picker_content(
    m: &ModelPickerOverlay,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    model_picker::content(m, styles)
}

pub(super) fn session_browser_content(
    s: &SessionBrowserOverlay,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    picker::session_browser_content(s, styles)
}

pub(super) fn quick_open_content(
    q: &QuickOpenOverlay,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    picker::quick_open_content(q, styles)
}

pub(super) fn export_content(e: &ExportOverlay, styles: UiStyles<'_>) -> (String, String, Color) {
    picker::export_content(e, styles)
}

pub(super) fn memory_dialog_content(
    m: &MemoryDialogOverlay,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    picker::memory_dialog_content(m, styles)
}

pub(super) fn mcp_server_select_content(
    ms: &McpServerSelectOverlay,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    picker::mcp_server_select_content(ms, styles)
}
