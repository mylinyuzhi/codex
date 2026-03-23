//! Theme-aware unified diff rendering.
//!
//! Renders unified diff text with color-coded add/delete lines, adapting
//! to terminal capabilities (TrueColor, 256-color, ANSI-16).

use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

use crate::theme::Theme;

// ========== Diff Palette Constants ==========

// TrueColor palettes (dark terminal)
const DARK_TC_ADD_BG: (u8, u8, u8) = (33, 58, 43); // #213A2B
const DARK_TC_DEL_BG: (u8, u8, u8) = (74, 34, 29); // #4A221D

// TrueColor palettes (light terminal)
const LIGHT_TC_ADD_BG: (u8, u8, u8) = (218, 251, 225); // #dafbe1
const LIGHT_TC_DEL_BG: (u8, u8, u8) = (255, 235, 233); // #ffebe9

// 256-color palettes (dark terminal)
const DARK_256_ADD_BG: u8 = 22;
const DARK_256_DEL_BG: u8 = 52;

// 256-color palettes (light terminal)
const LIGHT_256_ADD_BG: u8 = 194;
const LIGHT_256_DEL_BG: u8 = 224;

/// Color palette for diff rendering.
struct DiffPalette {
    add_style: Style,
    del_style: Style,
    hunk_style: Style,
    context_style: Style,
}

impl DiffPalette {
    /// Select palette based on terminal capabilities.
    fn detect(theme: &Theme) -> Self {
        let has_truecolor = crate::terminal_palette::has_truecolor();
        let is_light = crate::terminal_palette::detect_bg()
            .map(crate::terminal_palette::is_light)
            .unwrap_or(false);

        if has_truecolor {
            Self::truecolor(is_light)
        } else if crate::terminal_palette::has_256_colors() {
            Self::palette_256(is_light)
        } else {
            Self::ansi16(theme)
        }
    }

    #[allow(clippy::disallowed_methods)] // Intentional RGB for diff backgrounds
    fn truecolor(is_light: bool) -> Self {
        let (add_bg, del_bg) = if is_light {
            (LIGHT_TC_ADD_BG, LIGHT_TC_DEL_BG)
        } else {
            (DARK_TC_ADD_BG, DARK_TC_DEL_BG)
        };
        Self {
            add_style: Style::default()
                .fg(Color::Green)
                .bg(Color::Rgb(add_bg.0, add_bg.1, add_bg.2)),
            del_style: Style::default()
                .fg(Color::Red)
                .bg(Color::Rgb(del_bg.0, del_bg.1, del_bg.2)),
            hunk_style: Style::default().fg(Color::Cyan).italic(),
            context_style: Style::default().fg(Color::DarkGray),
        }
    }

    #[allow(clippy::disallowed_methods)] // Intentional 256-color indexed palette for diff backgrounds
    fn palette_256(is_light: bool) -> Self {
        let (add_bg, del_bg) = if is_light {
            (LIGHT_256_ADD_BG, LIGHT_256_DEL_BG)
        } else {
            (DARK_256_ADD_BG, DARK_256_DEL_BG)
        };
        Self {
            add_style: Style::default().fg(Color::Green).bg(Color::Indexed(add_bg)),
            del_style: Style::default().fg(Color::Red).bg(Color::Indexed(del_bg)),
            hunk_style: Style::default().fg(Color::Cyan).italic(),
            context_style: Style::default().fg(Color::DarkGray),
        }
    }

    /// ANSI-16: foreground-only (backgrounds too saturated).
    fn ansi16(theme: &Theme) -> Self {
        Self {
            add_style: Style::default().fg(theme.success),
            del_style: Style::default().fg(theme.error),
            hunk_style: Style::default().fg(theme.primary).italic(),
            context_style: Style::default().fg(theme.text_dim),
        }
    }
}

// ========== Public API ==========

/// Render a unified diff string to styled Lines.
///
/// Lines are styled based on their diff prefix:
/// - `+` → add (green background)
/// - `-` → delete (red background)
/// - `@@` → hunk header (cyan, italic)
/// - ` ` or other → context (dim)
pub(crate) fn render_diff_lines(diff_text: &str, theme: &Theme, _width: u16) -> Vec<Line<'static>> {
    if diff_text.is_empty() {
        return vec![];
    }

    let palette = DiffPalette::detect(theme);
    let mut lines = Vec::new();

    for raw_line in diff_text.lines() {
        let line = render_diff_line(raw_line, &palette);
        lines.push(line);
    }

    lines
}

/// Render a single diff line with the appropriate style.
fn render_diff_line(raw_line: &str, palette: &DiffPalette) -> Line<'static> {
    if raw_line.starts_with("@@") {
        // Hunk header
        Line::from(Span::styled(format!("  │ {raw_line}"), palette.hunk_style))
    } else if let Some(rest) = raw_line.strip_prefix('+') {
        // Addition — but skip +++ file headers
        if rest.starts_with("++") {
            Line::from(Span::styled(format!("  │ {raw_line}"), palette.hunk_style))
        } else {
            Line::from(vec![
                Span::styled("  │ ", palette.context_style),
                Span::styled(format!("+{rest}"), palette.add_style),
            ])
        }
    } else if let Some(rest) = raw_line.strip_prefix('-') {
        // Deletion — but skip --- file headers
        if rest.starts_with("--") {
            Line::from(Span::styled(format!("  │ {raw_line}"), palette.hunk_style))
        } else {
            Line::from(vec![
                Span::styled("  │ ", palette.context_style),
                Span::styled(format!("-{rest}"), palette.del_style),
            ])
        }
    } else if raw_line.starts_with("diff ") || raw_line.starts_with("index ") {
        // Diff metadata header
        Line::from(Span::styled(format!("  │ {raw_line}"), palette.hunk_style))
    } else {
        // Context line
        let display = raw_line.strip_prefix(' ').unwrap_or(raw_line);
        Line::from(Span::styled(
            format!("  │  {display}"),
            palette.context_style,
        ))
    }
}

#[cfg(test)]
#[path = "diff_display.test.rs"]
mod tests;
