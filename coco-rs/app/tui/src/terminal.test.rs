//! Frame-level seating tests (I-V4): drive the full `sync → commit → tail
//! fill → history emission → viewport draw` path through the backend-generic
//! harness. The pure seat/pin/shrink math lives in
//! `coco_tui_ui::engine::seat` and is tested there; these tests pin the
//! shell↔engine integration where the C1 and A4 regressions lived.

use std::sync::Arc;
use std::time::Instant;

use coco_messages::AssistantContent;
use coco_messages::TextContent;
use coco_messages::create_assistant_message;
use coco_tui_ui::engine::compatibility::TerminalCompatibility;
use coco_types::TokenUsage;
use crossterm::Command as _;
use pretty_assertions::assert_eq;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::layout::Size;

use crate::state::ModalState;
use crate::surface::history_driver::PreparedFinalizedHistory;
use crate::surface::modal::HistorySurfaceMode;
use coco_tui_ui::engine::terminal::SurfaceTerminal;

use super::*;

#[test]
fn interactive_viewport_max_height_grows_for_active_prompt() {
    use crate::state::PanePromptState;
    use crate::state::PlanEntryPromptState;
    let mut state = crate::state::AppState::new();
    // No prompt: the streaming/idle cap.
    assert_eq!(
        interactive_viewport_max_height(&state, 60),
        NATIVE_VIEWPORT_MAX_HEIGHT
    );
    // Active prompt: grows to nearly the full screen so all options fit.
    state
        .ui
        .push_prompt(PanePromptState::PlanEntry(PlanEntryPromptState {
            description: "x".into(),
        }));
    assert_eq!(
        interactive_viewport_max_height(&state, 60),
        60 - NATIVE_VIEWPORT_MIN_HEIGHT
    );
    // Never below the normal cap; clamped to the screen on tiny terminals.
    assert_eq!(interactive_viewport_max_height(&state, 10), 10);
}

#[test]
fn native_frame_overflow_backed_shrink_stays_bottom_pinned() {
    let width = 40;
    let height = 24;
    let size = Size::new(width, height);
    let plan = native_history_plan();
    let mut state = AppState::new();
    push_history_messages(&mut state, 40);

    let backend = TestBackend::new(width, height);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.sync_screen_size(size);
    terminal.set_viewport_area(Rect::new(0, 14, width, 10));
    let mut surface = NativeSurfaceController::default();

    let initial_frame = surface.prepare_native_frame(&state, width, plan, Instant::now());
    surface
        .draw_with_plan_at_frame(&mut terminal, &state, plan, initial_frame, 0)
        .expect("initial draw");
    assert_eq!(terminal.history_bottom_y(), 14);
    assert!(terminal.history_backs_row(20));

    let native_frame = surface.prepare_native_frame(&state, width, plan, Instant::now());
    let mut pin = ViewportPin::BottomPinned;
    let decision = draw_native_frame_for_test(
        &mut terminal,
        &mut surface,
        &mut pin,
        &state,
        plan,
        size,
        native_frame,
    )
    .expect("shrink frame");

    assert_eq!(pin, ViewportPin::BottomPinned);
    assert_eq!(terminal.viewport_area(), Rect::new(0, 20, width, 4));
    assert_eq!(terminal.history_bottom_y(), 20);
    assert_eq!(decision.shrink_deferred_rows, 0);
    assert!(
        decision.reveal_tail_rows >= 6,
        "shrink should reveal the freed rows from the tail cache: {decision:?}"
    );
    let revealed_rows = (14..20)
        .map(|y| buffer_row(terminal.backend().buffer(), y))
        .collect::<Vec<_>>();
    assert!(
        revealed_rows.iter().any(|row| !row.trim().is_empty()),
        "history reveal area should not be an all-blank band: {revealed_rows:?}"
    );
}

#[test]
fn sync_main_surface_uses_restored_inline_viewport_baseline() {
    let width = 80;
    let height = 24;
    let size = Size::new(width, height);
    let plan = native_history_plan();
    let state = AppState::new();
    let backend = TestBackend::new(width, height);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.sync_screen_size(size);
    terminal.set_viewport_area(Rect::new(0, 20, width, 4));
    terminal.note_history_rows_inserted(20);
    let mut surface = NativeSurfaceController::default();
    let native_frame = surface.prepare_native_frame(&state, width, plan, Instant::now());
    let mut pin = ViewportPin::BottomPinned;

    let decision = sync_main_surface_area(
        &mut terminal,
        &mut pin,
        &state,
        plan,
        size,
        native_frame
            .live_lines
            .as_ref()
            .map(|lines| lines.len() as u16),
        &native_frame,
    )
    .expect("sync");

    assert_eq!(decision.previous_viewport, Rect::new(0, 20, width, 4));
    assert_eq!(decision.committed_viewport, Rect::new(0, 20, width, 4));
    assert_eq!(decision.shrink_requested_rows, 0);
}

#[test]
fn tui_alt_screen_leave_uses_restored_inline_viewport_baseline() {
    let width = 80;
    let height = 24;
    let size = Size::new(width, height);
    let backend = TestBackend::new(width, height);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.sync_screen_size(size);
    terminal.set_viewport_area(Rect::new(0, 20, width, 4));
    terminal.note_history_rows_inserted(20);
    let mut tui = Tui::new_for_test(terminal, TerminalCompatibility::NativeScrollback);

    let mut modal_state = AppState::new();
    modal_state.ui.show_modal(ModalState::Help);
    tui.draw(&modal_state).expect("alt-screen draw");
    assert_eq!(
        tui.terminal().viewport_area(),
        Rect::new(0, 0, width, height)
    );

    let main_state = AppState::new();
    tui.draw(&main_state).expect("restore draw");
    let decision = tui
        .last_geometry_commit_for_test()
        .expect("restore frame geometry commit");

    assert_eq!(decision.previous_viewport, Rect::new(0, 20, width, 4));
    assert_eq!(decision.committed_viewport, Rect::new(0, 20, width, 4));
    assert_eq!(decision.shrink_requested_rows, 0);
    assert_eq!(tui.terminal().viewport_area(), Rect::new(0, 20, width, 4));
}

