//! Assistant-side message renderers — plain text (markdown), thinking
//! block (collapsible with token estimate), redacted thinking, tool-use
//! call, advisor message.

use std::time::Duration;

use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

use super::ChatWidget;
use crate::constants;
use crate::i18n::t;
use crate::state::session::MessageContent;
use crate::state::session::ToolUseStatus;

/// Turn-boundary glyph at the start of each assistant text response.
/// TS `BLACK_CIRCLE` from `constants/figures.ts` picks `⏺` on macOS for
/// vertical alignment and `●` elsewhere; we standardise on `⏺` which
/// renders cleanly in modern Linux/macOS/Windows Terminal fonts and
/// keeps a consistent visual across platforms.
const ASSISTANT_DOT: &str = "⏺";

/// Format a `Duration` for the tool-use elapsed badge.
///
/// - < 1s: milliseconds (`250ms`)
/// - < 60s: seconds with one decimal (`12.3s`)
/// - >= 60s: minutes + whole seconds (`3m 4s`)
fn format_elapsed(d: Duration) -> String {
    let ms = d.as_millis();
    if ms < 1_000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", d.as_secs_f64())
    } else {
        let secs = d.as_secs();
        format!("{}m {}s", secs / 60, secs % 60)
    }
}

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
                w.theme,
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
                    ratatui::style::Style::default().fg(w.theme.assistant_message),
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
                    Span::raw(ASSISTANT_DOT.to_string()).fg(w.theme.assistant_message),
                ));
            }
            lines.extend(md_lines);
            Some(())
        }
        MessageContent::Thinking {
            content,
            duration_ms,
        } => {
            // TS parity: `AssistantThinkingMessage.tsx`. The collapsed
            // form is a single italic dim line; the expanded form keeps
            // the same header glyph (`∴`) and indents the body two
            // spaces. We drop the per-row `│` gutter from the previous
            // visual — TS doesn't use it and it added visual weight to
            // a section that's supposed to recede.
            if w.show_thinking {
                let token_est = (content.split_whitespace().count() as f64
                    * constants::THINKING_TOKEN_MULTIPLIER) as i64;
                let dur = duration_ms
                    .map(|ms| format!(", {ms}ms"))
                    .unwrap_or_default();
                // Annotation lives in the header suffix instead of a
                // standalone line so the block opens with the same
                // glyph the collapsed form shows — readers' eyes
                // anchor on `∴` regardless of expansion state.
                let suffix = t!(
                    "chat.thinking_suffix_tokens_dur",
                    count = token_est,
                    dur = dur
                )
                .to_string();
                lines.push(Line::from(
                    Span::raw(t!("chat.thinking_header", suffix = suffix.as_str()).to_string())
                        .fg(w.theme.thinking)
                        .dim()
                        .italic(),
                ));
                // Body indented two spaces past the header glyph. TS
                // renders this as `<Markdown dimColor>`; coco-rs's
                // markdown reflow is overkill for thinking content
                // (short prose, no headers / lists worth the indent
                // math), so plain dim text is the closer match.
                for line in content.lines().take(constants::THINKING_PREVIEW_LINES) {
                    lines.push(Line::from(
                        Span::raw(format!("    {line}"))
                            .fg(w.theme.thinking)
                            .dim()
                            .italic(),
                    ));
                }
                if content.lines().count() > constants::THINKING_PREVIEW_LINES {
                    lines.push(Line::from(
                        Span::raw("    …").fg(w.theme.thinking).dim().italic(),
                    ));
                }
            } else {
                // Collapsed form mirrors TS `<Text dim italic>∴ Thinking
                // <CtrlOToExpand /></Text>`. The shortcut text is
                // baked into the i18n key so each locale can pick
                // its own modifier wording.
                lines.push(Line::from(
                    Span::raw(t!("chat.thinking_collapsed").to_string())
                        .fg(w.theme.thinking)
                        .dim()
                        .italic(),
                ));
            }
            Some(())
        }
        MessageContent::RedactedThinking => {
            // ✻ (teardrop asterisk) signals "still thinking" — TS uses
            // this glyph for the redacted/in-flight variant so users
            // can tell at a glance the block isn't finalized.
            lines.push(Line::from(
                Span::raw(t!("chat.redacted_thinking").to_string())
                    .fg(w.theme.thinking)
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
                ToolUseStatus::Queued => w.theme.text_dim,
                ToolUseStatus::Running => w.theme.tool_running,
                ToolUseStatus::Completed => w.theme.tool_completed,
                ToolUseStatus::Failed => w.theme.tool_error,
            };
            let preview = if input_preview.len() > constants::TOOL_DESCRIPTION_MAX_CHARS as usize {
                format!(
                    "{}…",
                    &input_preview[..constants::TOOL_DESCRIPTION_MAX_CHARS as usize - 1]
                )
            } else {
                input_preview.clone()
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
                .map(|t| format!(" ({})", format_elapsed(t.elapsed())))
                .unwrap_or_default();
            lines.push(Line::from(vec![
                Span::raw("  ● ").fg(color),
                Span::raw(tool_name.clone()).fg(w.theme.text).bold(),
                Span::raw(format!("({preview})")).fg(w.theme.text_dim),
                Span::raw(elapsed_badge).fg(w.theme.text_dim).dim(),
            ]));
            Some(())
        }
        MessageContent::Advisor {
            advisor_id,
            content,
        } => {
            lines.push(Line::from(vec![
                Span::raw("  📋 ").fg(w.theme.accent),
                Span::raw(format!("[advisor:{advisor_id}] "))
                    .fg(w.theme.text_dim)
                    .bold(),
            ]));
            let md_lines = crate::widgets::markdown::markdown_to_lines_with_syntax(
                content,
                w.theme,
                w.width,
                w.syntax_highlighting,
            );
            lines.extend(md_lines);
            Some(())
        }
        _ => None,
    }
}
