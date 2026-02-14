//! Lightweight terminal markdown renderer.
//!
//! Converts markdown text to styled ratatui `Line`s for display in the terminal.
//! Supports: bold, italic, inline code, code blocks, headers, lists, blockquotes.

use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

use crate::theme::Theme;

/// Convert markdown text to styled lines for terminal rendering.
///
/// Parses markdown line-by-line (not a full AST parser) and applies
/// appropriate styling using theme colors.
pub fn markdown_to_lines(text: &str, theme: &Theme, _width: u16) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut in_code_block = false;
    let mut code_block_lang = String::new();

    for raw_line in text.lines() {
        if raw_line.starts_with("```") {
            if in_code_block {
                // End of code block
                lines.push(Line::from(
                    Span::raw("  └────────────────────────────────").fg(theme.border),
                ));
                in_code_block = false;
                code_block_lang.clear();
            } else {
                // Start of code block
                in_code_block = true;
                code_block_lang = raw_line.trim_start_matches('`').trim().to_string();
                let label = if code_block_lang.is_empty() {
                    "  ┌────────────────────────────────".to_string()
                } else {
                    format!("  ┌─ {code_block_lang} ─────────────────────────")
                };
                lines.push(Line::from(Span::raw(label).fg(theme.border)));
            }
            continue;
        }

        if in_code_block {
            // Inside code block: render with dim styling and border
            lines.push(Line::from(vec![
                Span::raw("  │ ").fg(theme.border),
                Span::raw(raw_line.to_string()).fg(theme.text_dim),
            ]));
            continue;
        }

        // Headers
        if let Some(rest) = raw_line.strip_prefix("### ") {
            lines.push(Line::from(
                Span::raw(format!("  {rest}"))
                    .bold()
                    .underlined()
                    .fg(theme.primary),
            ));
            continue;
        }
        if let Some(rest) = raw_line.strip_prefix("## ") {
            lines.push(Line::from(
                Span::raw(format!("  {rest}"))
                    .bold()
                    .underlined()
                    .fg(theme.primary),
            ));
            continue;
        }
        if let Some(rest) = raw_line.strip_prefix("# ") {
            lines.push(Line::from(
                Span::raw(format!("  {rest}"))
                    .bold()
                    .underlined()
                    .fg(theme.primary),
            ));
            continue;
        }

        // Blockquotes
        if let Some(rest) = raw_line.strip_prefix("> ") {
            lines.push(Line::from(vec![
                Span::raw("  │ ").fg(theme.text_dim),
                Span::raw(rest.to_string()).fg(theme.text_dim).italic(),
            ]));
            continue;
        }
        if raw_line == ">" {
            lines.push(Line::from(Span::raw("  │").fg(theme.text_dim)));
            continue;
        }

        // List items
        if let Some(rest) = raw_line.strip_prefix("- ") {
            let styled = parse_inline_styles(rest, theme);
            let mut spans = vec![Span::raw("  • ")];
            spans.extend(styled);
            lines.push(Line::from(spans));
            continue;
        }
        if let Some(rest) = raw_line.strip_prefix("* ") {
            let styled = parse_inline_styles(rest, theme);
            let mut spans = vec![Span::raw("  • ")];
            spans.extend(styled);
            lines.push(Line::from(spans));
            continue;
        }
        // Numbered list items
        if raw_line.len() > 2 {
            let trimmed = raw_line.trim_start();
            if let Some(dot_pos) = trimmed.find(". ") {
                if dot_pos <= 3 && trimmed[..dot_pos].chars().all(|c| c.is_ascii_digit()) {
                    let num = &trimmed[..dot_pos];
                    let rest = &trimmed[dot_pos + 2..];
                    let styled = parse_inline_styles(rest, theme);
                    let indent = "  ".repeat(1 + (raw_line.len() - trimmed.len()) / 2);
                    let mut spans = vec![Span::raw(format!("{indent}{num}. "))];
                    spans.extend(styled);
                    lines.push(Line::from(spans));
                    continue;
                }
            }
        }

        // Regular text with inline styles
        if raw_line.is_empty() {
            lines.push(Line::from(""));
        } else {
            let styled = parse_inline_styles(raw_line, theme);
            let mut spans = vec![Span::raw("  ")];
            spans.extend(styled);
            lines.push(Line::from(spans));
        }
    }

    // Close unclosed code block
    if in_code_block {
        lines.push(Line::from(
            Span::raw("  └────────────────────────────────").fg(theme.border),
        ));
    }

    lines
}

/// Parse inline markdown styles (bold, italic, code) within a line.
fn parse_inline_styles(text: &str, theme: &Theme) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let c = chars[i];

        // Inline code: `text`
        if c == '`' && !matches!(chars.get(i + 1), Some('`')) {
            // Flush current text
            if !current.is_empty() {
                spans.push(Span::raw(std::mem::take(&mut current)));
            }
            // Find closing backtick
            let start = i + 1;
            i = start;
            while i < len && chars[i] != '`' {
                current.push(chars[i]);
                i += 1;
            }
            spans.push(Span::styled(
                std::mem::take(&mut current),
                Style::default().fg(theme.primary),
            ));
            if i < len {
                i += 1; // skip closing `
            }
            continue;
        }

        // Bold: **text**
        if c == '*' && matches!(chars.get(i + 1), Some('*')) {
            // Flush current text
            if !current.is_empty() {
                spans.push(Span::raw(std::mem::take(&mut current)));
            }
            // Find closing **
            let start = i + 2;
            i = start;
            while i < len {
                if chars[i] == '*' && matches!(chars.get(i + 1), Some('*')) {
                    break;
                }
                current.push(chars[i]);
                i += 1;
            }
            spans.push(Span::raw(std::mem::take(&mut current)).bold());
            if i < len {
                i += 2; // skip closing **
            }
            continue;
        }

        // Italic: *text*
        if c == '*' {
            // Flush current text
            if !current.is_empty() {
                spans.push(Span::raw(std::mem::take(&mut current)));
            }
            // Find closing *
            let start = i + 1;
            i = start;
            while i < len && chars[i] != '*' {
                current.push(chars[i]);
                i += 1;
            }
            spans.push(Span::raw(std::mem::take(&mut current)).italic());
            if i < len {
                i += 1; // skip closing *
            }
            continue;
        }

        current.push(c);
        i += 1;
    }

    // Flush remaining text
    if !current.is_empty() {
        spans.push(Span::raw(current));
    }

    spans
}

#[cfg(test)]
#[path = "markdown.test.rs"]
mod tests;
