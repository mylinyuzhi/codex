//! User-side cell renderers — text input, attachments, bash invocations
//! (`!cmd` echoed plus output), engine-pushed user interruption marker.
//!
//! Dispatches directly on `cell.kind` / `cell.source: Arc<Message>`.
//! All emitted lines are `Line<'static>` (owned spans).

use coco_messages::Message;
use coco_messages::SystemMessage;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

use super::CellsRenderer;
use crate::i18n::t;
use crate::transcript::cells::CellKind;
use crate::transcript::cells::RenderedCell;
use crate::transcript::cells::SystemCellKind;

pub(super) fn try_render(
    w: &CellsRenderer<'_>,
    cell: &RenderedCell,
    lines: &mut Vec<Line<'static>>,
) -> Option<()> {
    match &cell.kind {
        CellKind::UserText { text } => {
            if let Some(rendered) =
                crate::presentation::slash_command::render_slash_command_user_text(
                    cell.source.as_ref(),
                    text,
                    crate::presentation::slash_command::SlashCommandRenderOptions {
                        styles: w.styles,
                        width: w.width,
                        syntax_highlighting: w.syntax_highlighting,
                        apply_user_background: true,
                    },
                )
            {
                lines.extend(rendered);
                return Some(());
            }
            // Subtle background tint behind user prompt rows. The background
            // must paint the full row width rather than just the glyphs — the
            // bg must therefore live on the `Line`, not on individual spans.
            for line in text.lines() {
                let span = Span::raw(format!("❯ {line}")).fg(w.styles.user_message());
                let mut chat_line = Line::from(span);
                if let Some(bg) = w.styles.user_message_bg() {
                    chat_line = chat_line.style(ratatui::style::Style::default().bg(bg));
                }
                lines.push(chat_line);
            }
            Some(())
        }
        CellKind::Attachment => {
            // Renderable attachments (`renders_in_transcript() == true`) are
            // CONTENT, not collapsed `# [meta]` reminders — the TUI defers
            // meta-ness to the engine's `is_meta_message` (see
            // `presentation::transcript::is_meta`).
            //
            // Memory injections (nested CLAUDE.md / relevant memories) collapse to
            // a compact `◆ memory · <path>` chip: a width-1 marker aligned to the
            // column-2 gutter, distinct from tool/assistant dots by shape + dim
            // styling. Other attachments show the body's first line;
            // silent / structured payloads render nothing.
            if let Some(path) = super::compact_file_reference_chip_path(cell.source.as_ref(), w.cwd)
            {
                lines.push(Line::from(vec![
                    Span::raw("◇ ").fg(w.styles.accent()).dim(),
                    Span::raw("Referenced file ").fg(w.styles.dim()),
                    Span::raw(path).fg(w.styles.dim()).bold(),
                ]));
            } else if let Some(path) = super::nested_memory_chip_path(cell.source.as_ref(), w.cwd) {
                lines.push(Line::from(vec![
                    Span::raw("◆ ").fg(w.styles.accent()).dim(),
                    Span::raw("memory · ").fg(w.styles.dim()),
                    Span::raw(path).fg(w.styles.dim()),
                ]));
            } else if let Some(summary) = super::attachment_summary_text(cell.source.as_ref()) {
                // Generic attachment: width-1 hollow `◇` (vs memory's filled `◆`)
                // so injected context still aligns at the column-2 gutter.
                lines.push(Line::from(vec![
                    Span::raw("◇ ").fg(w.styles.accent()).dim(),
                    Span::raw(summary).fg(w.styles.dim()),
                ]));
            }
            Some(())
        }
        CellKind::System(SystemCellKind::UserInterruption { for_tool_use }) => {
            // Dim "Interrupted · …" row. The `for_tool_use` flag is
            // the engine-authoritative answer to "was a tool in flight
            // when the user cancelled?" (computed once in
            // `finalize_user_cancel`). Surfaces a more specific wording
            // for mid-tool cancellation so users see the distinction
            // encoded in the `INTERRUPT_MESSAGE_FOR_TOOL_USE` text variant.
            let key = if *for_tool_use {
                "chat.interrupted_for_tool_use_marker"
            } else {
                "chat.interrupted_marker"
            };
            lines.push(Line::from(
                Span::raw(t!(key).to_string()).fg(w.styles.dim()),
            ));
            Some(())
        }
        CellKind::System(SystemCellKind::LocalCommand) => {
            // `!cmd` echo: render the command on the `! …` row and the
            // captured stdout/stderr indented below it. Exit code isn't
            // carried on `SystemLocalCommandMessage` yet — treat
            // everything as success-styled.
            let Message::System(SystemMessage::LocalCommand(lc)) = cell.source.as_ref() else {
                return Some(());
            };
            // Bash input row — re-uses the input-area mode glyph so the
            // chat echo visually matches the prompt the user typed.
            lines.push(Line::from(vec![
                Span::raw("! ").fg(w.styles.accent()).bold(),
                Span::raw(lc.command.clone()).fg(w.styles.accent()),
            ]));
            // Output body, capped at 20 visible rows with a "… truncated"
            // hint below.
            let mut iter = lc.output.lines();
            for line in iter.by_ref().take(20) {
                lines.push(Line::from(
                    Span::raw(format!("  {line}")).fg(w.styles.dim()),
                ));
            }
            if iter.next().is_some() {
                lines.push(Line::from(
                    Span::raw(t!("chat.truncated").to_string())
                        .fg(w.styles.dim())
                        .italic(),
                ));
            }
            Some(())
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "user.test.rs"]
mod tests;
