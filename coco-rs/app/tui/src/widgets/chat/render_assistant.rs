//! Assistant-side cell renderers — text (markdown), thinking
//! (collapsible), redacted thinking, tool-use invocation.
//!
//! Phase 3d (§6): dispatches directly on `cell.kind` /
//! `cell.source: Arc<Message>` — `ChatMessage` / `MessageContent` are
//! gone. All emitted lines are `Line<'static>` (owned spans).

use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

use super::ChatWidget;
use crate::constants;
use crate::i18n::t;
use crate::presentation::thinking::ThinkingDisplay;
use crate::presentation::thinking::ThinkingRenderInput;
use crate::presentation::thinking::format_duration_seconds;
use crate::presentation::thinking::render_thinking_block;
use crate::state::transcript_view::CellKind;
use crate::state::transcript_view::RenderedCell;
use crate::tool_display::ToolNameTone;
use crate::tool_display::tool_name_tone;

/// Turn-boundary glyph at the start of each assistant text response.
/// TS `BLACK_CIRCLE` from `constants/figures.ts` picks `⏺` on macOS for
/// vertical alignment and `●` elsewhere; we standardise on `⏺` which
/// renders cleanly in modern Linux/macOS/Windows Terminal fonts and
/// keeps a consistent visual across platforms.
const ASSISTANT_DOT: &str = "⏺";

pub(super) fn try_render(
    w: &ChatWidget<'_>,
    cell: &RenderedCell,
    lines: &mut Vec<Line<'static>>,
) -> Option<()> {
    match &cell.kind {
        CellKind::AssistantText { text, .. } => {
            // TS parity: `AssistantTextMessage` renders the body with a
            // leading `BLACK_CIRCLE` glyph on the first line as a turn
            // marker (`shouldShowDot` is true for the top assistant
            // text of each response). We always show it here — coco-rs
            // doesn't yet thread the "is first text block" hint, but
            // the visual cost of an extra glyph on multi-block turns is
            // small compared to the wayfinding value on every other
            // turn. The trailing rows of the same response keep no
            // gutter so wrapped prose stays at column 0.
            let mut md_lines = crate::widgets::markdown::markdown_to_lines_with_syntax(
                text,
                w.styles,
                w.width,
                w.syntax_highlighting,
            );
            if let Some(first) = md_lines.first_mut() {
                // `widgets::markdown` left-pads each paragraph line with
                // two spaces (see the `vec![Span::raw("  ")]` initialiser
                // in `markdown.rs`). Replace that indent with our dot +
                // space so the marker lands at column 0 and the prose
                // continues at column 2 — matching TS layout
                // `<NoSelect minWidth={2}>⏺</NoSelect><Markdown>…`.
                let dot_span = Span::styled(
                    format!("{ASSISTANT_DOT} "),
                    ratatui::style::Style::default().fg(w.styles.assistant_message()),
                );
                let leading_is_indent = first
                    .spans
                    .first()
                    .map(|s| s.content.as_ref() == "  ")
                    .unwrap_or(false);
                if leading_is_indent {
                    first.spans[0] = dot_span;
                } else {
                    first.spans.insert(0, dot_span);
                }
            } else {
                // Empty markdown (e.g. blank response) — still emit the
                // marker line so the turn boundary is visible.
                lines.push(Line::from(
                    Span::raw(ASSISTANT_DOT.to_string()).fg(w.styles.assistant_message()),
                ));
            }
            lines.extend(md_lines);
            Some(())
        }
        CellKind::AssistantThinking {
            text,
            duration_ms,
            reasoning_tokens,
        } => {
            lines.extend(render_thinking_block(
                ThinkingRenderInput {
                    content: text,
                    duration_ms: *duration_ms,
                    reasoning_tokens: *reasoning_tokens,
                    display: if w.show_thinking {
                        ThinkingDisplay::Expanded {
                            max_body_lines: crate::constants::THINKING_PREVIEW_LINES,
                            truncated_hint: "…",
                        }
                    } else {
                        ThinkingDisplay::Collapsed
                    },
                },
                w.styles,
            ));
            Some(())
        }
        CellKind::AssistantRedactedThinking => {
            // ✻ (teardrop asterisk) signals "still thinking" — TS uses
            // this glyph for the redacted/in-flight variant so users
            // can tell at a glance the block isn't finalized.
            lines.push(Line::from(
                Span::raw(t!("chat.redacted_thinking").to_string())
                    .fg(w.styles.thinking())
                    .dim()
                    .italic(),
            ));
            Some(())
        }
        CellKind::ToolUse { call_id, tool_name } => {
            let input_preview =
                crate::state::derive::extract_tool_call_input_preview(&cell.source, call_id);
            let preview = if input_preview.len() > constants::TOOL_DESCRIPTION_MAX_CHARS as usize {
                format!(
                    "{}…",
                    &input_preview[..constants::TOOL_DESCRIPTION_MAX_CHARS as usize - 1]
                )
            } else {
                input_preview
            };
            // Elapsed time badge: `(250ms)` / `(1.2s)` / `(3m 4s)`
            // tail-aligned after the preview. Sourced from the
            // matching ToolExecution by call_id so running tools tick
            // forward via SpinnerTick redraws and completed tools
            // freeze at their final duration.
            let elapsed_badge = w
                .tool_executions
                .iter()
                .find(|t| t.call_id == *call_id)
                .map(|t| format!(" ({})", format_duration_seconds(t.elapsed())))
                .unwrap_or_default();
            let mut spans = vec![
                Span::raw("🔨 ").fg(w.styles.dim()),
                Span::raw(tool_name.clone())
                    .fg(tool_tone_color(tool_name_tone(tool_name), w.styles))
                    .bold(),
            ];
            if !preview.is_empty() {
                spans.push(Span::raw(format!("({preview})")).fg(w.styles.text()));
            }
            spans.push(Span::raw(elapsed_badge).fg(w.styles.dim()).dim());
            lines.push(Line::from(spans));
            Some(())
        }
        _ => None,
    }
}

fn tool_tone_color(
    tone: ToolNameTone,
    styles: crate::presentation::styles::UiStyles<'_>,
) -> ratatui::style::Color {
    match tone {
        ToolNameTone::ReadOnly => styles.success(),
        ToolNameTone::Shell => styles.primary(),
        ToolNameTone::Write => styles.warning(),
        ToolNameTone::Agent => styles.accent(),
        ToolNameTone::Plan => styles.plan(),
        ToolNameTone::Utility => styles.secondary(),
    }
}
