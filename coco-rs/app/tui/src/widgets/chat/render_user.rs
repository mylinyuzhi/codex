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

use super::ChatWidget;
use crate::i18n::t;
use crate::state::transcript_view::CellKind;
use crate::state::transcript_view::RenderedCell;
use crate::state::transcript_view::SystemCellKind;

pub(super) fn try_render(
    w: &ChatWidget<'_>,
    cell: &RenderedCell,
    lines: &mut Vec<Line<'static>>,
) -> Option<()> {
    match &cell.kind {
        CellKind::UserText { text } => {
            // Subtle background tint behind user prompt rows. TS parity:
            // `UserPromptMessage` wraps the body in `<Box
            // backgroundColor="userMessageBackground">`, which paints the
            // full row width rather than just the glyphs — the bg must
            // therefore live on the `Line`, not on individual spans.
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
        CellKind::UserAttachment | CellKind::Attachment => {
            // Engine `Message::Attachment` lands here. The attachment
            // kind / preview text isn't reliably typed yet — render a
            // bare paperclip row so users know an attachment slot was
            // populated.
            lines.push(Line::from(vec![
                Span::raw("❯ ").fg(w.styles.user_message()),
                Span::raw("📎 ").fg(w.styles.accent()),
                Span::raw("attachment".to_string()).fg(w.styles.dim()),
            ]));
            Some(())
        }
        CellKind::System(SystemCellKind::UserInterruption { for_tool_use: _ }) => {
            // TS `InterruptedByUser.tsx`: two dim-color text fragments
            // rendered inside a `<MessageResponse height={1}>`. The
            // wrapper is the assistant-row container (gives the row a
            // shaped indent), but the text itself is bare and dim. We
            // mirror that: no `❯` user-bubble prefix, no bg tint, just
            // a single dim line. `for_tool_use` is informational only —
            // TS uses the same component for both variants.
            lines.push(Line::from(
                Span::raw(t!("chat.interrupted_marker").to_string()).fg(w.styles.dim()),
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
