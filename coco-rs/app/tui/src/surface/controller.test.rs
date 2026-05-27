use pretty_assertions::assert_eq;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::*;
use crate::state::derive::test_helpers;
use crate::state::ui::StreamingState;
use crate::surface::modal::HistorySurfaceMode;
use crate::surface::modal::SurfaceFramePlan;

#[test]
fn native_draw_does_not_duplicate_header_across_streaming_redraws() {
    let backend = TestBackend::new(64, 14);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    apply_native_viewport(&mut terminal, Rect::new(0, 10, 64, 4));
    let mut state = AppState::new();
    state.session.provider = "deepseek-openai".to_string();
    state.session.model = "deepseek-v4-flash".to_string();
    let mut controller = NativeSurfaceController::new();
    let t0 = std::time::Instant::now();

    controller
        .draw_at(&mut terminal, &state, t0)
        .expect("startup draw");

    test_helpers::push_user_text(&mut state.session, "u1", "hello");
    let mut streaming = StreamingState::new();
    streaming.append_text("Hi");
    streaming.reveal_all();
    state.ui.streaming = Some(streaming);
    apply_native_viewport(&mut terminal, Rect::new(0, 8, 64, 6));
    controller
        .draw_at(
            &mut terminal,
            &state,
            t0 + std::time::Duration::from_millis(100),
        )
        .expect("first stream draw");

    let streaming = state.ui.streaming.as_mut().expect("streaming state");
    streaming.append_text(" there");
    streaming.reveal_all();
    apply_native_viewport(&mut terminal, Rect::new(0, 7, 64, 7));
    controller
        .draw_at(
            &mut terminal,
            &state,
            t0 + std::time::Duration::from_millis(200),
        )
        .expect("second stream draw");

    state.ui.streaming = None;
    test_helpers::push_assistant_text(&mut state.session, "Hi there");
    apply_native_viewport(&mut terminal, Rect::new(0, 10, 64, 4));
    controller
        .draw_at(
            &mut terminal,
            &state,
            t0 + std::time::Duration::from_millis(300),
        )
        .expect("final draw");

    let text = plain_terminal_text(&terminal);
    assert_eq!(text.matches("COCO").count(), 1, "{text}");
    assert_eq!(text.matches("❯ hello").count(), 1, "{text}");
    assert_eq!(text.matches("⏺ Hi there").count(), 1, "{text}");

    let lines = plain_terminal_lines(&terminal);
    let header_last = line_index(&lines, "╰─╯");
    let user = line_index(&lines, "❯ hello");
    let assistant = line_index(&lines, "⏺ Hi there");
    assert_eq!(user, header_last + 2, "{text}");
    assert!(lines[header_last + 1].trim().is_empty(), "{text}");
    assert_eq!(assistant, user + 2, "{text}");
    assert!(lines[user + 1].trim().is_empty(), "{text}");
}

#[test]
fn native_draw_emits_session_header_on_startup() {
    let backend = TestBackend::new(64, 8);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 4, 64, 4));
    let mut state = AppState::new();
    state.session.provider = "deepseek-openai".to_string();
    state.session.model = "deepseek-v4-flash".to_string();
    let mut controller = NativeSurfaceController::new();

    let outcome = controller.draw(&mut terminal, &state).expect("draw");

    assert_eq!(
        outcome.history,
        HistoryEmissionOutcome::Appended {
            start: 0,
            message_count: 0,
            rows: 4,
        }
    );
    let text = plain_terminal_text(&terminal);
    assert!(text.contains("COCO"));
    assert!(text.contains("deepseek-openai/deepseek-v4-flash"));
}

#[test]
fn native_draw_appends_finalized_history_and_keeps_live_tail_in_viewport() {
    let backend = TestBackend::new(48, 11);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 5, 48, 6));
    let mut state = AppState::new();
    test_helpers::push_assistant_text(&mut state.session, "finalized");
    let mut streaming = StreamingState::new();
    streaming.append_text("live response");
    streaming.reveal_all();
    state.ui.streaming = Some(streaming);
    let mut controller = NativeSurfaceController::new();

    let outcome = controller.draw(&mut terminal, &state).expect("draw");

    assert_eq!(
        outcome.history,
        HistoryEmissionOutcome::Appended {
            start: 0,
            message_count: 1,
            rows: 6,
        }
    );
    let text = plain_terminal_text(&terminal);
    assert!(text.contains("COCO"));
    assert!(text.contains("live response"), "{text}");
}

