//! Lightweight terminal markdown renderer.
//!
//! Converts markdown text to styled ratatui `Line`s.
//! Supports: bold, italic, inline code, code blocks (with syntax highlighting),
//! headers, lists, blockquotes, links, HR, and tables.

use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

use crate::constants::TABLE_MAX_COL_WIDTH;
use crate::constants::TABLE_MIN_COL_WIDTH;
use crate::theme::Theme;

/// Convert markdown text to styled lines for terminal rendering.
pub fn markdown_to_lines(text: &str, theme: &Theme, width: u16) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut in_code_block = false;
    let mut code_lang = String::new();
    let border_len = (width as usize).saturating_sub(4).min(60);
    let raw_lines: Vec<&str> = text.lines().collect();
    let mut idx: usize = 0;

    while idx < raw_lines.len() {
        let raw_line = raw_lines[idx];

        // Code block fences
        if raw_line.starts_with("```") {
            if in_code_block {
                lines.push(Line::from(
                    Span::raw(format!("  \u{2514}{}", "\u{2500}".repeat(border_len)))
                        .fg(theme.border),
                ));
                in_code_block = false;
                code_lang.clear();
            } else {
                in_code_block = true;
                code_lang = raw_line.trim_start_matches('`').trim().to_string();
                let label = if code_lang.is_empty() {
                    format!("  \u{250c}{}", "\u{2500}".repeat(border_len))
                } else {
                    let fill = border_len.saturating_sub(code_lang.len() + 3);
                    format!("  \u{250c}\u{2500} {code_lang} {}", "\u{2500}".repeat(fill))
                };
                lines.push(Line::from(Span::raw(label).fg(theme.border)));
            }
            idx += 1;
            continue;
        }

        if in_code_block {
            let spans = highlight_code_line(raw_line, &code_lang, theme);
            let mut full = vec![Span::raw("  \u{2502} ").fg(theme.border)];
            full.extend(spans);
            lines.push(Line::from(full));
            idx += 1;
            continue;
        }

        // Table detection: line contains `|` and next line is a separator row
        if raw_line.contains('|') && is_table_start(raw_line, raw_lines.get(idx + 1).copied()) {
            let table_end = find_table_end(&raw_lines, idx);
            let table_lines = &raw_lines[idx..table_end];
            render_table(table_lines, theme, &mut lines);
            idx = table_end;
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
            idx += 1;
            continue;
        }
        if let Some(rest) = raw_line.strip_prefix("## ") {
            lines.push(Line::from(
                Span::raw(format!("  {rest}"))
                    .bold()
                    .underlined()
                    .fg(theme.primary),
            ));
            idx += 1;
            continue;
        }
        if let Some(rest) = raw_line.strip_prefix("# ") {
            lines.push(Line::from(
                Span::raw(format!("  {rest}"))
                    .bold()
                    .underlined()
                    .fg(theme.primary),
            ));
            idx += 1;
            continue;
        }

        // Horizontal rule
        if matches!(raw_line, "---" | "***" | "___") {
            lines.push(Line::from(
                Span::raw(format!("  {}", "\u{2500}".repeat(border_len))).fg(theme.border),
            ));
            idx += 1;
            continue;
        }

        // Blockquotes (nested: count leading `> ` prefixes)
        if raw_line.starts_with('>') {
            let mut depth = 0;
            let mut rest = raw_line;
            while let Some(stripped) = rest.strip_prefix("> ") {
                depth += 1;
                rest = stripped;
            }
            // Handle bare `>` at any level
            if rest.starts_with('>') {
                depth += 1;
                rest = rest.strip_prefix('>').unwrap_or("");
            }
            let prefix: String = "  \u{2502} ".repeat(depth);
            if rest.is_empty() {
                lines.push(Line::from(
                    Span::raw(format!("  {}", "\u{2502} ".repeat(depth).trim_end()))
                        .fg(theme.text_dim),
                ));
            } else {
                let styled = parse_inline_styles(rest.trim(), theme);
                let mut spans = vec![Span::raw(prefix).fg(theme.text_dim)];
                spans.extend(styled.into_iter().map(|s| s.italic()));
                lines.push(Line::from(spans));
            }
            idx += 1;
            continue;
        }

        // Task list items (- [ ] unchecked, - [x] checked)
        if let Some(rest) = raw_line
            .strip_prefix("- [x] ")
            .or_else(|| raw_line.strip_prefix("- [X] "))
        {
            let styled = parse_inline_styles(rest, theme);
            let mut spans = vec![
                Span::raw("  ").fg(theme.success),
                Span::raw("☑ ").fg(theme.success),
            ];
            spans.extend(styled);
            lines.push(Line::from(spans));
            idx += 1;
            continue;
        }
        if let Some(rest) = raw_line.strip_prefix("- [ ] ") {
            let styled = parse_inline_styles(rest, theme);
            let mut spans = vec![
                Span::raw("  ").fg(theme.text_dim),
                Span::raw("☐ ").fg(theme.text_dim),
            ];
            spans.extend(styled);
            lines.push(Line::from(spans));
            idx += 1;
            continue;
        }

        // Unordered list items
        if let Some(rest) = raw_line
            .strip_prefix("- ")
            .or_else(|| raw_line.strip_prefix("* "))
        {
            let styled = parse_inline_styles(rest, theme);
            let mut spans = vec![Span::raw("  \u{2022} ")];
            spans.extend(styled);
            lines.push(Line::from(spans));
            idx += 1;
            continue;
        }

        // Numbered list items
        if let Some(pos) = raw_line.find(". ") {
            let prefix = &raw_line[..pos];
            if prefix.chars().all(|c| c.is_ascii_digit()) && !prefix.is_empty() {
                let rest = &raw_line[pos + 2..];
                let styled = parse_inline_styles(rest, theme);
                let mut spans = vec![Span::raw(format!("  {prefix}. "))];
                spans.extend(styled);
                lines.push(Line::from(spans));
                idx += 1;
                continue;
            }
        }

        // Empty line
        if raw_line.is_empty() {
            lines.push(Line::default());
            idx += 1;
            continue;
        }

        // Regular paragraph with inline styles
        let styled = parse_inline_styles(raw_line, theme);
        let mut spans = vec![Span::raw("  ")];
        spans.extend(styled);
        lines.push(Line::from(spans));
        idx += 1;
    }

    lines
}

