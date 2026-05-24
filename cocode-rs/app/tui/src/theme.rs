//! Theme system for the TUI.
//!
//! This module provides a theme system with 5 built-in themes:
//! - Default: Balanced colors for general use
//! - Dark: High contrast dark theme
//! - Light: Clean light theme
//! - Dracula: Popular dark purple theme
//! - Nord: Cool blue nordic theme

use ratatui::style::Color;

/// Available theme names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeName {
    /// Default balanced theme.
    #[default]
    Default,
    /// High contrast dark theme.
    Dark,
    /// Clean light theme.
    Light,
    /// Dracula purple theme.
    Dracula,
    /// Nord blue theme.
    Nord,
}

impl ThemeName {
    /// Get all available theme names.
    pub fn all() -> &'static [ThemeName] {
        &[
            ThemeName::Default,
            ThemeName::Dark,
            ThemeName::Light,
            ThemeName::Dracula,
            ThemeName::Nord,
        ]
    }

    /// Get theme name as a string.
    pub fn as_str(&self) -> &'static str {
        match self {
            ThemeName::Default => "default",
            ThemeName::Dark => "dark",
            ThemeName::Light => "light",
            ThemeName::Dracula => "dracula",
            ThemeName::Nord => "nord",
        }
    }

    /// Parse theme name from string.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<ThemeName> {
        match s.to_lowercase().as_str() {
            "default" => Some(ThemeName::Default),
            "dark" => Some(ThemeName::Dark),
            "light" => Some(ThemeName::Light),
            "dracula" => Some(ThemeName::Dracula),
            "nord" => Some(ThemeName::Nord),
            _ => None,
        }
    }
}

impl std::fmt::Display for ThemeName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Theme configuration for the TUI.
#[derive(Debug, Clone)]
pub struct Theme {
    /// Theme name.
    pub name: ThemeName,

    // ========== Base Colors ==========
    /// Primary accent color.
    pub primary: Color,
    /// Secondary accent color.
    pub secondary: Color,
    /// Tertiary/highlight color.
    pub accent: Color,
    /// Subtle background tint for user messages (terminal-adaptive, may be None).
    pub user_message_bg: Option<Color>,

    // ========== Text Colors ==========
    /// Normal text color.
    pub text: Color,
    /// Dimmed/muted text color.
    pub text_dim: Color,
    /// Bold/emphasized text color.
    pub text_bold: Color,

    // ========== Background Colors ==========
    /// Main background color.
    pub bg: Color,
    /// Secondary/elevated background.
    pub bg_secondary: Color,
    /// Selected/highlighted background.
    pub bg_selected: Color,

    // ========== Message Colors ==========
    /// User message color.
    pub user_message: Color,
    /// Assistant message color.
    pub assistant_message: Color,
    /// Thinking content color.
    pub thinking: Color,
    /// System message color.
    pub system_message: Color,

    // ========== Status Colors ==========
    /// Tool running indicator.
    pub tool_running: Color,
    /// Tool completed indicator.
    pub tool_completed: Color,
    /// Tool error indicator.
    pub tool_error: Color,
    /// Warning color.
    pub warning: Color,
    /// Success color.
    pub success: Color,
    /// Error color.
    pub error: Color,

    // ========== UI Element Colors ==========
    /// Border color.
    pub border: Color,
    /// Border focused color.
    pub border_focused: Color,
    /// Scrollbar color.
    pub scrollbar: Color,
    /// Plan mode indicator.
    pub plan_mode: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self::default_theme()
    }
}

impl Theme {
    /// Create the default theme.
    ///
    /// Follows strict color discipline: only {Reset, Cyan, Green, Red, Magenta, DarkGray}.
    /// Avoids Blue (hard to read on dark terminals) and Yellow (invisible on light terminals).
    pub fn default_theme() -> Self {
        Self {
            name: ThemeName::Default,
            // Base
            primary: Color::Cyan,
            secondary: Color::Cyan,
            accent: Color::Magenta,
            user_message_bg: crate::terminal_palette::user_message_bg(),
            // Text
            text: Color::Reset,
            text_dim: Color::DarkGray,
            text_bold: Color::Reset,
            // Background
            bg: Color::Reset,
            bg_secondary: Color::DarkGray,
            bg_selected: Color::DarkGray,
            // Messages
            user_message: Color::Green,
            assistant_message: Color::Cyan,
            thinking: Color::Magenta,
            system_message: Color::DarkGray,
            // Status
            tool_running: Color::Magenta,
            tool_completed: Color::Green,
            tool_error: Color::Red,
            warning: Color::Red,
            success: Color::Green,
            error: Color::Red,
            // UI
            border: Color::DarkGray,
            border_focused: Color::Cyan,
            scrollbar: Color::DarkGray,
            plan_mode: Color::Magenta,
        }
    }

