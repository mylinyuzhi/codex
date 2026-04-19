//! Theme system for TUI colors.
//!
//! Each theme provides a full color palette. Widgets reference theme
//! fields instead of hardcoding colors, enabling runtime theme switching.

use ratatui::style::Color;

/// Available theme names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeName {
    #[default]
    Default,
    Dark,
    Light,
    Dracula,
    Nord,
}

/// Complete color palette for TUI rendering.
///
/// Color discipline:
/// - Avoid Blue for text (hard to read on dark terminals)
/// - Avoid Yellow for backgrounds (invisible on light terminals)
/// - Never use `.white()` — prefer Reset (inherits terminal foreground)
#[derive(Debug, Clone)]
pub struct Theme {
    // ── Base ──
    pub primary: Color,
    pub secondary: Color,
    pub accent: Color,

    // ── Text ──
    pub text: Color,
    pub text_dim: Color,
    pub text_bold: Color,

    // ── Messages ──
    pub user_message: Color,
    /// Optional background tint applied to user-message lines.
    /// TS: components/messages/UserMessage draws a terminal-adaptive subtle
    /// tint so user prompts separate visually from assistant prose. None =
    /// inherit terminal background.
    pub user_message_bg: Option<Color>,
    pub assistant_message: Color,
    pub thinking: Color,
    pub system_message: Color,

    // ── Status ──
    pub tool_running: Color,
    pub tool_completed: Color,
    pub tool_error: Color,
    pub warning: Color,
    pub success: Color,
    pub error: Color,

    // ── UI Elements ──
    pub border: Color,
    pub border_focused: Color,
    pub scrollbar: Color,
    pub plan_mode: Color,

    // ── Diff ──
    pub diff_added: Color,
    pub diff_removed: Color,

    // ── Code highlighting ──
    pub code_keyword: Color,
    pub code_string: Color,
    pub code_comment: Color,
    pub code_number: Color,

    // ── Extended UI ──
    pub hyperlink: Color,
    pub table_border: Color,
    pub table_header: Color,
    pub search_match: Color,
    pub progress_bar: Color,
    pub context_used: Color,
    pub context_free: Color,
}

impl Theme {
    /// Get theme by name.
    pub fn from_name(name: ThemeName) -> Self {
        match name {
            ThemeName::Default => Self::default_theme(),
            ThemeName::Dark => Self::dark_theme(),
            ThemeName::Light => Self::light_theme(),
            ThemeName::Dracula => Self::dracula_theme(),
            ThemeName::Nord => Self::nord_theme(),
        }
    }

    fn default_theme() -> Self {
        Self {
            primary: Color::Cyan,
            secondary: Color::DarkGray,
            accent: Color::Magenta,

            text: Color::Reset,
            text_dim: Color::DarkGray,
            text_bold: Color::Reset,

            user_message: Color::Green,
            user_message_bg: None,
            assistant_message: Color::Cyan,
            thinking: Color::Magenta,
            system_message: Color::DarkGray,

            tool_running: Color::Yellow,
            tool_completed: Color::Green,
            tool_error: Color::Red,
            warning: Color::Yellow,
            success: Color::Green,
            error: Color::Red,

            border: Color::DarkGray,
            border_focused: Color::Cyan,
            scrollbar: Color::DarkGray,
            plan_mode: Color::Cyan,

            diff_added: Color::Green,
            diff_removed: Color::Red,

            code_keyword: Color::Magenta,
            code_string: Color::Green,
            code_comment: Color::DarkGray,
            code_number: Color::Cyan,

            hyperlink: Color::Cyan,
            table_border: Color::DarkGray,
            table_header: Color::Cyan,
            search_match: Color::Yellow,
            progress_bar: Color::Cyan,
            context_used: Color::Cyan,
            context_free: Color::DarkGray,
        }
    }

    fn dark_theme() -> Self {
        Self {
            primary: Color::LightCyan,
            secondary: Color::DarkGray,
            accent: Color::LightMagenta,

            text: Color::Reset,
            text_dim: Color::DarkGray,
            text_bold: Color::Reset,

            user_message: Color::LightGreen,
            user_message_bg: None,
            assistant_message: Color::LightCyan,
            thinking: Color::LightMagenta,
            system_message: Color::DarkGray,

            tool_running: Color::LightYellow,
            tool_completed: Color::LightGreen,
            tool_error: Color::LightRed,
            warning: Color::LightYellow,
            success: Color::LightGreen,
            error: Color::LightRed,

            border: Color::DarkGray,
            border_focused: Color::LightCyan,
            scrollbar: Color::DarkGray,
            plan_mode: Color::LightCyan,

            diff_added: Color::LightGreen,
            diff_removed: Color::LightRed,

            code_keyword: Color::LightMagenta,
            code_string: Color::LightGreen,
            code_comment: Color::DarkGray,
            code_number: Color::LightCyan,

            hyperlink: Color::LightCyan,
            table_border: Color::DarkGray,
            table_header: Color::LightCyan,
            search_match: Color::LightYellow,
            progress_bar: Color::LightCyan,
            context_used: Color::LightCyan,
            context_free: Color::DarkGray,
        }
    }