// ── Table rendering ─────────────────────────────────────────────────

/// Check if the current line and next line form a table header + separator.
fn is_table_start(line: &str, next: Option<&str>) -> bool {
    let Some(next_line) = next else {
        return false;
    };
    is_table_separator(next_line) && line.contains('|')
}

/// Check if a line is a table separator row (e.g. `| --- | --- |` or `|---|---|`).
fn is_table_separator(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.contains('|') {
        return false;
    }
    trimmed.split('|').filter(|s| !s.is_empty()).all(|cell| {
        let c = cell.trim();
        !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':' || ch == ' ')
    })
}

/// Find the end index (exclusive) of a contiguous table block.
fn find_table_end(raw_lines: &[&str], start: usize) -> usize {
    let mut end = start;
    while end < raw_lines.len() && raw_lines[end].contains('|') {
        end += 1;
    }
    end
}

/// Parse a table row into cells, stripping leading/trailing pipes.
fn parse_table_row(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let stripped = trimmed
        .strip_prefix('|')
        .unwrap_or(trimmed)
        .strip_suffix('|')
        .unwrap_or(trimmed);
    stripped
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

/// Render a markdown table with box-drawing borders.
fn render_table(table_lines: &[&str], theme: &Theme, out: &mut Vec<Line<'static>>) {
    if table_lines.len() < 2 {
        return;
    }

    // Parse all data rows (skip separator at index 1)
    let header_cells = parse_table_row(table_lines[0]);
    let mut body_rows: Vec<Vec<String>> = Vec::new();
    for line in table_lines.iter().skip(2) {
        if is_table_separator(line) {
            continue;
        }
        body_rows.push(parse_table_row(line));
    }

    let col_count = header_cells.len();
    if col_count == 0 {
        return;
    }

    // Compute column widths from content, clamped to min/max
    let min_w = TABLE_MIN_COL_WIDTH;
    let max_w = TABLE_MAX_COL_WIDTH;
    let mut col_widths: Vec<i32> = header_cells
        .iter()
        .map(|c| (c.len() as i32).clamp(min_w, max_w))
        .collect();
    for row in &body_rows {
        for (i, cell) in row.iter().enumerate() {
            if i < col_widths.len() {
                col_widths[i] = col_widths[i].max((cell.len() as i32).clamp(min_w, max_w));
            }
        }
    }

    // Top border: ┌───┬───┐
    out.push(table_border_line(
        "\u{250c}",
        "\u{252c}",
        "\u{2510}",
        &col_widths,
        theme,
    ));

    // Header row
    out.push(table_data_line(
        &header_cells,
        &col_widths,
        theme,
        /*is_header*/ true,
    ));

    // Header separator: ├───┼───┤
    out.push(table_border_line(
        "\u{251c}",
        "\u{253c}",
        "\u{2524}",
        &col_widths,
        theme,
    ));

    // Body rows
    for row in &body_rows {
        out.push(table_data_line(
            row,
            &col_widths,
            theme,
            /*is_header*/ false,
        ));
    }

    // Bottom border: └───┴───┘
    out.push(table_border_line(
        "\u{2514}",
        "\u{2534}",
        "\u{2518}",
        &col_widths,
        theme,
    ));
}

/// Build a horizontal border line with given corner/junction characters.
fn table_border_line(
    left: &str,
    mid: &str,
    right: &str,
    col_widths: &[i32],
    theme: &Theme,
) -> Line<'static> {
    let mut s = format!("  {left}");
    for (i, &w) in col_widths.iter().enumerate() {
        s.push_str(&"\u{2500}".repeat(w as usize + 2));
        if i + 1 < col_widths.len() {
            s.push_str(mid);
        }
    }
    s.push_str(right);
    Line::from(Span::raw(s).fg(theme.table_border))
}