    /// Create the dark theme.
    pub fn dark() -> Self {
        Self {
            name: ThemeName::Dark,
            // Base
            primary: Color::LightCyan,
            secondary: Color::LightCyan,
            accent: Color::LightMagenta,
            user_message_bg: crate::terminal_palette::user_message_bg(),
            // Text
            text: Color::Gray,
            text_dim: Color::DarkGray,
            text_bold: Color::Reset,
            // Background
            bg: Color::Reset,
            bg_secondary: Color::DarkGray,
            bg_selected: Color::DarkGray,
            // Messages
            user_message: Color::LightGreen,
            assistant_message: Color::LightCyan,
            thinking: Color::LightMagenta,
            system_message: Color::DarkGray,
            // Status
            tool_running: Color::LightMagenta,
            tool_completed: Color::Green,
            tool_error: Color::LightRed,
            warning: Color::LightRed,
            success: Color::Green,
            error: Color::LightRed,
            // UI
            border: Color::DarkGray,
            border_focused: Color::LightCyan,
            scrollbar: Color::DarkGray,
            plan_mode: Color::LightMagenta,
        }
    }

    /// Create the light theme.
    pub fn light() -> Self {
        Self {
            name: ThemeName::Light,
            // Base
            primary: Color::Cyan,
            secondary: Color::Cyan,
            accent: Color::Magenta,
            user_message_bg: crate::terminal_palette::user_message_bg(),
            // Text
            text: Color::Reset,
            text_dim: Color::DarkGray,
            text_bold: Color::Reset,
            // Background
            bg: Color::Reset,
            bg_secondary: Color::Gray,
            bg_selected: Color::Gray,
            // Messages
            user_message: Color::Green,
            assistant_message: Color::Cyan,
            thinking: Color::Magenta,
            system_message: Color::DarkGray,
            // Status
            tool_running: Color::Magenta,
            tool_completed: Color::Green,
            tool_error: Color::Red,
            warning: Color::Red,
            success: Color::Green,
            error: Color::Red,
            // UI
            border: Color::Gray,
            border_focused: Color::Cyan,
            scrollbar: Color::DarkGray,
            plan_mode: Color::Magenta,
        }
    }

    /// Create the Dracula theme.
    pub fn dracula() -> Self {
        Self {
            name: ThemeName::Dracula,
            // Base (Dracula palette)
            primary: Color::LightCyan,
            secondary: Color::LightMagenta,
            accent: Color::Magenta,
            user_message_bg: crate::terminal_palette::user_message_bg(),
            // Text
            text: Color::Gray,
            text_dim: Color::DarkGray,
            text_bold: Color::Reset,
            // Background
            bg: Color::Reset,
            bg_secondary: Color::DarkGray,
            bg_selected: Color::DarkGray,
            // Messages
            user_message: Color::LightGreen,
            assistant_message: Color::LightCyan,
            thinking: Color::LightMagenta,
            system_message: Color::LightRed,
            // Status
            tool_running: Color::LightRed,
            tool_completed: Color::LightGreen,
            tool_error: Color::LightRed,
            warning: Color::LightRed,
            success: Color::LightGreen,
            error: Color::LightRed,
            // UI
            border: Color::DarkGray,
            border_focused: Color::LightMagenta,
            scrollbar: Color::DarkGray,
            plan_mode: Color::LightMagenta,
        }
    }

    /// Create the Nord theme.
    pub fn nord() -> Self {
        Self {
            name: ThemeName::Nord,
            // Base (Nord palette)
            primary: Color::LightCyan,
            secondary: Color::LightCyan,
            accent: Color::LightMagenta,
            user_message_bg: crate::terminal_palette::user_message_bg(),
            // Text
            text: Color::Gray,
            text_dim: Color::DarkGray,
            text_bold: Color::Gray,
            // Background
            bg: Color::Reset,
            bg_secondary: Color::DarkGray,
            bg_selected: Color::DarkGray,
            // Messages
            user_message: Color::LightGreen,
            assistant_message: Color::LightCyan,
            thinking: Color::LightMagenta,
            system_message: Color::DarkGray,
            // Status
            tool_running: Color::LightMagenta,
            tool_completed: Color::LightGreen,
            tool_error: Color::Red,
            warning: Color::LightRed,
            success: Color::LightGreen,
            error: Color::Red,
            // UI
            border: Color::DarkGray,
            border_focused: Color::LightCyan,
            scrollbar: Color::DarkGray,
            plan_mode: Color::LightMagenta,
        }
    }

    /// Get a theme by name.
    pub fn by_name(name: ThemeName) -> Self {
        match name {
            ThemeName::Default => Self::default_theme(),
            ThemeName::Dark => Self::dark(),
            ThemeName::Light => Self::light(),
            ThemeName::Dracula => Self::dracula(),
            ThemeName::Nord => Self::nord(),
        }
    }
}

#[cfg(test)]
#[path = "theme.test.rs"]
mod tests;
