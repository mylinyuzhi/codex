//! Tool-result renderers — success, error, rejected, canceled, file-edit
//! diff, file-write result.

use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

use super::ChatWidget;
use crate::i18n::t;
use crate::state::session::MessageContent;

/// Render one line of an `LSPTool` result, highlighting the first
/// `path:line:col` (or `path:line`) location reference if present. The
/// formatter output (`coco_tools::tools::lsp::format_*`) embeds these
/// references in lines like `Defined in foo.rs:12:5` or `  foo.rs:42`.
fn render_lsp_line<'a>(w: &ChatWidget<'_>, line: &'a str) -> Line<'a> {
    let indent = "    ";
    let Some((leading_spaces, body)) = split_leading_space(line) else {
        return Line::from(Span::raw(format!("{indent}{line}")).fg(w.styles.text()));
    };

    if let Some((before, path_part, line_part)) = parse_location_marker(body) {
        let mut spans: Vec<Span<'a>> =
            vec![Span::raw(format!("{indent}{leading_spaces}")).fg(w.styles.text())];
        if !before.is_empty() {
            spans.push(Span::raw(before.to_string()).fg(w.styles.text()));
        }
        spans.push(
            Span::raw(path_part.to_string())
                .fg(w.styles.primary())
                .underlined(),
        );
        spans.push(Span::raw(line_part.to_string()).fg(w.styles.dim()));
        return Line::from(spans);
    }

    Line::from(Span::raw(format!("{indent}{line}")).fg(w.styles.text()))
}

/// Split `"    body"` into `("    ", "body")`. Empty body counts as
/// no-split because there is nothing left to style.
fn split_leading_space(s: &str) -> Option<(&str, &str)> {
    let body_start = s.find(|c: char| !c.is_whitespace())?;
    Some(s.split_at(body_start))
}

/// Find the first segment looking like `<path>:<line>(:<col>)?` and
/// split the line into `(prefix, path, line[:col])`. Heuristic: the
/// path token must contain a `/`, `\`, or `.` (extension) so prose
/// like "Hover info at 5:3" is not mis-identified. Returns `None` when
/// no marker is found.
fn parse_location_marker(line: &str) -> Option<(&str, &str, &str)> {
    // Walk segments separated by whitespace, find first that has a
    // colon-separated line/col tail with an integer.
    for (start, token) in line.match_indices(|c: char| !c.is_whitespace()) {
        let end = line[start..]
            .find(char::is_whitespace)
            .map_or(line.len(), |off| start + off);
        let candidate = &line[start..end];
        if let Some((path, tail)) = split_location_token(candidate)
            && (path.contains('/') || path.contains('\\') || path.contains('.'))
        {
            let _ = token; // strictly identify the span we matched
            return Some((
                &line[..start],
                path,
                &candidate[path.len()..path.len() + tail],
            ));
        }
    }
    None
}

/// Split `foo.rs:12:5` (or `foo.rs:12`) into (`foo.rs`, `:12:5`).
/// Returns `None` when the token has no integer suffix.
fn split_location_token(token: &str) -> Option<(&str, usize)> {
    // Find the last colon-prefix integer run, walking from the end.
    let bytes = token.as_bytes();
    let mut end = bytes.len();
    let mut tail_len = 0usize;
    // Pop optional `:N` then optional `:N` (col + line).
    for _ in 0..2 {
        if end == 0 || bytes[end - 1].is_ascii_digit() {
            let digits_start = bytes[..end]
                .iter()
                .rposition(|b| !b.is_ascii_digit())
                .map_or(0, |p| p + 1);
            if digits_start == end || digits_start == 0 || bytes[digits_start - 1] != b':' {
                break;
            }
            tail_len += end - (digits_start - 1);
            end = digits_start - 1;
        } else {
            break;
        }
    }
    if tail_len == 0 {
        return None;
    }
    Some((&token[..end], tail_len))
}

