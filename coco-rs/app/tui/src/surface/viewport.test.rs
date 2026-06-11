use pretty_assertions::assert_eq;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::*;
use crate::state::ui::StreamingState;
use crate::surface::modal::HistorySurfaceMode;
use crate::transcript::derive::test_helpers;
use coco_tui_ui::engine::terminal::SurfaceTerminal;

#[test]
fn interactive_viewport_does_not_render_session_header() {
    let backend = TestBackend::new(48, 6);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 0, 48, 6));
    let state = AppState::new();
    let mut transcript_layout = crate::widgets::TranscriptLayoutIndex::default();

    terminal
        .draw_viewport(|frame| {
            render_interactive_viewport(frame, &state, native_plan(), &mut transcript_layout, None);
        })
        .expect("draw");

    let text = plain_buffer_lines(terminal.backend().buffer()).join("\n");
    assert!(!text.contains("COCO"));
    assert!(!text.contains("Type a message"));
    assert!(text.contains("❯"));
}

#[test]
fn interactive_viewport_desired_height_tracks_idle_composer() {
    let state = AppState::new();

    assert_eq!(
        interactive_viewport_desired_height(&state, 48, 12, native_plan(), None),
        4
    );
}

#[test]
fn interactive_viewport_desired_height_never_exceeds_cap() {
    let state = AppState::new();

    assert_eq!(
        interactive_viewport_desired_height(&state, 48, 2, native_plan(), None),
        2
    );
}

#[test]
fn interactive_viewport_popup_height_is_stable_for_short_and_long_lists() {
    let short = state_with_popup_items(2);
    let medium = state_with_popup_items(5);
    let full = state_with_popup_items(SuggestionPopup::DEFAULT_MAX_VISIBLE as usize);

    let short_height = interactive_viewport_desired_height(&short, 48, 24, native_plan(), None);
    let medium_height = interactive_viewport_desired_height(&medium, 48, 24, native_plan(), None);
    let full_height = interactive_viewport_desired_height(&full, 48, 24, native_plan(), None);

    assert_eq!(short_height, 13);
    assert_eq!(medium_height, short_height);
    assert_eq!(full_height, short_height);
}

#[test]
fn interactive_viewport_popup_height_is_capped_by_terminal_height() {
    let state = state_with_popup_items(SuggestionPopup::DEFAULT_MAX_VISIBLE as usize);

    assert_eq!(
        interactive_viewport_desired_height(&state, 48, 8, native_plan(), None),
        8
    );
}

#[test]
fn interactive_viewport_does_not_render_finalized_messages() {
    let backend = TestBackend::new(48, 8);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 0, 48, 8));
    let mut state = AppState::new();
    test_helpers::push_assistant_text(&mut state.session, "finalized history");
    let mut transcript_layout = crate::widgets::TranscriptLayoutIndex::default();

    terminal
        .draw_viewport(|frame| {
            render_interactive_viewport(frame, &state, native_plan(), &mut transcript_layout, None);
        })
        .expect("draw");

    let text = plain_buffer_lines(terminal.backend().buffer()).join("\n");
    assert!(!text.contains("finalized history"));
}

#[test]
fn interactive_viewport_renders_finalized_messages_in_viewport_history_mode() {
    let backend = TestBackend::new(48, 8);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 0, 48, 8));
    let mut state = AppState::new();
    test_helpers::push_assistant_text(&mut state.session, "fallback history");
    let mut transcript_layout = crate::widgets::TranscriptLayoutIndex::default();

    terminal
        .draw_viewport(|frame| {
            render_interactive_viewport(
                frame,
                &state,
                viewport_history_plan(),
                &mut transcript_layout,
                None,
            );
        })
        .expect("draw");

    let text = plain_buffer_lines(terminal.backend().buffer()).join("\n");
    assert!(text.contains("fallback history"));
}

#[test]
fn interactive_viewport_renders_active_streaming_tail() {
    let backend = TestBackend::new(48, 10);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 0, 48, 10));
    let mut state = AppState::new();
    let mut streaming = StreamingState::new();
    streaming.append_text("live response");
    streaming.reveal_all();
    state.ui.streaming = Some(streaming);
    let mut transcript_layout = crate::widgets::TranscriptLayoutIndex::default();

    terminal
        .draw_viewport(|frame| {
            render_interactive_viewport(frame, &state, native_plan(), &mut transcript_layout, None);
        })
        .expect("draw");

    let text = plain_buffer_lines(terminal.backend().buffer()).join("\n");
    assert!(text.contains("live response"));
}

#[test]
fn interactive_viewport_reports_input_rect_for_cursor_policy() {
    let backend = TestBackend::new(48, 8);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 0, 48, 8));
    let state = AppState::new();
    let mut layout = FrameLayout::default();
    let mut transcript_layout = crate::widgets::TranscriptLayoutIndex::default();

    terminal
        .draw_viewport(|frame| {
            layout = render_interactive_viewport(
                frame,
                &state,
                native_plan(),
                &mut transcript_layout,
                None,
            );
        })
        .expect("draw");

    assert_eq!(layout.input.height, 3);
    assert_eq!(layout.input.width, 48);
}

