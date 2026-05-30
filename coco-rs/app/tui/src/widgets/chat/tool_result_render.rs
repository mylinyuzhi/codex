//! Rich, per-tool rendering of a tool call's RESULT body.
//!
//! Replaces the one-size-fits-all 5-line raw-text preview with renderers keyed
//! to what each tool actually produces. The dispatch is an **exhaustive match
//! over `coco_types::ToolName`** so every built-in tool is deliberately routed
//! (the compiler rejects a forgotten variant); MCP / custom tools whose wire
//! name doesn't parse to a `ToolName` fall to the structured default.
//!
//! Rich categories reuse existing primitives — the `diff_display` widget for
//! edits, the markdown highlighter for file content, and `similar` to synthesize
//! a unified diff from an Edit's `old_string`/`new_string`. The "default" path
//! still beats the old behaviour: it pretty-prints JSON instead of dumping it on
//! one line.
//!
//! The renderer is **surface-agnostic**: it takes a [`ToolResultRenderCtx`]
//! (styles + width + the syntax toggle + a truncation hint) rather than a
//! `ChatWidget`, so the inline chat AND the Ctrl+O transcript reader render tool
//! results identically. The reader sets `expanded: true`, relaxing the inline
//! row caps so a diff truncated inline ("… +N lines (ctrl+o to expand)") is shown
//! in full once expanded — the promise the inline hint makes.
//!
//! `input` is the tool's raw invocation arguments when the caller has the
//! issuing assistant message on hand (the paired path), else `None`; input-derived
//! views (diffs, highlighted file content, the web target) degrade gracefully to
//! output-only.

use std::path::Path;

use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use serde_json::Value;

use super::TOOL_OUTPUT_PREVIEW_ROWS;
use super::output_result_line;
use super::result_line;
use super::single_line_capped;
use super::transcript_safe_line;
use crate::presentation::transcript::TRANSCRIPT_EXPANDED_CELL_LINE_CAP;
use crate::presentation::transcript::ToolOutputPreview;
use crate::presentation::transcript::tool_output_preview;
use coco_tui_ui::display::SyntaxHighlighting;
use coco_tui_ui::style::UiStyles;
use coco_types::ToolName;

/// Tool-input field keys, named once so the per-tool renderers don't scatter
/// magic strings that silently drift from the typed inputs in `coco-tools`.
mod field {
    pub(super) const OLD_STRING: &str = "old_string";
    pub(super) const NEW_STRING: &str = "new_string";
    pub(super) const PATCH: &str = "patch";
    pub(super) const FILE_PATH: &str = "file_path";
    pub(super) const CONTENT: &str = "content";
    pub(super) const NEW_SOURCE: &str = "new_source";
    pub(super) const TODOS: &str = "todos";
    pub(super) const TODO_STATUS: &str = "status";
    pub(super) const TODO_CONTENT: &str = "content";
    pub(super) const TODO_ACTIVE_FORM: &str = "activeForm";
    pub(super) const URL: &str = "url";
    pub(super) const QUERY: &str = "query";
}

/// Inline row caps before truncation (full body shown when `ctx.expanded`).
const DIFF_PREVIEW_ROWS: usize = 24;
const CODE_PREVIEW_ROWS: usize = 20;
const MATCH_PREVIEW_ROWS: usize = 14;
const STRUCTURED_PREVIEW_ROWS: usize = 14;
const COMMAND_PREVIEW_ROWS: usize = 10;
/// Single-line header cap (matches the invocation header's preview width).
const HEADER_CAP: usize = 96;

/// Everything the per-tool renderers need from their host surface. Decouples the
/// renderer from `ChatWidget` so the inline chat and the transcript reader share
/// one implementation.
pub(crate) struct ToolResultRenderCtx<'a> {
    pub(crate) styles: UiStyles<'a>,
    pub(crate) width: u16,
    pub(crate) syntax_highlighting: SyntaxHighlighting,
    /// Appended to a "… +N lines" truncation row (e.g. `(ctrl+o to expand)`).
    /// Empty when the surface is itself the full-detail view (the reader).
    pub(crate) expand_hint: String,
    /// Full-detail surface (the reader): relax row caps so nothing re-truncates.
    pub(crate) expanded: bool,
}

