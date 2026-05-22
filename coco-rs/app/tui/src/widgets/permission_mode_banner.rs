//! Permission-mode elevation banner.
//!
//! Shows when `SessionState::permission_mode` is in an elevated mode
//! (anything other than `Default`). Users need a persistent visual
//! reminder that prompts-for-confirm are disabled or relaxed because a
//! dropped-down modal won't stay visible across sessions.
//!
//! TS reference: src/components/BypassPermissionsModeDialog.tsx — the
//! explicit indicator while bypass/yolo mode is active.

use coco_types::PermissionMode;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::Widget;

use crate::i18n::t;
use crate::presentation::styles::UiStyles;
use crate::widgets::lifecycle_banner::render_banner_row;

pub struct PermissionModeBanner<'a> {
    mode: PermissionMode,
    styles: UiStyles<'a>,
}

impl<'a> PermissionModeBanner<'a> {
    pub fn new(mode: PermissionMode, styles: UiStyles<'a>) -> Self {
        Self { mode, styles }
    }

    /// Banner shows for every mode that isn't `Default`. Default mode
    /// means "ask for every elevated action" and needs no warning.
    pub fn should_display(mode: PermissionMode) -> bool {
        mode != PermissionMode::Default
    }
}

impl Widget for PermissionModeBanner<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let (label, blurb, color) = match self.mode {
            PermissionMode::AcceptEdits => (
                t!("permission_mode.accept_edits"),
                t!("permission_mode.accept_edits_desc"),
                self.styles.accent(),
            ),
            PermissionMode::Plan => (
                t!("permission_mode.plan"),
                t!("permission_mode.plan_desc"),
                self.styles.plan(),
            ),
            PermissionMode::BypassPermissions => (
                t!("permission_mode.bypass"),
                t!("permission_mode.bypass_desc"),
                self.styles.error(),
            ),
            PermissionMode::DontAsk => (
                t!("permission_mode.dont_ask"),
                t!("permission_mode.dont_ask_desc"),
                self.styles.warning(),
            ),
            PermissionMode::Auto => (
                t!("permission_mode.auto"),
                t!("permission_mode.auto_desc"),
                self.styles.accent(),
            ),
            PermissionMode::Bubble => (
                t!("permission_mode.bubble"),
                t!("permission_mode.bubble_desc"),
                self.styles.dim(),
            ),
            PermissionMode::Default => return, // caller should have filtered
        };
        let parts = vec![
            Span::styled(
                t!("permission_mode.banner_prefix").to_string(),
                Style::default().fg(color).bold(),
            ),
            Span::styled(label.to_string(), Style::default().fg(color).bold()),
            Span::styled(blurb.to_string(), Style::default().fg(self.styles.dim())),
        ];
        render_banner_row(parts, self.styles, area, buf);
    }
}

#[cfg(test)]
#[path = "permission_mode_banner.test.rs"]
mod tests;
