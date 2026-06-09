use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use coco_messages::AssistantContent;
use coco_messages::TextContent;
use coco_messages::create_assistant_message;
use coco_tui_ui::engine::compatibility::TerminalCompatibility;
use coco_tui_ui::engine::history_insert::render_history_rows;
use coco_types::TokenUsage;
use crossterm::Command as _;
use pretty_assertions::assert_eq;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::layout::Size;
use ratatui::text::Line;

use crate::state::ModalState;
use crate::streaming::render_controller::StreamRenderKey;
use crate::surface::history_driver::PreparedFinalizedHistory;
use crate::surface::modal::HistorySurfaceMode;
use crate::surface::stream::CommittedStablePrefix;
use crate::surface::stream::PreparedProvisionalAppend;
use coco_tui_ui::engine::terminal::SurfaceTerminal;

use super::*;

#[test]
fn native_viewport_flows_after_history_before_screen_fills() {
    assert_eq!(
        native_viewport_area(
            /*anchor_y*/ 3,
            Size::new(80, 24),
            /*desired_height*/ 6
        ),
        Rect::new(0, 3, 80, 6)
    );
}

#[test]
fn native_viewport_bottom_pins_once_history_reaches_terminal_bottom() {
    assert_eq!(
        native_viewport_area(
            /*anchor_y*/ 22,
            Size::new(80, 24),
            /*desired_height*/ 6
        ),
        Rect::new(0, 18, 80, 6)
    );
}

#[test]
fn native_viewport_pin_state_keeps_later_height_changes_bottom_pinned() {
    let size = Size::new(80, 24);
    // Tall history (anchor_y at/above the bottom-pinned row) pins the viewport,
    // and a later height grow stays pinned because history still backs the row.
    let first = native_viewport_geometry_with_max(
        /*anchor_y*/ 22,
        size,
        /*desired_height*/ 4,
        NATIVE_VIEWPORT_MAX_HEIGHT,
        /*history_backs_pinned_row*/ false,
    );
    let grown = native_viewport_geometry_with_max(
        /*anchor_y*/ 20,
        size,
        /*desired_height*/ 10,
        NATIVE_VIEWPORT_MAX_HEIGHT,
        /*history_backs_pinned_row*/ false,
    );

    assert_eq!(first.pin, NativeViewportPin::BottomPinned);
    assert_eq!(first.area.bottom(), 24);
    assert_eq!(grown.area.bottom(), 24);
    assert_eq!(grown.pin, NativeViewportPin::BottomPinned);
    assert!(
        grown.area.top() < first.area.top(),
        "after pinning, larger live surfaces grow upward from the terminal bottom"
    );
}

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
fn native_viewport_clamps_to_small_terminal_height() {
    assert_eq!(
        native_viewport_area(
            /*anchor_y*/ 10,
            Size::new(80, 3),
            /*desired_height*/ 12
        ),
        Rect::new(0, 0, 80, 3)
    );
}

#[test]
fn native_viewport_handles_zero_height() {
    assert_eq!(
        native_viewport_area(
            /*anchor_y*/ 10,
            Size::new(80, 0),
            /*desired_height*/ 12
        ),
        Rect::new(0, 0, 80, 0)
    );
}

#[test]
fn native_viewport_uses_minimum_height_for_idle_composer() {
    assert_eq!(
        native_viewport_area(
            /*anchor_y*/ 2,
            Size::new(80, 24),
            /*desired_height*/ 1
        ),
        Rect::new(0, 2, 80, 4)
    );
}

#[test]
fn native_viewport_reverts_to_flowing_when_history_below_pinned_row() {
    // History shrank below the bottom-pinned row: the pin is not sticky, so the
    // viewport reverts to flowing and seats flush against history at anchor_y
    // instead of stranding an unbacked gap above a latched bottom position.
    let geometry = native_viewport_geometry_with_max(
        /*anchor_y*/ 8,
        Size::new(80, 40),
        /*desired_height*/ 4,
        NATIVE_VIEWPORT_MAX_HEIGHT,
        /*history_backs_pinned_row*/ false,
    );
    assert_eq!(geometry.pin, NativeViewportPin::Flowing);
    assert_eq!(geometry.area, Rect::new(0, 8, 80, 4));
}