impl ToolResultRenderCtx<'_> {
    /// Row budget for a section. The full-detail surface (the reader) uses the
    /// single per-cell cap so an inline-truncated body resolves to its full form
    /// once expanded — bounded, never the whole transcript (I-/cap invariant).
    fn rows(&self, base: usize) -> usize {
        if self.expanded {
            TRANSCRIPT_EXPANDED_CELL_LINE_CAP
        } else {
            base
        }
    }

    /// Truncation marker — a plain indented continuation row (no `└` gutter, which
    /// reads as a new block), eliding the hint when the surface has none.
    fn truncation_line(&self, omitted: usize) -> Line<'static> {
        let text = if self.expand_hint.is_empty() {
            format!("… +{omitted} lines")
        } else {
            format!("… +{omitted} lines {}", self.expand_hint)
        };
        output_result_line(text, self.styles.dim(), /*first*/ false)
    }
}

/// Render the result body for a completed tool call.
pub(crate) fn render_tool_result_body(
    cx: &ToolResultRenderCtx<'_>,
    tool_name: &str,
    input: Option<&Value>,
    output: &str,
    is_error: bool,
    lines: &mut Vec<Line<'static>>,
) {
    if is_error {
        // Errors are uniform across tools: the message matters, not the shape.
        push_text_preview(
            cx,
            output,
            cx.rows(COMMAND_PREVIEW_ROWS),
            lines,
            cx.styles.error(),
        );
        return;
    }
    match tool_name.parse::<ToolName>() {
        Ok(name) => render_known(cx, name, input, output, lines),
        // MCP (`mcp__server__tool`) / plugin / custom names don't parse.
        Err(_) => render_structured_default(cx, output, lines),
    }
}

/// Exhaustive over `ToolName` — adding a variant forces a rendering decision.
fn render_known(
    cx: &ToolResultRenderCtx<'_>,
    name: ToolName,
    input: Option<&Value>,
    output: &str,
    lines: &mut Vec<Line<'static>>,
) {
    use ToolName::*;
    match name {
        // ── Edits → colored unified diff ───────────────────────────────
        Edit => render_edit_diff(cx, input, output, lines),
        ApplyPatch => render_apply_patch(cx, input, output, lines),
        // ── File content → syntax-highlighted code ─────────────────────
        Read => render_read(cx, input, output, lines),
        Write => render_write(cx, input, output, lines),
        NotebookEdit => render_notebook_edit(cx, input, output, lines),
        // ── Shell → output (the `●`/`🔧` header already names the command) ─
        Bash | PowerShell | Repl => render_text(cx, output, COMMAND_PREVIEW_ROWS, lines),
        // ── Search → match list (taller, structured) ───────────────────
        Grep | Glob => render_text(cx, output, MATCH_PREVIEW_ROWS, lines),
        // ── Checklist ──────────────────────────────────────────────────
        TodoWrite => render_todos(cx, input, output, lines),
        // ── Web → target header + output ───────────────────────────────
        WebFetch | WebSearch => render_web(cx, input, output, lines),
        // ── Everything else → structured default (pretty JSON / text) ──
        Agent | Skill | SendMessage | TeamCreate | TeamDelete | TaskCreate | TaskGet | TaskList
        | TaskUpdate | TaskStop | TaskOutput | EnterPlanMode | ExitPlanMode
        | VerifyPlanExecution | EnterWorktree | ExitWorktree | AskUserQuestion | ToolSearch
        | Config | Brief | Lsp | McpAuth | ListMcpResources | ReadMcpResource | CronCreate
        | CronDelete | CronList | RemoteTrigger | Sleep | StructuredOutput => {
            render_structured_default(cx, output, lines)
        }
    }
}