/// Build a data row with cell contents padded to column widths.
fn table_data_line(
    cells: &[String],
    col_widths: &[i32],
    theme: &Theme,
    is_header: bool,
) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::raw("  ").fg(theme.table_border));
    spans.push(Span::raw("\u{2502}").fg(theme.table_border));

    for (i, width) in col_widths.iter().enumerate() {
        let w = *width as usize;
        let content = cells.get(i).map_or("", String::as_str);
        let truncated = if content.len() > w {
            &content[..w]
        } else {
            content
        };
        let padded = format!(" {truncated:<w$} ");
        if is_header {
            spans.push(Span::raw(padded).fg(theme.table_header).bold());
        } else {
            spans.push(Span::raw(padded));
        }
        spans.push(Span::raw("\u{2502}").fg(theme.table_border));
    }

    Line::from(spans)
}

// ── Syntax highlighting ─────────────────────────────────────────────

/// Keyword sets per language for basic highlighting.
fn keywords_for_lang(lang: &str) -> &'static [&'static str] {
    match lang {
        "rust" | "rs" => &[
            "fn", "let", "mut", "const", "pub", "use", "mod", "struct", "enum", "impl", "trait",
            "for", "while", "loop", "if", "else", "match", "return", "self", "Self", "super",
            "crate", "where", "async", "await", "move", "ref", "type", "true", "false", "as", "in",
            "dyn", "static", "unsafe",
        ],
        "python" | "py" => &[
            "def", "class", "if", "elif", "else", "for", "while", "return", "import", "from", "as",
            "with", "try", "except", "finally", "raise", "yield", "lambda", "pass", "break",
            "continue", "and", "or", "not", "in", "is", "None", "True", "False", "async", "await",
            "self",
        ],
        "javascript" | "js" | "typescript" | "ts" | "tsx" | "jsx" => &[
            "function",
            "const",
            "let",
            "var",
            "if",
            "else",
            "for",
            "while",
            "return",
            "class",
            "new",
            "this",
            "import",
            "export",
            "from",
            "async",
            "await",
            "try",
            "catch",
            "throw",
            "typeof",
            "instanceof",
            "true",
            "false",
            "null",
            "undefined",
            "switch",
            "case",
            "default",
            "break",
            "continue",
            "yield",
            "of",
            "in",
            "type",
            "interface",
            "enum",
        ],
        "go" | "golang" => &[
            "func",
            "var",
            "const",
            "type",
            "struct",
            "interface",
            "map",
            "chan",
            "go",
            "select",
            "case",
            "default",
            "if",
            "else",
            "for",
            "range",
            "return",
            "break",
            "continue",
            "switch",
            "package",
            "import",
            "defer",
            "nil",
            "true",
            "false",
            "make",
            "len",
            "append",
        ],
        "bash" | "sh" | "zsh" | "shell" => &[
            "if", "then", "else", "elif", "fi", "for", "while", "do", "done", "case", "esac",
            "function", "return", "local", "export", "source", "echo", "exit", "set", "unset",
            "true", "false", "in",
        ],
        _ => &[],
    }
}

