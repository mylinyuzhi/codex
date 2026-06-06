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
    let mut controller = NativeSurfaceController::default();
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
    let mut controller = NativeSurfaceController::default();

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
    let mut controller = NativeSurfaceController::default();

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
fn native_draw_provisionally_appends_stable_stream_and_consolidates_final_message() {
    let backend = TestBackend::new(64, 18);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 12, 64, 6));
    let mut state = AppState::new();
    let mut controller = NativeSurfaceController::default();
    controller
        .draw(&mut terminal, &state)
        .expect("initial draw");

    let mut streaming = StreamingState::new();
    streaming.append_text("alpha\n\nbeta");
    streaming.reveal_all();
    state.ui.streaming = Some(streaming);
    controller.draw(&mut terminal, &state).expect("stream draw");

    let streaming_text = plain_terminal_text(&terminal);
    assert_eq!(
        streaming_text.matches("alpha").count(),
        1,
        "{streaming_text}"
    );
    assert_eq!(
        streaming_text.matches("beta").count(),
        1,
        "{streaming_text}"
    );

    state.ui.streaming = None;
    test_helpers::push_assistant_text(&mut state.session, "alpha\n\nbeta");
    controller
        .draw(&mut terminal, &state)
        .expect("final consolidation draw");

    let final_text = plain_terminal_text(&terminal);
    assert_eq!(final_text.matches("alpha").count(), 1, "{final_text}");
    assert_eq!(final_text.matches("beta").count(), 1, "{final_text}");
}

#[test]
fn native_draw_keeps_stable_stream_visible_when_provisional_append_writes_no_rows() {
    let backend = TestBackend::new(64, 12);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 0, 64, 12));
    let mut state = AppState::new();
    let mut controller = NativeSurfaceController::default();
    let plan = SurfaceFramePlan {
        modal_placement: None,
        history_surface: HistorySurfaceMode::NativeScrollback,
        attention_requested: false,
    };

    let mut streaming = StreamingState::new();
    streaming.append_text("alpha\n\nbeta");
    streaming.reveal_all();
    state.ui.streaming = Some(streaming);
    controller
        .draw_with_plan(&mut terminal, &state, plan, None)
        .expect("stream draw");

    assert_eq!(terminal.visible_history_rows(), 0);
    let text = plain_buffer_lines(terminal.backend().buffer()).join("\n");
    assert_eq!(text.matches("alpha").count(), 1, "{text}");
    assert_eq!(text.matches("beta").count(), 1, "{text}");
}

#[test]
fn native_draw_repairs_provisional_append_after_mid_stream_resize() {
    let backend = TestBackend::new(64, 18);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 12, 64, 6));
    let mut state = AppState::new();
    let mut controller = NativeSurfaceController::default();
    controller
        .draw(&mut terminal, &state)
        .expect("initial draw");

    let mut streaming = StreamingState::new();
    streaming.append_text("alpha\n\nbeta\ngamma");
    streaming.reveal_all();
    state.ui.streaming = Some(streaming);
    controller.draw(&mut terminal, &state).expect("stream draw");

    terminal.set_viewport_area(Rect::new(0, 12, 32, 6));
    controller
        .draw(&mut terminal, &state)
        .expect("resize stream draw");

    let resized_text = plain_terminal_text(&terminal);
    assert_eq!(resized_text.matches("alpha").count(), 1, "{resized_text}");
    assert_eq!(resized_text.matches("gamma").count(), 1, "{resized_text}");

    state.ui.streaming = None;
    test_helpers::push_assistant_text(&mut state.session, "alpha\n\nbeta\ngamma");
    let outcome = controller
        .draw(&mut terminal, &state)
        .expect("final consolidation draw");

    assert!(matches!(
        outcome.history,
        HistoryEmissionOutcome::Replayed { .. } | HistoryEmissionOutcome::Appended { .. }
    ));
    let final_text = plain_terminal_text(&terminal);
    assert_eq!(final_text.matches("alpha").count(), 1, "{final_text}");
    assert_eq!(final_text.matches("beta").count(), 1, "{final_text}");
    assert_eq!(final_text.matches("gamma").count(), 1, "{final_text}");
}

#[test]
fn native_draw_repins_history_on_requested_replay() {
    let backend = TestBackend::new(48, 9);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    let mut state = AppState::new();
    let mut controller = NativeSurfaceController::default();
    controller
        .draw(&mut terminal, &state)
        .expect("initial draw");

    test_helpers::push_assistant_text(&mut state.session, "alpha\n\nbeta");
    controller.draw(&mut terminal, &state).expect("append draw");

    // A turn-end relax requests a re-pin. With nothing else changed the next
    // draw would normally be a no-op; the flag must force a full replay so
    // finalized content re-seats the viewport at the bottom of native scrollback.
    controller.request_repin_replay();
    let outcome = controller.draw(&mut terminal, &state).expect("repin draw");
    assert!(matches!(
        outcome.history,
        HistoryEmissionOutcome::Replayed { .. }
    ));
    let text = plain_terminal_text(&terminal);
    assert_eq!(text.matches("alpha").count(), 1, "{text}");
    assert_eq!(text.matches("beta").count(), 1, "{text}");

    // One-shot: a follow-up draw with nothing changed must NOT replay again —
    // the flag was consumed, so the re-pin fires at most once per request.
    let after = controller
        .draw(&mut terminal, &state)
        .expect("post-repin draw");
    assert!(
        !matches!(after.history, HistoryEmissionOutcome::Replayed { .. }),
        "pending_repin must be one-shot"
    );
}