// ── Edits ──────────────────────────────────────────────────────────────

fn render_edit_diff(
    cx: &ToolResultRenderCtx<'_>,
    input: Option<&Value>,
    output: &str,
    lines: &mut Vec<Line<'static>>,
) {
    if let Some(input) = input {
        let old = str_field(input, field::OLD_STRING).unwrap_or_default();
        let new = str_field(input, field::NEW_STRING).unwrap_or_default();
        if old != new {
            let diff = unified_diff_text(&old, &new);
            let rendered =
                coco_tui_ui::widgets::diff_display::render_diff_lines(&diff, cx.styles, cx.width);
            push_capped(cx, indent2(rendered), cx.rows(DIFF_PREVIEW_ROWS), lines);
            return;
        }
    }
    // No input (collapsed/standalone path) — show what the tool reported.
    render_output_preview(cx, output, lines);
}

fn render_apply_patch(
    cx: &ToolResultRenderCtx<'_>,
    input: Option<&Value>,
    output: &str,
    lines: &mut Vec<Line<'static>>,
) {
    // apply_patch carries a `*** Begin Patch`-style body that is NOT standard
    // unified-diff, so colour the +/- lines directly rather than feeding the
    // unified-diff parser. The field is `patch` (see `ApplyPatchInput`).
    let patch = input
        .and_then(|i| str_field(i, field::PATCH))
        .filter(|p| !p.trim().is_empty());
    let Some(patch) = patch else {
        render_output_preview(cx, output, lines);
        return;
    };
    let rendered: Vec<Line<'static>> = patch
        .lines()
        .map(|raw| {
            let color = match raw.as_bytes().first() {
                Some(b'+') => cx.styles.diff_added(),
                Some(b'-') => cx.styles.diff_removed(),
                _ => cx.styles.dim(),
            };
            Line::from(Span::raw(format!("  {}", transcript_safe_line(raw))).fg(color))
        })
        .collect();
    push_capped(cx, rendered, cx.rows(DIFF_PREVIEW_ROWS), lines);
}

// ── File content ─────────────────────────────────────────────────────────

fn render_read(
    cx: &ToolResultRenderCtx<'_>,
    input: Option<&Value>,
    output: &str,
    lines: &mut Vec<Line<'static>>,
) {
    let ext = input
        .and_then(|i| str_field(i, field::FILE_PATH))
        .map(|p| file_ext(&p))
        .unwrap_or_default();
    render_code(cx, &ext, output, None, lines);
}

fn render_write(
    cx: &ToolResultRenderCtx<'_>,
    input: Option<&Value>,
    output: &str,
    lines: &mut Vec<Line<'static>>,
) {
    let Some(input) = input else {
        render_output_preview(cx, output, lines);
        return;
    };
    let path = str_field(input, field::FILE_PATH).unwrap_or_default();
    let Some(content) = str_field(input, field::CONTENT) else {
        render_output_preview(cx, output, lines);
        return;
    };
    let header = (!path.is_empty()).then(|| format!("wrote {path}"));
    render_code(cx, &file_ext(&path), &content, header, lines);
}

fn render_notebook_edit(
    cx: &ToolResultRenderCtx<'_>,
    input: Option<&Value>,
    output: &str,
    lines: &mut Vec<Line<'static>>,
) {
    // Cell replacement has no `old_string` to diff against; show the new source
    // as code (notebooks are Python unless told otherwise).
    if let Some(src) = input.and_then(|i| str_field(i, field::NEW_SOURCE)) {
        render_code(cx, "python", &src, None, lines);
        return;
    }
    render_structured_default(cx, output, lines);
}

// ── Checklist ──────────────────────────────────────────────────────────

