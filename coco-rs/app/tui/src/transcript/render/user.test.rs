use crate::transcript::cells::CellKind;
use crate::transcript::cells::RenderedCell;
use coco_messages::LlmMessage;
use coco_messages::Message;
use coco_messages::MessageOrigin;
use coco_messages::UserMessage;
use coco_tui_ui::display::SyntaxHighlighting;
use coco_tui_ui::style::UiStyles;
use coco_tui_ui::theme::Theme;
use std::sync::Arc;
use uuid::Uuid;

fn user_cell(text: &str, origin: Option<MessageOrigin>) -> RenderedCell {
    let msg = Message::User(UserMessage {
        message: LlmMessage::user_text(text),
        uuid: Uuid::new_v4(),
        timestamp: String::new(),
        is_visible_in_transcript_only: origin.is_some(),
        is_virtual: false,
        is_compact_summary: false,
        permission_mode: None,
        origin,
        parent_tool_use_id: None,
    });
    RenderedCell {
        message_uuid: Uuid::new_v4(),
        kind: CellKind::UserText {
            text: text.to_string(),
        },
        source: Arc::new(msg),
    }
}

fn render_user(cell: &RenderedCell) -> Vec<ratatui::text::Line<'static>> {
    let theme = Theme::default();
    let cells: Vec<RenderedCell> = Vec::new();
    let w = crate::transcript::render::CellsRenderer::new(&cells, UiStyles::new(&theme));
    let mut lines = Vec::new();
    super::try_render(&w, cell, &mut lines).expect("user cell renders");
    lines
}

fn flatten(lines: &[ratatui::text::Line<'static>]) -> String {
    lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.content.as_ref())
        .collect()
}

#[test]
fn plan_implementation_origin_renders_compact_chip_not_wall() {
    let plan = "Implement the following plan:\n\n# Trip\n- Day 1\n- Day 2\n\nPlan file path: /Users/x/.coco/plans/keen-knitting-lagoon.md";
    let lines = render_user(&user_cell(plan, Some(MessageOrigin::PlanImplementation)));

    // One compact chip — NOT a multi-line `❯` echo of the whole plan.
    assert_eq!(lines.len(), 1);
    let text = flatten(&lines);
    assert!(text.contains("Implementing approved plan"), "{text}");
    assert!(text.contains("keen-knitting-lagoon.md"), "{text}");
    assert!(!text.contains('❯'), "{text}");
}

#[test]
fn ordinary_multiline_user_text_still_renders_chevron_rows() {
    let lines = render_user(&user_cell(
        "line one\nline two",
        Some(MessageOrigin::UserInput),
    ));
    assert_eq!(lines.len(), 2);
    assert!(flatten(&lines).contains('❯'));
}

#[test]
fn plan_implementation_chip_file_extracts_basename() {
    assert_eq!(
        super::plan_implementation_chip_file("foo\n\nPlan file path: /a/b/c.md"),
        Some("c.md".to_string())
    );
    assert_eq!(super::plan_implementation_chip_file("no marker here"), None);
}

#[test]
fn slash_origin_gates_command_pill_rendering() {
    let echo = "<command-name>/help</command-name>\n<command-args></command-args>";
    let theme = Theme::default();
    let opts = || crate::presentation::slash_command::SlashCommandRenderOptions {
        styles: UiStyles::new(&theme),
        width: 80,
        syntax_highlighting: SyntaxHighlighting::Disabled,
        apply_user_background: false,
    };
    // Genuine slash echo (origin stamped) → eligible for the `❯ /cmd` pill.
    assert!(
        crate::presentation::slash_command::render_slash_command_user_text(
            user_cell(echo, Some(MessageOrigin::SlashCommand))
                .source
                .as_ref(),
            echo,
            opts()
        )
        .is_some()
    );
    // Identical text typed by a user (no slash origin) → NOT a pill; it
    // renders as plain user text so a raw `<command-name>` substring is never
    // mistaken for a command invocation.
    assert!(
        crate::presentation::slash_command::render_slash_command_user_text(
            user_cell(echo, None).source.as_ref(),
            echo,
            opts()
        )
        .is_none()
    );
}
