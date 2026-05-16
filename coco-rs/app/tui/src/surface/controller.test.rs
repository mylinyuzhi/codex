use pretty_assertions::assert_eq;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::*;
use crate::state::overlay::TranscriptOverlay;
use crate::state::session::ChatMessage;
use crate::state::ui::StreamingState;

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

    state
        .session
        .messages
        .push(ChatMessage::user_text("u1", "hello"));
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
    state
        .session
        .messages
        .push(ChatMessage::assistant_text("a1", "Hi there"));
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
    let text = plain_buffer_lines(terminal.backend().buffer()).join("\n");
    assert!(text.contains("COCO"));
    assert!(text.contains("deepseek-openai/deepseek-v4-flash"));
}

#[test]
fn native_draw_appends_finalized_history_and_keeps_live_tail_in_viewport() {
    let backend = TestBackend::new(48, 11);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 5, 48, 6));
    let mut state = AppState::new();
    state
        .session
        .messages
        .push(ChatMessage::assistant_text("a1", "finalized"));
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
            rows: 5,
        }
    );
    let text = plain_buffer_lines(terminal.backend().buffer()).join("\n");
    assert!(text.find("COCO").unwrap() < text.find("finalized").unwrap());
    assert!(text.contains("finalized"));
    assert!(text.contains("live response"), "{text}");
    assert_eq!(text.matches("finalized").count(), 1);
}

#[test]
fn native_draw_replays_history_when_source_prefix_diverges() {
    let backend = TestBackend::new(48, 8);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 5, 48, 3));
    let mut state = AppState::new();
    state
        .session
        .messages
        .push(ChatMessage::assistant_text("a1", "one"));
    let mut controller = NativeSurfaceController::new();
    controller.draw(&mut terminal, &state).expect("first draw");

    state.session.messages = vec![ChatMessage::assistant_text("a2", "two")];
    let outcome = controller.draw(&mut terminal, &state).expect("replay");

    assert_eq!(
        outcome.history,
        HistoryEmissionOutcome::Replayed {
            message_count: 1,
            rows: 5,
        }
    );
    let text = plain_buffer_lines(terminal.backend().buffer()).join("\n");
    assert!(!text.contains("one"));
    assert!(text.contains("two"));
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
    state
        .session
        .messages
        .push(ChatMessage::assistant_text("a1", "done"));
    let outcome = controller.draw(&mut terminal, &state).expect("finish draw");

    assert_eq!(
        outcome.history,
        HistoryEmissionOutcome::Replayed {
            message_count: 1,
            rows: 5,
        }
    );
    let text = plain_buffer_lines(terminal.backend().buffer()).join("\n");
    assert!(text.contains("done"));
}

#[test]
fn native_draw_does_not_replay_after_viewport_height_changes() {
    let backend = TestBackend::new(48, 12);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 7, 48, 3));
    let mut state = AppState::new();
    state
        .session
        .messages
        .push(ChatMessage::assistant_text("a1", "height replay"));
    let mut controller = NativeSurfaceController::new();
    controller
        .draw(&mut terminal, &state)
        .expect("initial draw");

    terminal.set_viewport_area(Rect::new(0, 7, 48, 4));
    let height_change = controller
        .draw(&mut terminal, &state)
        .expect("height change draw");
    assert_eq!(height_change.history, HistoryEmissionOutcome::Noop);

    state
        .session
        .messages
        .push(ChatMessage::assistant_text("a2", "after resize"));
    let outcome = controller.draw(&mut terminal, &state).expect("replay draw");

    assert_eq!(
        outcome.history,
        HistoryEmissionOutcome::Appended {
            start: 1,
            message_count: 1,
            rows: 2,
        }
    );
}

#[test]
fn native_draw_defers_history_while_overlay_is_open() {
    let backend = TestBackend::new(48, 8);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 5, 48, 3));
    let mut state = AppState::new();
    state
        .session
        .messages
        .push(ChatMessage::assistant_text("a1", "deferred"));
    state
        .ui
        .set_overlay(crate::state::Overlay::Transcript(TranscriptOverlay::new()));
    let mut controller = NativeSurfaceController::new();

    let outcome = controller.draw(&mut terminal, &state).expect("draw");

    assert_eq!(outcome.history, HistoryEmissionOutcome::Noop);
    let text = plain_buffer_lines(terminal.backend().buffer()).join("\n");
    assert!(!text.contains("deferred"));
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

fn apply_native_viewport(terminal: &mut SurfaceTerminal<TestBackend>, area: Rect) {
    terminal
        .apply_viewport_area(area, true)
        .expect("apply viewport area");
}