fn render_todos(
    cx: &ToolResultRenderCtx<'_>,
    input: Option<&Value>,
    output: &str,
    lines: &mut Vec<Line<'static>>,
) {
    let Some(todos) = input
        .and_then(|i| i.get(field::TODOS))
        .and_then(Value::as_array)
        .filter(|t| !t.is_empty())
    else {
        render_output_preview(cx, output, lines);
        return;
    };
    for (i, todo) in todos.iter().enumerate() {
        let status = todo
            .get(field::TODO_STATUS)
            .and_then(Value::as_str)
            .unwrap_or("pending");
        let content = todo
            .get(field::TODO_CONTENT)
            .and_then(Value::as_str)
            .or_else(|| todo.get(field::TODO_ACTIVE_FORM).and_then(Value::as_str))
            .unwrap_or_default();
        let (glyph, color) = match status {
            "completed" => ("✔", cx.styles.success()),
            "in_progress" => ("◐", cx.styles.warning()),
            _ => ("☐", cx.styles.dim()),
        };
        lines.push(output_result_line(
            format!("{glyph} {}", transcript_safe_line(content)),
            color,
            i == 0,
        ));
    }
}

// ── Web ──────────────────────────────────────────────────────────────────

fn render_web(
    cx: &ToolResultRenderCtx<'_>,
    input: Option<&Value>,
    output: &str,
    lines: &mut Vec<Line<'static>>,
) {
    if let Some(target) =
        input.and_then(|i| str_field(i, field::URL).or_else(|| str_field(i, field::QUERY)))
    {
        lines.push(result_line(
            single_line_capped(&target, HEADER_CAP),
            cx.styles.secondary(),
        ));
    }
    push_text_preview(
        cx,
        output,
        cx.rows(STRUCTURED_PREVIEW_ROWS),
        lines,
        cx.styles.text(),
    );
}

// ── Structured default (everything else + MCP/custom) ─────────────────────

fn render_structured_default(
    cx: &ToolResultRenderCtx<'_>,
    output: &str,
    lines: &mut Vec<Line<'static>>,
) {
    let trimmed = output.trim();
    if (trimmed.starts_with('{') || trimmed.starts_with('['))
        && let Ok(value) = serde_json::from_str::<Value>(trimmed)
        && (value.is_object() || value.is_array())
        && let Ok(pretty) = serde_json::to_string_pretty(&value)
    {
        push_text_preview(
            cx,
            &pretty,
            cx.rows(STRUCTURED_PREVIEW_ROWS),
            lines,
            cx.styles.text(),
        );
        return;
    }
    render_output_preview(cx, output, lines);
}

// ── Shared helpers ─────────────────────────────────────────────────────────

/// Plain-text body at a fixed base row budget (scaled on the reader surface).
fn render_text(
    cx: &ToolResultRenderCtx<'_>,
    output: &str,
    base_rows: usize,
    lines: &mut Vec<Line<'static>>,
) {
    push_text_preview(cx, output, cx.rows(base_rows), lines, cx.styles.text());
}

/// Render `content` as syntax-highlighted code, one line at a time.
///
/// Highlights via [`coco_tui_markdown::highlight_code_lines`] directly rather
/// than wrapping the content in a Markdown fence: a fence both
/// breaks on a `` ``` `` *inside* the content (e.g. reading a Markdown file) and
/// draws a code-block border that doesn't belong under a tool header. Only the
/// first `rows` lines are highlighted — truncation happens *before* the work.
fn render_code(
    cx: &ToolResultRenderCtx<'_>,
    lang: &str,
    content: &str,
    header: Option<String>,
    lines: &mut Vec<Line<'static>>,
) {
    if let Some(header) = header {
        lines.push(result_line(header, cx.styles.dim()));
    }
    if content.trim().is_empty() {
        lines.push(result_line("(empty)".to_string(), cx.styles.dim()));
        return;
    }
    let rows = cx.rows(CODE_PREVIEW_ROWS);
    let mut body = content.lines();
    let visible: Vec<String> = body.by_ref().take(rows).map(transcript_safe_line).collect();
    let highlighted = coco_tui_markdown::highlight_code_lines(
        &visible.join("\n"),
        lang,
        cx.styles,
        cx.syntax_highlighting,
    );
    for (index, line) in visible.iter().enumerate() {
        let prefix = if index == 0 { "  └ " } else { "    " };
        let mut spans = vec![Span::raw(prefix).fg(cx.styles.dim())];
        if let Some(line_spans) = highlighted.as_ref().and_then(|h| h.get(index)) {
            spans.extend(line_spans.iter().cloned());
        } else {
            spans.push(Span::raw(line.clone()).fg(cx.styles.text()));
        }
        lines.push(Line::from(spans));
    }
    // `count()` consumes the lazy remainder without materializing it.
    let omitted = body.count();
    if omitted > 0 {
        lines.push(cx.truncation_line(omitted));
    }
}

