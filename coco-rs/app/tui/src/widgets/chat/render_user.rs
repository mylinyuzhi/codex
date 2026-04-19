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
            // Optional subtle background tint behind user prompt lines.
            // TS: user-message component uses a terminal-adaptive fill so
            // prompts visually separate from assistant prose. None = inherit.
            for line in text.lines() {
                let mut span = Span::raw(format!("> {line}")).fg(w.theme.user_message);
                if let Some(bg) = w.theme.user_message_bg {
                    span = span.bg(bg);
                }
                lines.push(Line::from(span));
            }
            Some(())
        }
        MessageContent::Image { path } => {
            lines.push(Line::from(vec![
                Span::raw("> ").fg(w.theme.user_message),
                Span::raw("📎 ").fg(w.theme.accent),
                Span::raw(path.as_str()).fg(w.theme.primary).underlined(),
            ]));
            Some(())
        }
        MessageContent::BashInput { command } => {
            lines.push(Line::from(vec![
                Span::raw("> $ ").fg(w.theme.user_message),
                Span::raw(command.as_str()).fg(w.theme.accent),
            ]));
            Some(())
        }
        MessageContent::BashOutput { output, exit_code } => {
            let color = if *exit_code == 0 {
                w.theme.text_dim
            } else {
                w.theme.error
            };
            for line in output.lines().take(20) {
                lines.push(Line::from(Span::raw(format!("  {line}")).fg(color)));
            }
            if output.lines().count() > 20 {
                lines.push(Line::from(
                    Span::raw(t!("chat.truncated").to_string())
                        .fg(w.theme.text_dim)
                        .italic(),
                ));
            }
            if *exit_code != 0 {
                lines.push(Line::from(
                    Span::raw(t!("chat.exit_code", code = exit_code).to_string()).fg(w.theme.error),
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
                Span::raw(format!("  {text}"))
                    .fg(w.theme.plan_mode)
                    .italic(),
            ));
            Some(())
        }
        MessageContent::MemoryInput { content } => {
            lines.push(Line::from(vec![
                Span::raw("> ").fg(w.theme.user_message),
                Span::raw("💾 ").fg(w.theme.accent),
                Span::raw(content.as_str()).fg(w.theme.text_dim),
            ]));
            Some(())
        }
        MessageContent::AgentNotification { agent_id, summary } => {
            lines.push(Line::from(vec![
                Span::raw("  🤖 ").fg(w.theme.accent),
                Span::raw(format!("[{agent_id}] ")).fg(w.theme.text_dim),
                Span::raw(summary.as_str()).fg(w.theme.text),
            ]));
            Some(())
        }
        MessageContent::TeammateMessage { teammate, content } => {
            let parsed = parse_teammate_xml(content);
            if parsed.is_empty() {
                lines.push(Line::from(vec![
                    Span::raw(format!("  @{teammate}: ")).fg(w.theme.primary),
                    Span::raw(content.clone()).fg(w.theme.text),
                ]));
            } else {
                for part in parsed {
                    let name_color = part
                        .color
                        .as_deref()
                        .map(teammate_color_to_ratatui)
                        .unwrap_or(w.theme.primary);
                    let mut header =
                        vec![Span::raw(format!("  @{}: ", part.teammate_id)).fg(name_color)];
                    if let Some(summary) = part.summary {
                        header.push(Span::raw(format!("({summary}) ")).dim());
                    }
                    lines.push(Line::from(header));
                    for line in part.content.lines() {
                        lines.push(Line::from(vec![
                            Span::raw("    ".to_string()),
                            Span::raw(line.to_string()).fg(w.theme.text),
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
                Span::raw("> ").fg(w.theme.user_message),
                Span::raw(format!("📎 [{attachment_type}] ")).fg(w.theme.accent),
                Span::raw(preview.as_str()).fg(w.theme.text_dim),
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
                Span::raw("  ⇢ ").fg(w.theme.accent),
                Span::raw(short_source.to_string())
                    .fg(w.theme.primary)
                    .bold(),
            ];
            if let Some(u) = user {
                header.push(Span::raw(format!(" [{u}]")).fg(w.theme.text_dim));
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
                Span::raw(format!("    {truncated}")).fg(w.theme.text),
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
                Span::raw("  ↻ ").fg(w.theme.accent),
                Span::raw(format!("{label} ")).fg(w.theme.text_dim),
                Span::raw(server.as_str()).fg(w.theme.primary),
                Span::raw(" · ").fg(w.theme.text_dim),
                Span::raw(display_target).fg(w.theme.text),
            ];
            if let Some(r) = reason {
                parts.push(Span::raw(format!(" ({r})")).fg(w.theme.text_dim).italic());
            }
            lines.push(Line::from(parts));
            Some(())
        }
        _ => None,
    }
}