#[test]
fn overflow_backed_shrink_partially_defers_when_backing_is_insufficient() {
    let width = 80;
    let height = 24;
    let size = Size::new(width, height);
    let plan = native_history_plan();
    let state = AppState::new();
    let backend = TestBackend::new(width, height);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.sync_screen_size(size);
    terminal.set_viewport_area(Rect::new(0, 14, width, 10));
    terminal.note_history_rows_inserted(30);
    let mut surface = NativeSurfaceController::default();
    let native_frame = surface.prepare_native_frame(&state, width, plan, Instant::now());
    let mut pin = ViewportPin::BottomPinned;

    let decision = sync_main_surface_area(
        &mut terminal,
        &mut pin,
        &state,
        plan,
        size,
        native_frame
            .live_lines
            .as_ref()
            .map(|lines| lines.len() as u16),
        &native_frame,
    )
    .expect("sync");

    assert_eq!(pin, ViewportPin::BottomPinned);
    assert_eq!(decision.desired_viewport, Rect::new(0, 20, width, 4));
    assert_eq!(decision.committed_viewport.bottom(), height);
    assert!(decision.committed_viewport.top() < decision.desired_viewport.top());
    assert_eq!(terminal.viewport_area(), decision.committed_viewport);
    assert!(decision.shrink_deferred_rows > 0);
}

#[test]
fn bottom_pinned_shrink_uses_only_finalized_append_rows() {
    let width = 80;
    let height = 24;
    let size = Size::new(width, height);
    let plan = native_history_plan();
    let state = AppState::new();
    let backend = TestBackend::new(width, height);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.sync_screen_size(size);
    terminal.set_viewport_area(Rect::new(0, 14, width, 10));
    terminal.note_history_rows_inserted(20);
    let native_frame = NativeSurfaceFramePlan {
        live_lines: None,
        finalized_history: PreparedFinalizedHistory::FastNoop { revision: 0 },
        stream_append: None,
        stream_render_key_invalidated: false,
        history_tail_reveal_rows: 0,
        prepare_stats: crate::surface::controller::NativePrepareStats::default(),
    };
    let mut pin = ViewportPin::BottomPinned;

    let decision = sync_main_surface_area(
        &mut terminal,
        &mut pin,
        &state,
        plan,
        size,
        None,
        &native_frame,
    )
    .expect("sync");

    assert_eq!(pin, ViewportPin::BottomPinned);
    assert_eq!(native_frame.guaranteed_append_rows(), 0);
    assert_eq!(decision.desired_viewport, Rect::new(0, 20, width, 4));
    assert_eq!(decision.shrink_requested_rows, 6);
    assert_eq!(decision.shrink_committed_rows, 0);
    assert_eq!(decision.shrink_deferred_rows, 6);
    assert_eq!(decision.committed_viewport, Rect::new(0, 14, width, 10));
    assert_eq!(terminal.viewport_area(), Rect::new(0, 14, width, 10));
}

#[test]
fn short_history_growth_stays_flowing() {
    let width = 80;
    let height = 24;
    let size = Size::new(width, height);
    let plan = native_history_plan();
    let state = AppState::new();
    let backend = TestBackend::new(width, height);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.sync_screen_size(size);
    terminal.set_viewport_area(Rect::new(0, 2, width, 4));
    terminal.note_history_rows_inserted(2);
    let mut surface = NativeSurfaceController::default();
    let native_frame = surface.prepare_native_frame(&state, width, plan, Instant::now());
    let mut pin = ViewportPin::Flowing;

    let decision = sync_main_surface_area(
        &mut terminal,
        &mut pin,
        &state,
        plan,
        size,
        Some(10),
        &native_frame,
    )
    .expect("sync");

    assert_eq!(pin, ViewportPin::Flowing);
    assert_eq!(decision.committed_viewport.top(), 2);
    assert!(decision.committed_viewport.height > 4);
    assert_eq!(terminal.viewport_area(), decision.committed_viewport);
}

#[test]
fn alternate_scroll_commands_emit_xterm_private_mode_bytes() {
    let mut enabled = String::new();
    EnableAlternateScroll
        .write_ansi(&mut enabled)
        .expect("write enable bytes");
    assert_eq!(enabled, "\x1b[?1007h");

    let mut disabled = String::new();
    DisableAlternateScroll
        .write_ansi(&mut disabled)
        .expect("write disable bytes");
    assert_eq!(disabled, "\x1b[?1007l");
}

fn native_history_plan() -> SurfaceFramePlan {
    SurfaceFramePlan {
        modal_placement: None,
        history_surface: HistorySurfaceMode::NativeScrollback,
        attention_requested: false,
    }
}

fn push_history_messages(state: &mut AppState, count: usize) {
    for index in 0..count {
        let message = create_assistant_message(
            vec![AssistantContent::Text(TextContent::new(format!(
                "history row {index:02}"
            )))],
            "test-model",
            TokenUsage::default(),
        );
        state
            .session
            .transcript
            .on_message_appended(Arc::new(message));
    }
}

fn buffer_row(buffer: &Buffer, y: u16) -> String {
    let mut row = String::new();
    for x in 0..buffer.area.width {
        row.push_str(buffer[(x, y)].symbol());
    }
    row
}