#[test]
fn question_prompt_sets_input_height_to_zero() {
    let backend = TestBackend::new(48, 10);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 0, 48, 10));
    let state = question_state(vec![question_item("Short", "Short?", 1)]);
    let mut layout = FrameLayout::default();
    let mut transcript_layout = crate::widgets::TranscriptLayoutIndex::default();

    terminal
        .draw_viewport(|frame| {
            layout = render_interactive_viewport(
                frame,
                &state,
                native_plan(),
                &mut transcript_layout,
                None,
            );
        })
        .expect("draw");

    assert_eq!(layout.input.height, 0);
}

#[test]
fn question_prompt_uses_full_viewport_width() {
    let backend = TestBackend::new(140, 14);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 0, 140, 14));
    let state = question_state(vec![question_item("Short", "Short?", 1)]);
    let mut layout = FrameLayout::default();
    let mut transcript_layout = crate::widgets::TranscriptLayoutIndex::default();

    terminal
        .draw_viewport(|frame| {
            layout = render_interactive_viewport(
                frame,
                &state,
                native_plan(),
                &mut transcript_layout,
                None,
            );
        })
        .expect("draw");

    assert_eq!(layout.question_prompt.width, 140);
}

#[test]
fn question_prompt_desired_height_uses_tallest_question_tab() {
    let state = question_state(vec![
        question_item("Short", "Short?", 1),
        question_item("Long", &"Long question ".repeat(20), 4),
    ]);
    let short_only = question_state(vec![question_item("Short", "Short?", 1)]);

    let with_tall_tab = interactive_viewport_desired_height(&state, 48, 24, native_plan(), None);
    let short_height =
        interactive_viewport_desired_height(&short_only, 48, 24, native_plan(), None);

    assert!(
        with_tall_tab > short_height,
        "question prompt should reserve tallest tab height"
    );
}

#[test]
fn compact_prompt_body_preserves_tail_action_block() {
    let body = "\
Execute shell command

Command:
  rm -rf /tmp/test

Risk:
  Removes files recursively

Actions:
▸ Yes, approve once
  Yes, always allow Bash for this session
  No, deny
↑/↓ Navigate  Enter Select  Y/N/A shortcuts";

    let compact = compact_prompt_body(body, 7);

    assert_eq!(
        compact,
        "\
Execute shell command
...
Actions:
▸ Yes, approve once
  Yes, always allow Bash for this session
  No, deny
↑/↓ Navigate  Enter Select  Y/N/A shortcuts"
    );
}

fn native_plan() -> SurfaceFramePlan {
    SurfaceFramePlan {
        modal_placement: None,
        history_surface: HistorySurfaceMode::NativeScrollback,
        attention_requested: false,
    }
}

fn viewport_history_plan() -> SurfaceFramePlan {
    SurfaceFramePlan {
        history_surface: HistorySurfaceMode::Viewport,
        ..native_plan()
    }
}

fn question_state(items: Vec<crate::state::QuestionItem>) -> AppState {
    let mut state = AppState::new();
    state
        .ui
        .push_prompt(crate::state::PanePromptState::Question(
            crate::state::QuestionPromptState {
                request_id: "q".into(),
                original_input: serde_json::json!({}),
                questions: items,
                current_question: crate::state::QuestionPage::Question(0),
                focus_target: crate::state::QuestionFocusTarget::QuestionOption(0),
                is_in_plan_mode: false,
            },
        ));
    state
}

fn question_item(header: &str, question: &str, option_count: usize) -> crate::state::QuestionItem {
    crate::state::QuestionItem {
        header: header.into(),
        question: question.into(),
        options: (0..option_count)
            .map(|idx| crate::state::QuestionOption {
                label: format!("Option {}", idx + 1),
                description: "description".into(),
                preview: None,
            })
            .collect(),
        multi_select: false,
        selected: None,
        checked: Vec::new(),
        other_input: crate::state::OtherInputState::default(),
    }
}

fn state_with_popup_items(count: usize) -> AppState {
    let mut state = AppState::new();
    let items = (0..count)
        .map(|idx| crate::widgets::suggestion_popup::SuggestionItem {
            label: format!("src/{idx}.rs"),
            description: None,
            metadata: Some(crate::widgets::suggestion_popup::SuggestionMeta::Path {
                is_directory: false,
            }),
        })
        .collect::<Vec<_>>();
    state.ui.completion.set_active(
        crate::state::ActiveSuggestions {
            kind: crate::state::SuggestionKind::At,
            items,
            selected: 0,
            query: "s".into(),
            trigger_pos: 0,
        },
        0..2,
        "@s".into(),
    );
    state.ui.sync_popup_from_active_suggestions();
    state
}

fn plain_buffer_lines(buffer: &Buffer) -> Vec<String> {
    buffer
        .content
        .chunks(buffer.area.width as usize)
        .map(|cells| cells.iter().map(ratatui::buffer::Cell::symbol).collect())
        .collect()
}