    fn light_theme() -> Self {
        Self {
            primary: Color::Cyan,
            secondary: Color::DarkGray,
            accent: Color::Magenta,

            text: Color::Reset,
            text_dim: Color::DarkGray,
            text_bold: Color::Reset,

            user_message: Color::Green,
            user_message_bg: None,
            assistant_message: Color::Cyan,
            thinking: Color::Magenta,
            system_message: Color::DarkGray,

            tool_running: Color::Yellow,
            tool_completed: Color::Green,
            tool_error: Color::Red,
            warning: Color::Yellow,
            success: Color::Green,
            error: Color::Red,

            border: Color::DarkGray,
            border_focused: Color::Cyan,
            scrollbar: Color::DarkGray,
            plan_mode: Color::Cyan,

            diff_added: Color::Green,
            diff_removed: Color::Red,

            code_keyword: Color::Magenta,
            code_string: Color::Green,
            code_comment: Color::DarkGray,
            code_number: Color::Cyan,

            hyperlink: Color::Cyan,
            table_border: Color::DarkGray,
            table_header: Color::Cyan,
            search_match: Color::Yellow,
            progress_bar: Color::Cyan,
            context_used: Color::Cyan,
            context_free: Color::DarkGray,
        }
    }

    fn dracula_theme() -> Self {
        Self {
            primary: Color::Rgb(139, 233, 253), // cyan
            secondary: Color::DarkGray,
            accent: Color::Rgb(255, 121, 198), // pink

            text: Color::Reset,
            text_dim: Color::DarkGray,
            text_bold: Color::Reset,

            user_message: Color::Rgb(80, 250, 123), // green
            user_message_bg: None,
            assistant_message: Color::Rgb(139, 233, 253),
            thinking: Color::Rgb(189, 147, 249), // purple
            system_message: Color::DarkGray,

            tool_running: Color::Rgb(241, 250, 140), // yellow
            tool_completed: Color::Rgb(80, 250, 123),
            tool_error: Color::Rgb(255, 85, 85), // red
            warning: Color::Rgb(241, 250, 140),
            success: Color::Rgb(80, 250, 123),
            error: Color::Rgb(255, 85, 85),

            border: Color::DarkGray,
            border_focused: Color::Rgb(139, 233, 253),
            scrollbar: Color::DarkGray,
            plan_mode: Color::Rgb(139, 233, 253),

            diff_added: Color::Rgb(80, 250, 123),
            diff_removed: Color::Rgb(255, 85, 85),

            code_keyword: Color::Rgb(255, 121, 198),
            code_string: Color::Rgb(241, 250, 140),
            code_comment: Color::Rgb(98, 114, 164),
            code_number: Color::Rgb(189, 147, 249),

            hyperlink: Color::Rgb(139, 233, 253),
            table_border: Color::Rgb(98, 114, 164),
            table_header: Color::Rgb(189, 147, 249),
            search_match: Color::Rgb(241, 250, 140),
            progress_bar: Color::Rgb(139, 233, 253),
            context_used: Color::Rgb(139, 233, 253),
            context_free: Color::Rgb(68, 71, 90),
        }
    }

    fn nord_theme() -> Self {
        Self {
            primary: Color::Rgb(136, 192, 208), // nord8
            secondary: Color::DarkGray,
            accent: Color::Rgb(180, 142, 173), // nord15

            text: Color::Reset,
            text_dim: Color::DarkGray,
            text_bold: Color::Reset,

            user_message: Color::Rgb(163, 190, 140), // nord14
            user_message_bg: None,
            assistant_message: Color::Rgb(136, 192, 208),
            thinking: Color::Rgb(180, 142, 173),
            system_message: Color::DarkGray,

            tool_running: Color::Rgb(235, 203, 139), // nord13
            tool_completed: Color::Rgb(163, 190, 140),
            tool_error: Color::Rgb(191, 97, 106), // nord11
            warning: Color::Rgb(235, 203, 139),
            success: Color::Rgb(163, 190, 140),
            error: Color::Rgb(191, 97, 106),

            border: Color::DarkGray,
            border_focused: Color::Rgb(136, 192, 208),
            scrollbar: Color::DarkGray,
            plan_mode: Color::Rgb(136, 192, 208),

            diff_added: Color::Rgb(163, 190, 140),
            diff_removed: Color::Rgb(191, 97, 106),

            code_keyword: Color::Rgb(180, 142, 173),
            code_string: Color::Rgb(163, 190, 140),
            code_comment: Color::Rgb(76, 86, 106),
            code_number: Color::Rgb(180, 142, 173),

            hyperlink: Color::Rgb(136, 192, 208),
            table_border: Color::Rgb(76, 86, 106),
            table_header: Color::Rgb(136, 192, 208),
            search_match: Color::Rgb(235, 203, 139),
            progress_bar: Color::Rgb(136, 192, 208),
            context_used: Color::Rgb(136, 192, 208),
            context_free: Color::Rgb(76, 86, 106),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::default_theme()
    }
}