#[test]
fn native_draw_replays_history_when_source_prefix_diverges() {
    let backend = TestBackend::new(48, 8);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 5, 48, 3));
    let mut state = AppState::new();
    test_helpers::push_assistant_text(&mut state.session, "one");
    let mut controller = NativeSurfaceController::new();
    controller.draw(&mut terminal, &state).expect("first draw");

    // Reset the engine-authoritative transcript so the prefix-divergence
    // path fires (the renderer reads cells).
    state.session.transcript.on_session_reset();
    test_helpers::push_assistant_text(&mut state.session, "two");
    let outcome = controller.draw(&mut terminal, &state).expect("replay");

    assert_eq!(
        outcome.history,
        HistoryEmissionOutcome::Replayed {
            message_count: 1,
            rows: 6,
        }
    );
    let text = plain_buffer_lines(terminal.backend().buffer()).join("\n");
    assert!(!text.contains("one"));
    assert!(text.contains("two"));
}

#[test]
fn native_draw_replays_history_when_thinking_display_changes() {
    let backend = TestBackend::new(64, 10);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 6, 64, 4));
    let mut state = AppState::new();
    test_helpers::push_assistant_thinking(&mut state.session, "Need to inspect files.", 1300, 15);
    let mut controller = NativeSurfaceController::new();
    controller.draw(&mut terminal, &state).expect("first draw");

    state.ui.show_thinking = true;
    let outcome = controller
        .draw(&mut terminal, &state)
        .expect("thinking replay");

    assert!(matches!(
        outcome.history,
        HistoryEmissionOutcome::Replayed {
            message_count: 1,
            ..
        }
    ));
    let text = plain_buffer_lines(terminal.backend().buffer()).join("\n");
    assert!(text.contains("Need to inspect files."));
}

#[test]
fn native_draw_replays_history_when_reasoning_metadata_changes() {
    let backend = TestBackend::new(64, 10);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 6, 64, 4));
    let mut state = AppState::new();
    let msg = coco_messages::create_assistant_message(
        vec![coco_messages::AssistantContent::Reasoning(
            coco_messages::ReasoningContent::new("Need to inspect files."),
        )],
        "test-model",
        coco_types::TokenUsage::default(),
    );
    let uuid = match &msg {
        coco_messages::Message::Assistant(a) => a.uuid,
        _ => unreachable!("create_assistant_message yields Assistant"),
    };
    state
        .session
        .transcript
        .on_message_appended(std::sync::Arc::new(msg));
    let mut controller = NativeSurfaceController::new();
    controller.draw(&mut terminal, &state).expect("first draw");
    let text = plain_buffer_lines(terminal.backend().buffer()).join("\n");
    assert!(text.contains("F2 to expand"));
    assert!(!text.contains("reasoning tokens"));

    state.session.insert_reasoning_metadata(
        uuid,
        crate::state::session::ReasoningMetadata {
            duration_ms: None,
            reasoning_tokens: 22,
        },
    );
    let outcome = controller
        .draw(&mut terminal, &state)
        .expect("metadata replay");

    assert!(matches!(
        outcome.history,
        HistoryEmissionOutcome::Replayed {
            message_count: 1,
            ..
        }
    ));
    let text = plain_buffer_lines(terminal.backend().buffer()).join("\n");
    assert!(text.contains("22 reasoning tokens"), "{text}");
    assert!(text.contains("F2 to expand"), "{text}");
}

#[test]
fn native_draw_replays_after_resize_requested_during_stream_finishes() {
    let backend = TestBackend::new(48, 8);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 5, 20, 3));
    let mut state = AppState::new();
    state.ui.streaming = Some(StreamingState::new());
    let mut controller = NativeSurfaceController::new();

    controller
        .draw(&mut terminal, &state)
        .expect("initial draw");
    terminal.set_viewport_area(Rect::new(0, 5, 30, 3));
    controller.draw(&mut terminal, &state).expect("resize draw");

    state.ui.streaming = None;
    test_helpers::push_assistant_text(&mut state.session, "done");
    let outcome = controller.draw(&mut terminal, &state).expect("finish draw");

    assert_eq!(
        outcome.history,
        HistoryEmissionOutcome::Replayed {
            message_count: 1,
            rows: 6,
        }
    );
    let text = plain_buffer_lines(terminal.backend().buffer()).join("\n");
    assert!(text.contains("done"));
}