/// Head-truncate already-rendered rich lines, appending an expand hint.
fn push_capped(
    cx: &ToolResultRenderCtx<'_>,
    rendered: Vec<Line<'static>>,
    max: usize,
    out: &mut Vec<Line<'static>>,
) {
    let total = rendered.len();
    if total <= max {
        out.extend(rendered);
        return;
    }
    let head = max.saturating_sub(1).max(1);
    out.extend(rendered.into_iter().take(head));
    out.push(cx.truncation_line(total - head));
}

/// Plain-text preview with a configurable row budget and base color. This is the
/// single middle-truncation implementation; `render_output_preview` is the
/// default-budget, default-color shorthand over it.
fn push_text_preview(
    cx: &ToolResultRenderCtx<'_>,
    output: &str,
    rows: usize,
    out: &mut Vec<Line<'static>>,
    color: ratatui::style::Color,
) {
    match tool_output_preview(output, rows) {
        ToolOutputPreview::Empty => {
            out.push(result_line("(no output)".to_string(), cx.styles.dim()));
        }
        ToolOutputPreview::Full(body) => {
            for (i, line) in body.into_iter().enumerate() {
                out.push(output_result_line(
                    transcript_safe_line(line),
                    color,
                    i == 0,
                ));
            }
        }
        ToolOutputPreview::Truncated {
            head,
            omitted,
            tail,
        } => {
            for (i, line) in head.into_iter().enumerate() {
                out.push(output_result_line(
                    transcript_safe_line(line),
                    color,
                    i == 0,
                ));
            }
            out.push(cx.truncation_line(omitted));
            for line in tail {
                out.push(output_result_line(transcript_safe_line(line), color, false));
            }
        }
    }
}

/// Default plain-text preview (text color, default row budget) — the
/// graceful-degradation fallback shared by the input-derived renderers.
pub(crate) fn render_output_preview(
    cx: &ToolResultRenderCtx<'_>,
    output: &str,
    lines: &mut Vec<Line<'static>>,
) {
    push_text_preview(
        cx,
        output,
        cx.rows(TOOL_OUTPUT_PREVIEW_ROWS),
        lines,
        cx.styles.text(),
    );
}

/// Prefix each line with a two-space indent so rich blocks nest under the tool
/// header (the diff widget owns its own line-number gutter).
fn indent2(rendered: Vec<Line<'static>>) -> Vec<Line<'static>> {
    rendered
        .into_iter()
        .map(|mut line| {
            line.spans.insert(0, Span::raw("  "));
            line
        })
        .collect()
}

/// Build unified-diff text (`@@` hunks + signed lines) from two strings for the
/// diff widget. No `---`/`+++` file header is emitted — the tool header already
/// names the file, so it would only be redundant inline. The `\ No newline at
/// end of file` markers `similar` emits when an input lacks a trailing newline
/// are stripped — git-porcelain noise with no place in a tool-result view.
fn unified_diff_text(old: &str, new: &str) -> String {
    let diff = similar::TextDiff::from_lines(old, new);
    let mut unified = diff.unified_diff();
    unified.context_radius(3);
    unified
        .to_string()
        .lines()
        .filter(|line| !line.contains("No newline at end of file"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn str_field(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn file_ext(path: &str) -> String {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
#[path = "tool_result_render.test.rs"]
mod tests;
