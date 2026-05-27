//! Filterable-list picker state content builders.

use ratatui::prelude::*;

use crate::presentation::model_picker;
use crate::presentation::picker;
use crate::presentation::styles::UiStyles;
use crate::state::AgentsDialogState;
use crate::state::CopyPickerState;
use crate::state::ExportState;
use crate::state::McpServerSelectState;
use crate::state::MemoryDialogState;
use crate::state::ModelPickerState;
use crate::state::QuickOpenState;
use crate::state::SessionBrowserState;
use crate::state::SkillsDialogState;
use crate::state::SubagentInstance;

pub(super) fn model_picker_content(
    m: &ModelPickerState,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    model_picker::content(m, styles)
}

pub(super) fn session_browser_content(
    s: &SessionBrowserState,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    picker::session_browser_content(s, styles)
}

pub(super) fn quick_open_content(
    q: &QuickOpenState,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    picker::quick_open_content(q, styles)
}

pub(super) fn export_content(e: &ExportState, styles: UiStyles<'_>) -> (String, String, Color) {
    picker::export_content(e, styles)
}

pub(super) fn memory_dialog_content(
    m: &MemoryDialogState,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    picker::memory_dialog_content(m, styles)
}

pub(super) fn skills_dialog_content(
    s: &SkillsDialogState,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    picker::skills_dialog_content(s, styles)
}

pub(super) fn agents_dialog_content(
    a: &AgentsDialogState,
    subagents: &[SubagentInstance],
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    picker::agents_dialog_content(a, subagents, styles)
}

pub(super) fn mcp_server_select_content(
    ms: &McpServerSelectState,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    picker::mcp_server_select_content(ms, styles)
}

pub(super) fn copy_picker_content(
    cp: &CopyPickerState,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    picker::copy_picker_content(cp, styles)
}