#[test]
fn native_draw_stream_finish_replay_does_not_leave_gap_before_input() {
    let backend = TestBackend::new(64, 30);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 26, 64, 4));
    let mut state = AppState::new();
    let mut controller = NativeSurfaceController::new();

    controller
        .draw(&mut terminal, &state)
        .expect("initial draw");

    test_helpers::push_user_text(&mut state.session, "u1", "hello");
    let mut streaming = StreamingState::new();
    streaming.append_text("short reply");
    streaming.reveal_all();
    state.ui.streaming = Some(streaming);
    apply_native_viewport(&mut terminal, Rect::new(0, 18, 64, 12));
    controller.draw(&mut terminal, &state).expect("stream draw");

    state.ui.streaming = None;
    test_helpers::push_assistant_text(&mut state.session, "short reply");
    apply_native_viewport(&mut terminal, Rect::new(0, 26, 64, 4));
    let outcome = controller.draw(&mut terminal, &state).expect("finish draw");

    assert!(matches!(
        outcome.history,
        HistoryEmissionOutcome::Replayed { .. }
    ));
    let lines = plain_terminal_lines(&terminal);
    let assistant = line_index(&lines, "⏺ short reply");
    let input = empty_input_index_after(&lines, assistant);
    let gap = input.saturating_sub(assistant + 1);
    assert!(
        gap <= 3,
        "stream-finish replay left {gap} rows before input:\n{}",
        lines.join("\n")
    );
}

#[test]
fn native_draw_replays_after_viewport_height_change_debounce() {
    let backend = TestBackend::new(48, 12);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 7, 48, 3));
    let mut state = AppState::new();
    test_helpers::push_assistant_text(&mut state.session, "height replay");
    let mut controller = NativeSurfaceController::new();
    controller
        .draw(&mut terminal, &state)
        .expect("initial draw");

    terminal.set_viewport_area(Rect::new(0, 7, 48, 4));
    let height_change = controller
        .draw(&mut terminal, &state)
        .expect("height change draw");
    assert_eq!(height_change.history, HistoryEmissionOutcome::Noop);

    let outcome = controller
        .draw_at(
            &mut terminal,
            &state,
            std::time::Instant::now() + std::time::Duration::from_millis(100),
        )
        .expect("replay draw");

    assert_eq!(
        outcome.history,
        HistoryEmissionOutcome::Replayed {
            message_count: 1,
            rows: 6,
        }
    );
}

#[test]
fn native_draw_defers_history_while_modal_is_open() {
    let backend = TestBackend::new(48, 8);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 5, 48, 3));
    let mut state = AppState::new();
    test_helpers::push_assistant_text(&mut state.session, "deferred");
    state.ui.show_modal(crate::state::ModalState::Help);
    let mut controller = NativeSurfaceController::new();

    let outcome = controller.draw(&mut terminal, &state).expect("draw");

    assert_eq!(outcome.history, HistoryEmissionOutcome::Noop);
    let text = plain_buffer_lines(terminal.backend().buffer()).join("\n");
    assert!(!text.contains("deferred"));
}

#[test]
fn native_draw_renders_finalized_history_in_viewport_when_terminal_is_incompatible() {
    let backend = TestBackend::new(48, 12);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 0, 48, 12));
    let mut state = AppState::new();
    test_helpers::push_assistant_text(&mut state.session, "zellij deferred");
    let mut controller = NativeSurfaceController::new();
    let plan = SurfaceFramePlan {
        modal_placement: None,
        history_surface: HistorySurfaceMode::Viewport,
        attention_requested: false,
    };

    let outcome = controller
        .draw_with_plan(&mut terminal, &state, plan)
        .expect("draw");

    assert_eq!(outcome.history, HistoryEmissionOutcome::Noop);
    let text = plain_buffer_lines(terminal.backend().buffer()).join("\n");
    assert!(text.contains("zellij deferred"));
}

fn plain_buffer_lines(buffer: &Buffer) -> Vec<String> {
    buffer
        .content
        .chunks(buffer.area.width as usize)
        .map(|cells| cells.iter().map(ratatui::buffer::Cell::symbol).collect())
        .collect()
}

fn plain_terminal_text(terminal: &SurfaceTerminal<TestBackend>) -> String {
    plain_terminal_lines(terminal).join("\n")
}

fn plain_terminal_lines(terminal: &SurfaceTerminal<TestBackend>) -> Vec<String> {
    let mut lines = plain_buffer_lines(terminal.backend().scrollback());
    lines.extend(plain_buffer_lines(terminal.backend().buffer()));
    lines
}

fn line_index(lines: &[String], needle: &str) -> usize {
    lines
        .iter()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("missing {needle:?} in {lines:#?}"))
}

fn empty_input_index_after(lines: &[String], after: usize) -> usize {
    lines
        .iter()
        .enumerate()
        .skip(after + 1)
        .find_map(|(index, line)| (line.trim() == "❯").then_some(index))
        .unwrap_or_else(|| panic!("missing empty input prompt after row {after} in {lines:#?}"))
}

fn apply_native_viewport(terminal: &mut SurfaceTerminal<TestBackend>, area: Rect) {
    terminal
        .apply_viewport_area(area, true)
        .expect("apply viewport area");
}
