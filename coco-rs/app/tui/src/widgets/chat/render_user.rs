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
            // Slash-command echo/result messages carry TS command tags
            // (`<command-name>…`, `<local-command-stdout>…`). Render them
            // tool-style (`❯ /cmd args` + `⎿ output`) rather than as raw
            // XML. Model-visibility is orthogonal (the engine's
            // `is_visible_in_transcript_only` gate); both display modes
            // render identically here, matching TS where `createUserMessage`
            // and `createCommandInputMessage` share the command-pill UI.
            if coco_messages::is_command_input(text) {
                render_command_echo(w, text, lines);
                return Some(());
            }
            if coco_messages::is_local_command_output(text) {
                render_command_output(w, text, lines);
                return Some(());
            }
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
        CellKind::System(SystemCellKind::UserInterruption { for_tool_use }) => {
            // Dim "Interrupted · …" row. The `for_tool_use` flag is
            // the engine-authoritative answer to "was a tool in flight
            // when the user cancelled?" (computed once in
            // `finalize_user_cancel`). Surfaces a more specific
            // wording for mid-tool cancellation so users see the
            // distinction TS encodes via the
            // `INTERRUPT_MESSAGE_FOR_TOOL_USE` text variant in
            // persisted JSONL.
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

/// Render a slash-command echo (`<command-name>…` body) as `❯ /cmd args`,
/// reusing the user-prompt glyph + background so it reads as a command the
/// user issued. TS parity: `UserCommandMessage`.
fn render_command_echo(w: &ChatWidget<'_>, text: &str, lines: &mut Vec<Line<'static>>) {
    let name = coco_messages::extract_tag(text, coco_messages::COMMAND_NAME_TAG).unwrap_or("");
    let args = coco_messages::extract_tag(text, coco_messages::COMMAND_ARGS_TAG).unwrap_or("");
    let echo = if args.is_empty() {
        name.to_string()
    } else {
        format!("{name} {args}")
    };
    let span = Span::raw(format!("❯ {echo}")).fg(w.styles.user_message());
    let mut chat_line = Line::from(span);
    if let Some(bg) = w.styles.user_message_bg() {
        chat_line = chat_line.style(ratatui::style::Style::default().bg(bg));
    }
    lines.push(chat_line);
}

/// Render a slash-command result (`<local-command-stdout|stderr>…` body)
/// as a markdown body under a `└` gutter (first row `  └ `, continuation
/// rows aligned). TS parity: `UserLocalCommandOutputMessage` (`⎿`).
fn render_command_output(w: &ChatWidget<'_>, text: &str, lines: &mut Vec<Line<'static>>) {
    let body = coco_messages::extract_tag(text, coco_messages::LOCAL_COMMAND_STDOUT_TAG)
        .or_else(|| coco_messages::extract_tag(text, coco_messages::LOCAL_COMMAND_STDERR_TAG))
        .unwrap_or("");
    if body.is_empty() {
        return;
    }
    // Markdown-render so rich results (/help, /context) keep formatting;
    // reserve 4 cols for the gutter so wrapping accounts for it.
    let opts = coco_tui_markdown::MarkdownOptions::new(
        w.styles,
        w.width.saturating_sub(4),
        w.syntax_highlighting,
    );
    let rendered = coco_tui_markdown::render_markdown(body, opts, None);
    for (index, mut line) in rendered.into_iter().enumerate() {
        let prefix = if index == 0 { "  └ " } else { "    " };
        line.spans.insert(0, Span::raw(prefix).fg(w.styles.dim()));
        lines.push(line);
    }
}