#[test]
fn native_viewport_stays_bottom_pinned_when_history_backs_pinned_row() {
    let geometry = native_viewport_geometry_with_max(
        /*anchor_y*/ 14,
        Size::new(80, 24),
        /*desired_height*/ 4,
        NATIVE_VIEWPORT_MAX_HEIGHT,
        /*history_backs_pinned_row*/ true,
    );

    assert_eq!(geometry.pin, NativeViewportPin::BottomPinned);
    assert_eq!(geometry.area, Rect::new(0, 20, 80, 4));
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
    let mut pin = NativeViewportPin::BottomPinned;
    let commit = draw_native_frame_for_test(
        &mut terminal,
        &mut surface,
        &mut pin,
        &state,
        plan,
        size,
        native_frame,
    )
    .expect("shrink frame");

    assert_eq!(pin, NativeViewportPin::BottomPinned);
    assert_eq!(terminal.viewport_area(), Rect::new(0, 20, width, 4));
    assert_eq!(terminal.history_bottom_y(), 20);
    assert_eq!(commit.shrink_deferred_rows, 0);
    assert!(
        commit.reveal_tail_rows >= 6,
        "shrink should reveal the freed rows from the tail cache: {commit:?}"
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
    let mut pin = NativeViewportPin::BottomPinned;

    let commit = sync_main_surface_area(
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

    assert_eq!(commit.previous_viewport, Rect::new(0, 20, width, 4));
    assert_eq!(commit.committed_viewport, Rect::new(0, 20, width, 4));
    assert_eq!(commit.shrink_requested_rows, 0);
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
    let commit = tui
        .last_geometry_commit_for_test()
        .expect("restore frame geometry commit");

    assert_eq!(commit.previous_viewport, Rect::new(0, 20, width, 4));
    assert_eq!(commit.committed_viewport, Rect::new(0, 20, width, 4));
    assert_eq!(commit.shrink_requested_rows, 0);
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
    let mut pin = NativeViewportPin::BottomPinned;

    let commit = sync_main_surface_area(
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

    assert_eq!(pin, NativeViewportPin::BottomPinned);
    assert_eq!(commit.desired_viewport, Rect::new(0, 20, width, 4));
    assert_eq!(commit.committed_viewport.bottom(), height);
    assert!(commit.committed_viewport.top() < commit.desired_viewport.top());
    assert_eq!(terminal.viewport_area(), commit.committed_viewport);
    assert!(commit.shrink_deferred_rows > 0);
}

#[test]
fn bottom_pinned_shrink_ignores_provisional_append_rows() {
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
        provisional_history: Some(provisional_append_rows(width, 6)),
        history_tail_reveal_rows: 0,
    };
    let mut pin = NativeViewportPin::BottomPinned;

    let commit = sync_main_surface_area(
        &mut terminal,
        &mut pin,
        &state,
        plan,
        size,
        None,
        &native_frame,
    )
    .expect("sync");

    assert_eq!(pin, NativeViewportPin::BottomPinned);
    assert_eq!(native_frame.guaranteed_append_rows(), 0);
    assert_eq!(commit.desired_viewport, Rect::new(0, 20, width, 4));
    assert_eq!(commit.shrink_requested_rows, 6);
    assert_eq!(commit.shrink_committed_rows, 0);
    assert_eq!(commit.shrink_deferred_rows, 6);
    assert_eq!(commit.committed_viewport, Rect::new(0, 14, width, 10));
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
    let mut pin = NativeViewportPin::Flowing;

    let commit = sync_main_surface_area(
        &mut terminal,
        &mut pin,
        &state,
        plan,
        size,
        Some(10),
        &native_frame,
    )
    .expect("sync");

    assert_eq!(pin, NativeViewportPin::Flowing);
    assert_eq!(commit.committed_viewport.top(), 2);
    assert!(commit.committed_viewport.height > 4);
    assert_eq!(terminal.viewport_area(), commit.committed_viewport);
}

#[test]
fn flowing_viewport_seat_invariant_guards_off_bottom_pinned() {
    // The draw_native_frame debug_assert uses this predicate. A Flowing viewport
    // with a gap is the /clear-class regression and MUST be flagged; a Flowing
    // viewport seated flush is fine; a BottomPinned viewport with a transient
    // backed gap is exempt — the pin guard is load-bearing.
    assert!(!flowing_viewport_seats_flush(
        NativeViewportPin::Flowing,
        18,
        4
    ));
    assert!(flowing_viewport_seats_flush(
        NativeViewportPin::Flowing,
        4,
        4
    ));
    assert!(flowing_viewport_seats_flush(
        NativeViewportPin::BottomPinned,
        18,
        4
    ));
}

#[test]
fn native_viewport_caps_to_native_max_height() {
    assert_eq!(
        native_viewport_area(
            /*anchor_y*/ 0,
            Size::new(80, 80),
            /*desired_height*/ 80
        )
        .height,
        NATIVE_VIEWPORT_MAX_HEIGHT
    );
}

#[test]
fn bottom_pinned_shrink_commits_only_backed_rows() {
    let commit = commit_native_viewport_geometry(NativeViewportCommitInputs {
        pin: NativeViewportPin::BottomPinned,
        previous_viewport: Rect::new(0, 8, 80, 16),
        desired_viewport: Rect::new(0, 20, 80, 4),
        terminal_height: 24,
        history_tail_reveal_rows: 3,
        guaranteed_append_rows: 4,
    });

    assert_eq!(commit.shrink_requested_rows, 12);
    assert_eq!(commit.shrink_committed_rows, 7);
    assert_eq!(commit.reveal_tail_rows, 3);
    assert_eq!(commit.append_fill_rows, 4);
    assert_eq!(commit.shrink_deferred_rows, 5);
    assert_eq!(commit.committed_viewport, Rect::new(0, 15, 80, 9));
}

#[test]
fn bottom_pinned_shrink_uses_append_rows_after_tail_reveal() {
    let commit = commit_native_viewport_geometry(NativeViewportCommitInputs {
        pin: NativeViewportPin::BottomPinned,
        previous_viewport: Rect::new(0, 12, 80, 12),
        desired_viewport: Rect::new(0, 20, 80, 4),
        terminal_height: 24,
        history_tail_reveal_rows: 2,
        guaranteed_append_rows: 6,
    });

    assert_eq!(commit.shrink_requested_rows, 8);
    assert_eq!(commit.shrink_committed_rows, 8);
    assert_eq!(commit.reveal_tail_rows, 2);
    assert_eq!(commit.append_fill_rows, 6);
    assert_eq!(commit.shrink_deferred_rows, 0);
    assert_eq!(commit.committed_viewport, commit.desired_viewport);
}

#[test]
fn bottom_pinned_shrink_defers_when_history_is_insufficient() {
    let commit = commit_native_viewport_geometry(NativeViewportCommitInputs {
        pin: NativeViewportPin::BottomPinned,
        previous_viewport: Rect::new(0, 10, 80, 14),
        desired_viewport: Rect::new(0, 20, 80, 4),
        terminal_height: 24,
        history_tail_reveal_rows: 0,
        guaranteed_append_rows: 0,
    });

    assert_eq!(commit.shrink_requested_rows, 10);
    assert_eq!(commit.shrink_committed_rows, 0);
    assert_eq!(commit.shrink_deferred_rows, 10);
    assert_eq!(commit.committed_viewport, Rect::new(0, 10, 80, 14));
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

fn provisional_append_rows(width: u16, rows: u16) -> PreparedProvisionalAppend {
    let lines = (0..rows)
        .map(|index| Line::from(format!("provisional row {index}")))
        .collect::<Vec<_>>();
    PreparedProvisionalAppend {
        committed_prefix: CommittedStablePrefix {
            source: "provisional".to_string(),
            line_count: rows as usize,
            render_key: StreamRenderKey::default(),
        },
        line_count: rows as usize,
        rows: render_history_rows(lines, width),
        render_elapsed: Duration::default(),
    }
}
