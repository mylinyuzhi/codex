//! Diff display widget — renders unified diff with color coding, line numbers,
//! word-level highlighting, and box-drawing structure.

use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

use crate::theme::Theme;

// ── Box-drawing characters ──────────────────────────────────────────

const BOX_TOP_LEFT: &str = "╭";
const BOX_TOP_RIGHT: &str = "╮";
const BOX_BOTTOM_LEFT: &str = "╰";
const BOX_BOTTOM_RIGHT: &str = "╯";
const BOX_HORIZONTAL: &str = "─";
const BOX_VERTICAL: &str = "│";

// ── Hunk header parsing ─────────────────────────────────────────────

/// Parsed line numbers from a `@@ -old_start,old_count +new_start,new_count @@` header.
struct HunkHeader {
    old_start: i32,
    new_start: i32,
    label: String,
}

/// Parse a unified diff `@@` line into old/new start positions.
fn parse_hunk_header(line: &str) -> Option<HunkHeader> {
    // Format: @@ -old_start[,old_count] +new_start[,new_count] @@ [label]
    let stripped = line.strip_prefix("@@ ")?;
    let end_idx = stripped.find(" @@")?;
    let range_part = &stripped[..end_idx];
    let label = stripped.get(end_idx + 3..).unwrap_or("").trim().to_string();

    let mut parts = range_part.split_whitespace();

    let old_range = parts.next()?.strip_prefix('-')?;
    let old_start: i32 = old_range
        .split(',')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    let new_range = parts.next()?.strip_prefix('+')?;
    let new_start: i32 = new_range
        .split(',')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    Some(HunkHeader {
        old_start,
        new_start,
        label,
    })
}

// ── Word-level diff ─────────────────────────────────────────────────

/// Given two lines (one removed, one added), produce spans that highlight the
/// differing segments. Returns `(removed_spans, added_spans)`.
fn word_diff_spans(
    old_text: &str,
    new_text: &str,
    removed_style: Style,
    added_style: Style,
    emphasis_style: Style,
) -> (Vec<Span<'static>>, Vec<Span<'static>>) {
    let old_chars: Vec<char> = old_text.chars().collect();
    let new_chars: Vec<char> = new_text.chars().collect();

    // Find common prefix length
    let prefix_len = old_chars
        .iter()
        .zip(new_chars.iter())
        .take_while(|(a, b)| a == b)
        .count();

    // Find common suffix length (after prefix)
    let old_remaining = &old_chars[prefix_len..];
    let new_remaining = &new_chars[prefix_len..];
    let suffix_len = old_remaining
        .iter()
        .rev()
        .zip(new_remaining.iter().rev())
        .take_while(|(a, b)| a == b)
        .count();

    let old_diff_end = old_chars.len().saturating_sub(suffix_len);
    let new_diff_end = new_chars.len().saturating_sub(suffix_len);

    let old_prefix: String = old_chars[..prefix_len].iter().collect();
    let old_changed: String = old_chars[prefix_len..old_diff_end].iter().collect();
    let old_suffix: String = old_chars[old_diff_end..].iter().collect();

    let new_prefix: String = new_chars[..prefix_len].iter().collect();
    let new_changed: String = new_chars[prefix_len..new_diff_end].iter().collect();
    let new_suffix: String = new_chars[new_diff_end..].iter().collect();

    let removed_spans = vec![
        Span::styled(old_prefix, removed_style),
        Span::styled(
            old_changed,
            emphasis_style.fg(removed_style.fg.unwrap_or(ratatui::style::Color::Red)),
        ),
        Span::styled(old_suffix, removed_style),
    ];

    let added_spans = vec![
        Span::styled(new_prefix, added_style),
        Span::styled(
            new_changed,
            emphasis_style.fg(added_style.fg.unwrap_or(ratatui::style::Color::Green)),
        ),
        Span::styled(new_suffix, added_style),
    ];

    (removed_spans, added_spans)
}