/// Apply basic syntax highlighting to a single code line.
fn highlight_code_line(line: &str, lang: &str, theme: &Theme) -> Vec<Span<'static>> {
    let keywords = keywords_for_lang(lang);
    if keywords.is_empty() {
        return vec![Span::raw(line.to_string()).fg(theme.text_dim)];
    }

    let mut spans: Vec<Span<'static>> = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut pos: usize = 0;

    while pos < len {
        let ch = chars[pos];

        // Line comment: // or #
        if (ch == '/' && pos + 1 < len && chars[pos + 1] == '/')
            || (ch == '#' && matches!(lang, "python" | "py" | "bash" | "sh" | "zsh" | "shell"))
        {
            let rest: String = chars[pos..].iter().collect();
            spans.push(Span::raw(rest).fg(theme.code_comment).italic());
            break;
        }

        // String literals: "..." or '...'
        if ch == '"' || ch == '\'' {
            let quote = ch;
            let start = pos;
            pos += 1;
            while pos < len && chars[pos] != quote {
                if chars[pos] == '\\' {
                    pos += 1; // skip escaped char
                }
                pos += 1;
            }
            if pos < len {
                pos += 1; // closing quote
            }
            let s: String = chars[start..pos].iter().collect();
            spans.push(Span::raw(s).fg(theme.code_string));
            continue;
        }

        // Numbers
        if ch.is_ascii_digit() {
            let start = pos;
            while pos < len
                && (chars[pos].is_ascii_alphanumeric() || chars[pos] == '.' || chars[pos] == '_')
            {
                pos += 1;
            }
            let s: String = chars[start..pos].iter().collect();
            spans.push(Span::raw(s).fg(theme.code_number));
            continue;
        }

        // Identifiers / keywords
        if ch.is_alphabetic() || ch == '_' {
            let start = pos;
            while pos < len && (chars[pos].is_alphanumeric() || chars[pos] == '_') {
                pos += 1;
            }
            let word: String = chars[start..pos].iter().collect();
            if keywords.contains(&word.as_str()) {
                spans.push(Span::raw(word).fg(theme.code_keyword).bold());
            } else {
                spans.push(Span::raw(word).fg(theme.text_dim));
            }
            continue;
        }

        // Other characters (operators, whitespace, punctuation)
        spans.push(Span::raw(ch.to_string()).fg(theme.text_dim));
        pos += 1;
    }

    spans
}

// ── Inline style parsing ────────────────────────────────────────────