#[cfg(test)]
#[path = "render_tool.test.rs"]
mod tests;

pub(super) fn try_render<'a>(
    w: &ChatWidget<'a>,
    content: &'a MessageContent,
    lines: &mut Vec<Line<'a>>,
) -> Option<()> {
    match content {
        MessageContent::ToolSuccess { tool_name, output } => {
            // Mirror the invocation row: the result block reuses the
            // `●` glyph so the eye groups call+result. Status is
            // colour-encoded (green ⇒ completed). Body is indented
            // four spaces past the gutter so column alignment matches
            // the tool-use header.
            lines.push(Line::from(vec![
                Span::raw("  ● ").fg(w.styles.tool_completed()),
                Span::raw(tool_name.clone()).fg(w.styles.text()).bold(),
            ]));
            let total = output.lines().count();
            // LSP results contain `path:line:col` location references
            // that we want to highlight (path → primary, line/col →
            // text_dim). The string-formatting itself lives in
            // `coco_tools::tools::lsp` — this is just per-line styling
            // for the terminal. All other tools stay on the generic
            // text-only path.
            let is_lsp = tool_name == "LSP";
            for line in output.lines().take(15) {
                if is_lsp {
                    lines.push(render_lsp_line(w, line));
                } else {
                    lines.push(Line::from(
                        Span::raw(format!("    {line}")).fg(w.styles.text()),
                    ));
                }
            }
            if total > 15 {
                lines.push(Line::from(
                    Span::raw(format!("    … ({} more lines)", total - 15))
                        .fg(w.styles.dim())
                        .italic(),
                ));
            }
            Some(())
        }
        MessageContent::ToolError { tool_name, error } => {
            lines.push(Line::from(vec![
                Span::raw("  ● ").fg(w.styles.tool_error()),
                Span::raw(tool_name.clone()).fg(w.styles.text()).bold(),
                Span::raw(": ").fg(w.styles.dim()),
                Span::raw(error.as_str()).fg(w.styles.error()),
            ]));
            Some(())
        }
        MessageContent::ToolRejected { tool_name, reason } => {
            // ⊘ (circled division slash) reads as "blocked" without
            // implying error or success — TS uses similar gating
            // glyphs. Warning colour keeps the row visible but not
            // alarming.
            lines.push(Line::from(vec![
                Span::raw("  ⊘ ").fg(w.styles.warning()),
                Span::raw(t!("chat.tool_rejected", tool_name = tool_name).to_string())
                    .fg(w.styles.dim()),
                Span::raw(reason.as_str()).fg(w.styles.warning()),
            ]));
            Some(())
        }
        MessageContent::ToolCanceled { tool_name } => {
            lines.push(Line::from(vec![
                Span::raw("  ⊘ ").fg(w.styles.dim()),
                Span::raw(t!("chat.tool_canceled", tool_name = tool_name).to_string())
                    .fg(w.styles.dim())
                    .italic(),
            ]));
            Some(())
        }
        MessageContent::FileEditDiff { path, diff, .. } => {
            lines.push(Line::from(vec![
                Span::raw("  📝 ").fg(w.styles.accent()),
                Span::raw(path.as_str()).fg(w.styles.primary()).underlined(),
            ]));
            let diff_lines =
                crate::widgets::diff_display::render_diff_lines(diff, w.styles, w.width);
            lines.extend(diff_lines);
            Some(())
        }
        MessageContent::FileWriteResult {
            path,
            bytes_written,
        } => {
            lines.push(Line::from(vec![
                Span::raw("  ✓ ").fg(w.styles.tool_completed()),
                Span::raw(t!("chat.wrote_bytes").to_string()).fg(w.styles.dim()),
                Span::raw(path.as_str()).fg(w.styles.primary()),
                Span::raw(format!(" ({bytes_written} bytes)")).fg(w.styles.dim()),
            ]));
            Some(())
        }
        _ => None,
    }
}