/// Try to pair consecutive removed/added lines for word-level diffing.
/// Returns groups: each group is either a pair (old, new) or standalone lines.
enum DiffChunk<'a> {
    Paired { old: &'a str, new: &'a str },
    Removed(&'a str),
    Added(&'a str),
    Context(&'a str),
    Hunk(&'a str),
    FileHeader(&'a str),
}

fn classify_diff_lines<'a>(lines: &'a [&'a str]) -> Vec<DiffChunk<'a>> {
    let mut chunks = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];

        if line.starts_with("--- ") || line.starts_with("+++ ") {
            chunks.push(DiffChunk::FileHeader(line));
            i += 1;
        } else if line.starts_with("@@") {
            chunks.push(DiffChunk::Hunk(line));
            i += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            // Collect consecutive removed lines
            let rm_start = i;
            while i < lines.len() && lines[i].starts_with('-') && !lines[i].starts_with("---") {
                i += 1;
            }
            // Collect consecutive added lines
            let add_start = i;
            while i < lines.len() && lines[i].starts_with('+') && !lines[i].starts_with("+++") {
                i += 1;
            }
            let removed = &lines[rm_start..add_start];
            let added = &lines[add_start..i];

            // Pair up removed/added lines for word-diff
            let pairs = removed.len().min(added.len());
            for j in 0..pairs {
                chunks.push(DiffChunk::Paired {
                    old: removed[j],
                    new: added[j],
                });
            }
            // Leftover removed lines
            for line in &removed[pairs..] {
                chunks.push(DiffChunk::Removed(line));
            }
            // Leftover added lines
            for line in &added[pairs..] {
                chunks.push(DiffChunk::Added(line));
            }
        } else if line.starts_with('+') && !line.starts_with("+++") {
            chunks.push(DiffChunk::Added(line));
            i += 1;
        } else {
            chunks.push(DiffChunk::Context(line));
            i += 1;
        }
    }

    chunks
}

// ── Line number formatting ──────────────────────────────────────────

/// Format a line number into a fixed-width string, or blanks if not applicable.
fn fmt_line_no(n: Option<i32>, width: i32) -> String {
    match n {
        Some(num) => format!("{num:>w$}", w = width as usize),
        None => " ".repeat(width as usize),
    }
}

// ── Public API ──────────────────────────────────────────────────────

/// Render diff text as colored lines with line numbers and word-level
/// highlighting.
///
/// Maintains the original public signature for backward compatibility with
/// the chat widget.
pub fn render_diff_lines(diff_text: &str, theme: &Theme, _width: u16) -> Vec<Line<'static>> {
    let raw_lines: Vec<&str> = diff_text.lines().collect();
    let chunks = classify_diff_lines(&raw_lines);

    let gutter_width: i32 = 4;
    let removed_style = Style::new().fg(theme.diff_removed);
    let added_style = Style::new().fg(theme.diff_added);
    let emphasis = Style::new().reversed();

    let mut old_line: i32 = 1;
    let mut new_line: i32 = 1;
    let mut result = Vec::new();

    for chunk in &chunks {
        match chunk {
            DiffChunk::FileHeader(text) => {
                let (label, path) = if let Some(rest) = text.strip_prefix("--- ") {
                    ("─", rest)
                } else if let Some(rest) = text.strip_prefix("+++ ") {
                    ("+", rest)
                } else {
                    ("", *text)
                };
                result.push(Line::from(vec![
                    Span::raw(format!("  {label} ")).fg(theme.text_dim),
                    Span::raw(path.to_string()).fg(theme.primary).bold(),
                ]));
            }
            DiffChunk::Hunk(text) => {
                if let Some(hdr) = parse_hunk_header(text) {
                    old_line = hdr.old_start;
                    new_line = hdr.new_start;
                    let label_part = if hdr.label.is_empty() {
                        String::new()
                    } else {
                        format!(" {}", hdr.label)
                    };
                    result.push(Line::from(
                        Span::raw(format!("  ╶╴ @@ -{old_line} +{new_line} @@{label_part}"))
                            .fg(theme.primary)
                            .dim(),
                    ));
                } else {
                    result.push(Line::from(
                        Span::raw(format!("  {text}")).fg(theme.primary).dim(),
                    ));
                }
            }
            DiffChunk::Context(text) => {
                let old_no = fmt_line_no(Some(old_line), gutter_width);
                let new_no = fmt_line_no(Some(new_line), gutter_width);
                let content = text.strip_prefix(' ').unwrap_or(text);
                result.push(Line::from(vec![
                    Span::raw(format!("  {old_no} {new_no} "))
                        .fg(theme.text_dim)
                        .dim(),
                    Span::raw(format!("{BOX_VERTICAL} ")).fg(theme.border),
                    Span::raw(content.to_string()).fg(theme.text_dim),
                ]));
                old_line += 1;
                new_line += 1;
            }
            DiffChunk::Removed(text) => {
                let old_no = fmt_line_no(Some(old_line), gutter_width);
                let blank = fmt_line_no(None, gutter_width);
                let content = text.strip_prefix('-').unwrap_or(text);
                result.push(Line::from(vec![
                    Span::raw(format!("  {old_no} {blank} "))
                        .fg(theme.text_dim)
                        .dim(),
                    Span::styled(
                        format!("{BOX_VERTICAL} "),
                        Style::new().fg(theme.diff_removed),
                    ),
                    Span::styled(format!("-{content}"), removed_style),
                ]));
                old_line += 1;
            }
            DiffChunk::Added(text) => {
                let blank = fmt_line_no(None, gutter_width);
                let new_no = fmt_line_no(Some(new_line), gutter_width);
                let content = text.strip_prefix('+').unwrap_or(text);
                result.push(Line::from(vec![
                    Span::raw(format!("  {blank} {new_no} "))
                        .fg(theme.text_dim)
                        .dim(),
                    Span::styled(
                        format!("{BOX_VERTICAL} "),
                        Style::new().fg(theme.diff_added),
                    ),
                    Span::styled(format!("+{content}"), added_style),
                ]));
                new_line += 1;
            }
            DiffChunk::Paired { old, new } => {
                let old_content = old.strip_prefix('-').unwrap_or(old);
                let new_content = new.strip_prefix('+').unwrap_or(new);

                let (rm_spans, add_spans) = word_diff_spans(
                    old_content,
                    new_content,
                    removed_style,
                    added_style,
                    emphasis,
                );

                // Removed line
                let old_no = fmt_line_no(Some(old_line), gutter_width);
                let blank = fmt_line_no(None, gutter_width);
                let mut rm_line_spans = vec![
                    Span::raw(format!("  {old_no} {blank} "))
                        .fg(theme.text_dim)
                        .dim(),
                    Span::styled(
                        format!("{BOX_VERTICAL} "),
                        Style::new().fg(theme.diff_removed),
                    ),
                    Span::styled("-", removed_style),
                ];
                rm_line_spans.extend(rm_spans);
                result.push(Line::from(rm_line_spans));
                old_line += 1;

                // Added line
                let blank2 = fmt_line_no(None, gutter_width);
                let new_no = fmt_line_no(Some(new_line), gutter_width);
                let mut add_line_spans = vec![
                    Span::raw(format!("  {blank2} {new_no} "))
                        .fg(theme.text_dim)
                        .dim(),
                    Span::styled(
                        format!("{BOX_VERTICAL} "),
                        Style::new().fg(theme.diff_added),
                    ),
                    Span::styled("+", added_style),
                ];
                add_line_spans.extend(add_spans);
                result.push(Line::from(add_line_spans));
                new_line += 1;
            }
        }
    }

    result
}

/// Render a full-screen structured diff view with file path header, line
/// numbers, box-drawing border, and scroll support.
///
/// The `scroll` parameter controls which line is at the top of the viewport.
/// Negative values are clamped to 0.
pub fn render_structured_diff(
    path: &str,
    diff_text: &str,
    theme: &Theme,
    width: u16,
    scroll: i32,
) -> Vec<Line<'static>> {
    let inner_width = (width as i32 - 4).max(10) as usize;
    let horiz_border: String = BOX_HORIZONTAL.repeat(inner_width);

    let mut all_lines: Vec<Line<'static>> = Vec::new();

    // ── File header ─────────────────────────────────────────────
    all_lines.push(Line::from(vec![
        Span::raw(format!("{BOX_TOP_LEFT}{horiz_border}{BOX_TOP_RIGHT}")).fg(theme.border),
    ]));
    let path_display = truncate_path(path, inner_width.saturating_sub(4));
    all_lines.push(Line::from(vec![
        Span::raw(format!("{BOX_VERTICAL} ")).fg(theme.border),
        Span::raw(path_display).fg(theme.primary).bold(),
    ]));
    all_lines.push(Line::from(vec![
        Span::raw(format!("{BOX_VERTICAL}{horiz_border}{BOX_VERTICAL}")).fg(theme.border),
    ]));

    // ── Diff content ────────────────────────────────────────────
    let diff_lines = render_diff_lines(diff_text, theme, width);
    for line in diff_lines {
        // Re-wrap each line inside the box border
        let mut spans = vec![Span::raw(BOX_VERTICAL.to_string()).fg(theme.border)];
        spans.extend(line.spans);
        all_lines.push(Line::from(spans));
    }

    // ── Footer ──────────────────────────────────────────────────
    all_lines.push(Line::from(vec![
        Span::raw(format!("{BOX_BOTTOM_LEFT}{horiz_border}{BOX_BOTTOM_RIGHT}")).fg(theme.border),
    ]));

    // ── Apply scroll offset ─────────────────────────────────────
    let offset = scroll.max(0) as usize;
    if offset >= all_lines.len() {
        return Vec::new();
    }
    all_lines.split_off(offset)
}

/// Truncate a path to fit within `max_len`, keeping the tail.
fn truncate_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        return path.to_string();
    }
    if max_len <= 3 {
        return "...".to_string();
    }
    let keep = max_len - 3;
    format!("...{}", &path[path.len() - keep..])
}

#[cfg(test)]
#[path = "diff_display.test.rs"]
mod tests;