/// Parse inline markdown styles: **bold**, *italic*, `code`, [links](url).
fn parse_inline_styles(text: &str, theme: &Theme) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '`' => {
                if !current.is_empty() {
                    spans.push(Span::raw(current.clone()));
                    current.clear();
                }
                let mut code = String::new();
                for next in chars.by_ref() {
                    if next == '`' {
                        break;
                    }
                    code.push(next);
                }
                spans.push(Span::raw(code).fg(theme.accent));
            }
            '~' if chars.peek() == Some(&'~') => {
                chars.next(); // consume second ~
                if !current.is_empty() {
                    spans.push(Span::raw(current.clone()));
                    current.clear();
                }
                let mut strike_text = String::new();
                while let Some(&next) = chars.peek() {
                    if next == '~' {
                        chars.next();
                        if chars.peek() == Some(&'~') {
                            chars.next();
                        }
                        break;
                    }
                    strike_text.push(next);
                    chars.next();
                }
                spans.push(Span::raw(strike_text).fg(theme.text_dim).dim());
            }
            '*' if chars.peek() == Some(&'*') => {
                chars.next(); // consume second *
                if !current.is_empty() {
                    spans.push(Span::raw(current.clone()));
                    current.clear();
                }
                let mut bold_text = String::new();
                while let Some(&next) = chars.peek() {
                    if next == '*' {
                        chars.next();
                        if chars.peek() == Some(&'*') {
                            chars.next();
                        }
                        break;
                    }
                    bold_text.push(next);
                    chars.next();
                }
                spans.push(Span::raw(bold_text).bold());
            }
            '*' | '_' => {
                if !current.is_empty() {
                    spans.push(Span::raw(current.clone()));
                    current.clear();
                }
                let end_char = c;
                let mut italic_text = String::new();
                for next in chars.by_ref() {
                    if next == end_char {
                        break;
                    }
                    italic_text.push(next);
                }
                spans.push(Span::raw(italic_text).italic());
            }
            '[' => {
                if !current.is_empty() {
                    spans.push(Span::raw(current.clone()));
                    current.clear();
                }
                let mut link_text = String::new();
                let mut found_close = false;
                for next in chars.by_ref() {
                    if next == ']' {
                        found_close = true;
                        break;
                    }
                    link_text.push(next);
                }
                if found_close && chars.peek() == Some(&'(') {
                    chars.next(); // consume (
                    let mut url = String::new();
                    for next in chars.by_ref() {
                        if next == ')' {
                            break;
                        }
                        url.push(next);
                    }
                    spans.push(Span::raw(link_text).fg(theme.primary).underlined());
                    let _ = url; // URL not displayed in terminal
                } else {
                    spans.push(Span::raw(format!("[{link_text}]")));
                }
            }
            // GFM autolinks: detect bare https:// and http:// URLs
            'h' if current_plus_rest_starts_with_url(c, &chars, &current) => {
                if !current.is_empty() {
                    spans.push(Span::raw(current.clone()));
                    current.clear();
                }
                let mut url = String::from(c);
                // Collect chars from the peekable iterator
                while let Some(&next) = chars.peek() {
                    if next.is_whitespace() || next == ')' || next == ']' || next == '>' {
                        break;
                    }
                    url.push(next);
                    chars.next();
                }
                // Strip trailing punctuation that's likely not part of the URL
                while url.ends_with(['.', ',', ';', '!', '?']) {
                    let trailing = url.pop();
                    if let Some(t) = trailing {
                        // We'll push trailing punct after the URL span
                        spans.push(Span::raw(url).fg(theme.hyperlink).underlined());
                        spans.push(Span::raw(t.to_string()));
                        url = String::new();
                        break;
                    }
                }
                if !url.is_empty() {
                    spans.push(Span::raw(url).fg(theme.hyperlink).underlined());
                }
            }
            _ => {
                current.push(c);
            }
        }
    }

    if !current.is_empty() {
        spans.push(Span::raw(current));
    }

    spans
}

/// Check if the current char + remaining iterator starts a URL.
///
/// We look at what's already buffered (`current`) + the peek-ahead chars to detect
/// `https://` or `http://` at a word boundary.
fn current_plus_rest_starts_with_url(
    c: char,
    chars: &std::iter::Peekable<std::str::Chars<'_>>,
    current: &str,
) -> bool {
    if c != 'h' {
        return false;
    }
    // Only start URL detection at a word boundary (start of text or after whitespace/punctuation)
    if let Some(last) = current.chars().last() {
        if last.is_alphanumeric() {
            return false;
        }
    }
    // Peek ahead: we need "ttp://" or "ttps://" after 'h'
    let rest: String = chars.clone().take(7).collect();
    rest.starts_with("ttps://") || rest.starts_with("ttp://")
}

#[cfg(test)]
#[path = "markdown.test.rs"]
mod tests;