#[test]
fn native_draw_replays_when_provisional_render_key_differs_on_finalize() {
    let backend = TestBackend::new(64, 18);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 12, 64, 6));
    let mut state = AppState::new();
    let mut controller = NativeSurfaceController::default();
    controller
        .draw(&mut terminal, &state)
        .expect("initial draw");

    let mut streaming = StreamingState::new();
    streaming.append_text("alpha\n\nbeta");
    streaming.reveal_all();
    state.ui.streaming = Some(streaming);
    controller.draw(&mut terminal, &state).expect("stream draw");

    state.ui.theme.accent = ratatui::style::Color::LightRed;
    state.ui.streaming = None;
    test_helpers::push_assistant_text(&mut state.session, "alpha\n\nbeta");
    let outcome = controller
        .draw(&mut terminal, &state)
        .expect("final consolidation draw");

    assert!(matches!(
        outcome.history,
        HistoryEmissionOutcome::Replayed { .. }
    ));
    let final_text = plain_terminal_text(&terminal);
    assert_eq!(final_text.matches("alpha").count(), 1, "{final_text}");
    assert_eq!(final_text.matches("beta").count(), 1, "{final_text}");
}

#[test]
fn native_draw_fast_noops_when_transcript_revision_is_unchanged() {
    let backend = TestBackend::new(48, 9);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 5, 48, 4));
    let mut state = AppState::new();
    test_helpers::push_assistant_text(&mut state.session, "stable");
    let mut controller = NativeSurfaceController::default();
    controller.draw(&mut terminal, &state).expect("first draw");
    let revision = state.session.transcript.revision();

    let outcome = controller.draw(&mut terminal, &state).expect("second draw");

    assert_eq!(
        outcome.history,
        HistoryEmissionOutcome::FastNoop { revision }
    );
}

#[test]
fn native_draw_replays_finalized_history_when_theme_changes() {
    let backend = TestBackend::new(48, 9);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 5, 48, 4));
    let mut state = AppState::new();
    test_helpers::push_assistant_text(&mut state.session, "stable");
    let mut controller = NativeSurfaceController::default();
    controller.draw(&mut terminal, &state).expect("first draw");

    state.ui.theme.accent = ratatui::style::Color::LightRed;
    let outcome = controller
        .draw(&mut terminal, &state)
        .expect("theme replay");

    assert!(matches!(
        outcome.history,
        HistoryEmissionOutcome::Replayed { .. }
    ));
}

#[test]
fn native_draw_appends_after_transcript_revision_changes() {
    let backend = TestBackend::new(48, 12);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 8, 48, 4));
    let mut state = AppState::new();
    test_helpers::push_assistant_text(&mut state.session, "one");
    let mut controller = NativeSurfaceController::default();
    controller.draw(&mut terminal, &state).expect("first draw");

    test_helpers::push_assistant_text(&mut state.session, "two");
    let outcome = controller.draw(&mut terminal, &state).expect("append draw");

    assert!(matches!(
        outcome.history,
        HistoryEmissionOutcome::Appended {
            message_count: 1,
            ..
        }
    ));
    let text = plain_terminal_text(&terminal);
    assert!(text.contains("one"), "{text}");
    assert!(text.contains("two"), "{text}");
}

#[test]
fn native_draw_reappends_stable_stream_after_width_replay() {
    let backend = TestBackend::new(64, 18);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 12, 64, 6));
    let mut state = AppState::new();
    let mut controller = NativeSurfaceController::default();
    controller
        .draw(&mut terminal, &state)
        .expect("initial draw");

    let mut streaming = StreamingState::new();
    streaming.append_text("alpha\n\nbeta");
    streaming.reveal_all();
    state.ui.streaming = Some(streaming);
    controller.draw(&mut terminal, &state).expect("stream draw");

    // A width change (terminal resize) reflows history — immediately if the
    // resized buffer forces it, otherwise after the debounce. Either way the
    // stable stream content must re-emit exactly once (no duplication). A
    // viewport *height* change must not replay at all — exercised separately.
    terminal.set_viewport_area(Rect::new(0, 12, 60, 6));
    let immediate = controller
        .draw(&mut terminal, &state)
        .expect("width change draw");
    let debounced = controller
        .draw_at(
            &mut terminal,
            &state,
            std::time::Instant::now() + std::time::Duration::from_millis(100),
        )
        .expect("width replay");

    assert!(
        matches!(immediate.history, HistoryEmissionOutcome::Replayed { .. })
            || matches!(debounced.history, HistoryEmissionOutcome::Replayed { .. }),
        "a width resize must replay history: {:?} then {:?}",
        immediate.history,
        debounced.history,
    );
    let text = plain_terminal_text(&terminal);
    assert_eq!(text.matches("alpha").count(), 1, "{text}");
    assert_eq!(text.matches("beta").count(), 1, "{text}");
}

