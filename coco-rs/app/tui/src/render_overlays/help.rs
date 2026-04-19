//! Help overlay renderer — lists the spec keybindings.

use ratatui::prelude::Color;

use crate::i18n::t;
use crate::theme::Theme;

pub(super) fn help_content(theme: &Theme) -> (String, String, Color) {
    (
        t!("help.title").to_string(),
        [
            t!("help.body_line_tab"),
            t!("help.body_line_shift_tab"),
            t!("help.body_line_ctrl_t"),
            t!("help.body_line_ctrl_m"),
            t!("help.body_line_ctrl_c"),
            t!("help.body_line_ctrl_l"),
            t!("help.body_line_ctrl_k"),
            t!("help.body_line_ctrl_y"),
            t!("help.body_line_ctrl_e"),
            t!("help.body_line_ctrl_p"),
            t!("help.body_line_ctrl_s"),
            t!("help.body_line_ctrl_f"),
            t!("help.body_line_ctrl_shift_f"),
            t!("help.body_line_ctrl_o"),
            t!("help.body_line_ctrl_w"),
            t!("help.body_line_f6"),
            t!("help.body_line_ctrl_q"),
            t!("help.body_line_f1"),
            t!("help.body_line_esc"),
            t!("help.body_line_pageup"),
        ]
        .join("\n"),
        theme.primary,
    )
}
