//! Assistant-side message renderers — plain text (markdown), thinking
//! block (collapsible with token estimate), redacted thinking, tool-use
//! call, advisor message.

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
use crate::state::session::MessageContent;
use crate::state::session::ToolUseStatus;

/// Turn-boundary glyph at the start of each assistant text response.
/// TS `BLACK_CIRCLE` from `constants/figures.ts` picks `⏺` on macOS for
/// vertical alignment and `●` elsewhere; we standardise on `⏺` which
/// renders cleanly in modern Linux/macOS/Windows Terminal fonts and
/// keeps a consistent visual across platforms.
const ASSISTANT_DOT: &str = "⏺";

pub(super) fn try_render<'a>(
    w: &ChatWidget<'a>,
    content: &'a MessageContent,
    lines: &mut Vec<Line<'a>>,
) -> Option<()> {
    match content {
        MessageContent::AssistantText(text) => {
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
        MessageContent::Thinking {
            content,
            duration_ms,
            reasoning_tokens,
        } => {
            lines.extend(render_thinking_block(
                ThinkingRenderInput {
                    content,
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
        MessageContent::RedactedThinking => {
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
        MessageContent::ToolUse {
            tool_name,
            call_id,
            input_preview,
            status,
        } => {
            // TS parity: `AssistantToolUseMessage.tsx` + `ToolUseLoader`.
            // The dot is the same glyph (`●`) across all states; only
            // the colour varies. This is intentional — different
            // glyphs for queued/running/done created visual churn
            // when a long turn cycled through them, and the colour
            // alone is enough to distinguish status at a glance. The
            // `bold` tool name plus the inline preview match the TS
            // layout `<dot> <bold name>(<preview>)`.
            let color = match status {
                ToolUseStatus::Queued => w.styles.dim(),
                ToolUseStatus::Running => w.styles.tool_running(),
                ToolUseStatus::Completed => w.styles.tool_completed(),
                ToolUseStatus::Failed => w.styles.tool_error(),
            };
            let preview = if input_preview.len() > constants::TOOL_DESCRIPTION_MAX_CHARS as usize {
                format!(
                    "{}…",
                    &input_preview[..constants::TOOL_DESCRIPTION_MAX_CHARS as usize - 1]
                )
            } else {
                input_preview.clone()
            };
            let label = if preview.is_empty() {
                format!("🔨 {tool_name}")
            } else {
                format!("🔨 {tool_name}({preview})")
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
            lines.push(Line::from(vec![
                Span::raw("• ").fg(color),
                Span::raw(label).fg(w.styles.text()),
                Span::raw(elapsed_badge).fg(w.styles.dim()).dim(),
            ]));
            Some(())
        }
        MessageContent::Advisor {
            advisor_id,
            content,
        } => {
            lines.push(Line::from(vec![
                Span::raw("  📋 ").fg(w.styles.accent()),
                Span::raw(format!("[advisor:{advisor_id}] "))
                    .fg(w.styles.dim())
                    .bold(),
            ]));
            let md_lines = crate::widgets::markdown::markdown_to_lines_with_syntax(
                content,
                w.styles,
                w.width,
                w.syntax_highlighting,
            );
            lines.extend(md_lines);
            Some(())
        }
        _ => None,
    }
}