#[test]
fn native_draw_replays_history_when_source_prefix_diverges() {
    let backend = TestBackend::new(48, 8);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 5, 48, 3));
    let mut state = AppState::new();
    test_helpers::push_assistant_text(&mut state.session, "one");
    let mut controller = NativeSurfaceController::default();
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
    let mut controller = NativeSurfaceController::default();
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
fn native_draw_bakes_reasoning_metadata_without_full_replay() {
    // A per-turn reasoning-metadata attach must NOT trigger `replay_all_capped`.
    // The engine emits `MessageAppended` + `ReasoningMetadataAttached` back to
    // back at turn finalize and the TUI coalesces them into one draw, so the
    // duration/tokens are in the side-cache when the assistant cell first
    // commits — they bake into its single append-only emit (mirrors how
    // claude-code-kim bakes the metadata footer into the finalized element).
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
    // Metadata in the side-cache before the cell's first (and only) commit.
    state.session.insert_reasoning_metadata(
        uuid,
        crate::state::session::ReasoningMetadata {
            duration_ms: None,
            reasoning_tokens: 22,
        },
    );

    let mut controller = NativeSurfaceController::default();
    let outcome = controller
        .draw(&mut terminal, &state)
        .expect("finalize draw");

    // Append-only emit, never a full replay — that per-turn rewrite is the cost
    // this change removes.
    assert!(
        !matches!(outcome.history, HistoryEmissionOutcome::Replayed { .. }),
        "reasoning metadata must not force a full history replay: {:?}",
        outcome.history
    );
    let text = plain_buffer_lines(terminal.backend().buffer()).join("\n");
    assert!(
        text.contains("22 reasoning tok"),
        "metadata baked in: {text}"
    );
}

#[test]
fn native_draw_replays_after_resize_requested_during_stream_finishes() {
    let backend = TestBackend::new(48, 8);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 5, 20, 3));
    let mut state = AppState::new();
    state.ui.streaming = Some(StreamingState::new());
    let mut controller = NativeSurfaceController::default();

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
    let mut controller = NativeSurfaceController::default();

    controller
        .draw(&mut terminal, &state)
        .expect("initial draw");

    test_helpers::push_user_text(&mut state.session, "u1", "hello");
    let mut streaming = StreamingState::new();
    streaming.append_text("short reply");
    streaming.reveal_all();
    state.ui.streaming = Some(streaming);
    // Resize the terminal *width* mid-stream: that requests a stream-finish
    // replay (reflow is width-keyed; a height-only change would not, and would
    // not exercise the finish-replay gap path this test guards).
    apply_native_viewport(&mut terminal, Rect::new(0, 18, 60, 12));
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
fn native_draw_does_not_replay_on_viewport_height_change() {
    // History re-wrap depends only on width. A viewport *height* change (the
    // live tail growing/shrinking during streaming) must NOT schedule a
    // full-history replay — that per-frame replay was the streaming flicker.
    let backend = TestBackend::new(48, 12);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 7, 48, 3));
    let mut state = AppState::new();
    test_helpers::push_assistant_text(&mut state.session, "height replay");
    let mut controller = NativeSurfaceController::default();
    controller
        .draw(&mut terminal, &state)
        .expect("initial draw");

    // Height-only change (width stays 48).
    terminal.set_viewport_area(Rect::new(0, 7, 48, 4));
    let height_change = controller
        .draw(&mut terminal, &state)
        .expect("height change draw");
    assert!(matches!(
        height_change.history,
        HistoryEmissionOutcome::FastNoop { .. }
    ));

    // Even past the reflow debounce window, no replay is scheduled.
    let outcome = controller
        .draw_at(
            &mut terminal,
            &state,
            std::time::Instant::now() + std::time::Duration::from_millis(100),
        )
        .expect("post-debounce draw");
    assert!(
        !matches!(outcome.history, HistoryEmissionOutcome::Replayed { .. }),
        "viewport height change must not trigger a history replay: {:?}",
        outcome.history
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
    let mut controller = NativeSurfaceController::default();

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
    let mut controller = NativeSurfaceController::default();
    let plan = SurfaceFramePlan {
        modal_placement: None,
        history_surface: HistorySurfaceMode::Viewport,
        attention_requested: false,
    };

    let outcome = controller
        .draw_with_plan(&mut terminal, &state, plan, None)
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
