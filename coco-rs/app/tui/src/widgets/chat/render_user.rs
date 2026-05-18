//! User-side message renderers — text, images, bash input/output, plan
//! markers, memory input, agent notifications, teammate messages,
//! attachments, channel messages, MCP resource updates.

use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

use super::ChatWidget;
use super::format_resource_target;
use super::parse_teammate_xml;
use super::teammate_color_to_ratatui;
use crate::i18n::t;
use crate::state::session::MessageContent;
use crate::state::session::PlanAction;
use crate::state::session::ResourceUpdateKind;

pub(super) fn try_render<'a>(
    w: &ChatWidget<'a>,
    content: &'a MessageContent,
    lines: &mut Vec<Line<'a>>,
) -> Option<()> {
    match content {
        MessageContent::Text(text) => {
            // Subtle background tint behind user prompt rows. TS parity:
            // `UserPromptMessage` wraps the body in `<Box
            // backgroundColor="userMessageBackground">`, which paints the
            // full row width rather than just the glyphs — the bg must
            // therefore live on the `Line`, not on individual spans.
            let mut iter = text.lines();
            for line in iter.by_ref() {
                let span = Span::raw(format!("❯ {line}")).fg(w.styles.user_message());
                let mut chat_line = Line::from(span);
                if let Some(bg) = w.styles.user_message_bg() {
                    chat_line = chat_line.style(ratatui::style::Style::default().bg(bg));
                }
                lines.push(chat_line);
            }
            Some(())
        }
        MessageContent::Image { path } => {
            lines.push(Line::from(vec![
                Span::raw("❯ ").fg(w.styles.user_message()),
                Span::raw("📎 ").fg(w.styles.accent()),
                Span::raw(path.as_str()).fg(w.styles.primary()).underlined(),
            ]));
            Some(())
        }
        MessageContent::BashInput { command } => {
            // TS `UserBashInputMessage` renders the leading `!` in
            // `bashBorder` style. We re-use the input-area mode glyph so
            // the chat echo visually matches the prompt the user typed.
            lines.push(Line::from(vec![
                Span::raw("! ").fg(w.styles.accent()).bold(),
                Span::raw(command.as_str()).fg(w.styles.accent()),
            ]));
            Some(())
        }
        MessageContent::BashOutput { output, exit_code } => {
            let color = if *exit_code == 0 {
                w.styles.dim()
            } else {
                w.styles.error()
            };
            let mut iter = output.lines();
            for line in iter.by_ref().take(20) {
                lines.push(Line::from(Span::raw(format!("  {line}")).fg(color)));
            }
            if iter.next().is_some() {
                lines.push(Line::from(
                    Span::raw(t!("chat.truncated").to_string())
                        .fg(w.styles.dim())
                        .italic(),
                ));
            }
            if *exit_code != 0 {
                lines.push(Line::from(
                    Span::raw(t!("chat.exit_code", code = exit_code).to_string())
                        .fg(w.styles.error()),
                ));
            }
            Some(())
        }
        MessageContent::PlanMarker { action } => {
            let text = match action {
                PlanAction::Enter => t!("chat.plan_entered"),
                PlanAction::Exit => t!("chat.plan_exited"),
            };
            lines.push(Line::from(
                Span::raw(format!("  {text}")).fg(w.styles.plan()).italic(),
            ));
            Some(())
        }
        MessageContent::AgentNotification { agent_id, summary } => {
            lines.push(Line::from(vec![
                Span::raw("  🤖 ").fg(w.styles.accent()),
                Span::raw(format!("[{agent_id}] ")).fg(w.styles.dim()),
                Span::raw(summary.as_str()).fg(w.styles.text()),
            ]));
            Some(())
        }
        MessageContent::TeammateMessage { teammate, content } => {
            let parsed = parse_teammate_xml(content);
            if parsed.is_empty() {
                lines.push(Line::from(vec![
                    Span::raw(format!("  @{teammate}: ")).fg(w.styles.primary()),
                    Span::raw(content.clone()).fg(w.styles.text()),
                ]));
            } else {
                for part in parsed {
                    let name_color = part
                        .color
                        .as_deref()
                        .map(teammate_color_to_ratatui)
                        .unwrap_or(w.styles.primary());
                    let mut header =
                        vec![Span::raw(format!("  @{}: ", part.teammate_id)).fg(name_color)];
                    if let Some(summary) = part.summary {
                        header.push(Span::raw(format!("({summary}) ")).dim());
                    }
                    lines.push(Line::from(header));
                    for line in part.content.lines() {
                        lines.push(Line::from(vec![
                            Span::raw("    ".to_string()),
                            Span::raw(line.to_string()).fg(w.styles.text()),
                        ]));
                    }
                }
            }
            Some(())
        }
        MessageContent::Attachment {
            attachment_type,
            preview,
        } => {
            lines.push(Line::from(vec![
                Span::raw("❯ ").fg(w.styles.user_message()),
                Span::raw(format!("📎 [{attachment_type}] ")).fg(w.styles.accent()),
                Span::raw(preview.as_str()).fg(w.styles.dim()),
            ]));
            Some(())
        }
        MessageContent::ChannelMessage {
            source,
            user,
            content,
        } => {
            // TS UserChannelMessage: "source ⇢ [user] truncated-body"
            let short_source = source.rsplit(':').next().unwrap_or(source);
            let mut header = vec![
                Span::raw("  ⇢ ").fg(w.styles.accent()),
                Span::raw(short_source.to_string())
                    .fg(w.styles.primary())
                    .bold(),
            ];
            if let Some(u) = user {
                header.push(Span::raw(format!(" [{u}]")).fg(w.styles.dim()));
            }
            lines.push(Line::from(header));
            let body = content.split_whitespace().collect::<Vec<_>>().join(" ");
            let truncated = if body.chars().count() > 60 {
                let mut s = body.chars().take(59).collect::<String>();
                s.push('…');
                s
            } else {
                body
            };
            lines.push(Line::from(
                Span::raw(format!("    {truncated}")).fg(w.styles.text()),
            ));
            Some(())
        }
        MessageContent::ResourceUpdate {
            kind,
            server,
            target,
            reason,
        } => {
            // TS UserResourceUpdateMessage: "↻ server · target (reason)"
            let label = match kind {
                ResourceUpdateKind::Resource => "resource",
                ResourceUpdateKind::Polling => "polling",
            };
            let display_target = format_resource_target(target);
            let mut parts = vec![
                Span::raw("  ↻ ").fg(w.styles.accent()),
                Span::raw(format!("{label} ")).fg(w.styles.dim()),
                Span::raw(server.as_str()).fg(w.styles.primary()),
                Span::raw(" · ").fg(w.styles.dim()),
                Span::raw(display_target).fg(w.styles.text()),
            ];
            if let Some(r) = reason {
                parts.push(Span::raw(format!(" ({r})")).fg(w.styles.dim()).italic());
            }
            lines.push(Line::from(parts));
            Some(())
        }
        _ => None,
    }
}
