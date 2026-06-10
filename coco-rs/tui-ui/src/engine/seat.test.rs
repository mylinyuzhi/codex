use pretty_assertions::assert_eq;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::layout::Size;

use super::*;

// Representative shell policy bounds (the shell owns the real constants).
const MIN_H: u16 = 4;
const MAX_H: u16 = 12;

fn inputs(screen: Size, desired_height: u16) -> SeatInputs {
    SeatInputs {
        screen,
        desired_height,
        min_height: MIN_H,
        max_height: MAX_H,
        tail_reveal_rows: 0,
        guaranteed_append_rows: 0,
    }
}

fn terminal_with_viewport(screen: Size, viewport: Rect) -> SurfaceTerminal<TestBackend> {
    let backend = TestBackend::new(screen.width, screen.height);
    let mut terminal = SurfaceTerminal::new(backend).expect("terminal");
    terminal.sync_screen_size(screen);
    terminal.set_viewport_area(viewport);
    terminal
}

// ─── seat_viewport: the engine-owned decision ───────────────────────────────

#[test]
fn seat_flows_after_history_before_screen_fills() {
    let screen = Size::new(80, 24);
    let terminal = terminal_with_viewport(screen, Rect::new(0, 3, 80, 6));

    let decision = terminal.seat_viewport(inputs(screen, 6));

    assert_eq!(decision.pin, ViewportPin::Flowing);
    assert_eq!(decision.committed_viewport, Rect::new(0, 3, 80, 6));
}

#[test]
fn seat_bottom_pins_once_anchor_reaches_pinned_row() {
    let screen = Size::new(80, 24);
    let terminal = terminal_with_viewport(screen, Rect::new(0, 22, 80, 2));

    let decision = terminal.seat_viewport(inputs(screen, 6));

    assert_eq!(decision.pin, ViewportPin::BottomPinned);
    assert_eq!(decision.committed_viewport, Rect::new(0, 18, 80, 6));
}

#[test]
fn seat_stays_pinned_when_unclamped_extent_backs_pinned_row() {
    // The C1 class at decision level: history overflowed (the engine clamped
    // `history_bottom_y` to the viewport top), a tall prompt closes and the
    // desired height shrinks. The anchor (14) sits above the new pinned row
    // (20), but the UNCLAMPED extent still backs it — the seat must stay
    // bottom-pinned and commit the shrink against the provided backing, not
    // jump the viewport to mid-screen over rows nothing will repaint.
    let screen = Size::new(80, 24);
    let mut terminal = terminal_with_viewport(screen, Rect::new(0, 14, 80, 10));
    terminal.note_history_rows_inserted(20);
    assert!(terminal.history_backs_row(20));

    let decision = terminal.seat_viewport(SeatInputs {
        tail_reveal_rows: 6,
        ..inputs(screen, 4)
    });

    assert_eq!(decision.pin, ViewportPin::BottomPinned);
    assert_eq!(decision.desired_viewport, Rect::new(0, 20, 80, 4));
    assert_eq!(decision.committed_viewport, Rect::new(0, 20, 80, 4));
    assert_eq!(decision.shrink_requested_rows, 6);
    assert_eq!(decision.shrink_committed_rows, 6);
    assert_eq!(decision.reveal_tail_rows, 6);
    assert_eq!(decision.shrink_deferred_rows, 0);
}

#[test]
fn seat_shrink_defers_when_backing_is_insufficient() {
    let screen = Size::new(80, 24);
    let mut terminal = terminal_with_viewport(screen, Rect::new(0, 10, 80, 14));
    terminal.note_history_rows_inserted(20);

    let decision = terminal.seat_viewport(inputs(screen, 4));

    assert_eq!(decision.pin, ViewportPin::BottomPinned);
    assert_eq!(decision.desired_viewport, Rect::new(0, 20, 80, 4));
    assert_eq!(decision.committed_viewport, Rect::new(0, 10, 80, 14));
    assert_eq!(decision.shrink_requested_rows, 10);
    assert_eq!(decision.shrink_committed_rows, 0);
    assert_eq!(decision.shrink_deferred_rows, 10);
}

#[test]
fn seat_reverts_to_flowing_when_history_below_pinned_row() {
    // History shrank below the bottom-pinned row: the pin is not sticky, so
    // the viewport reverts to flowing and seats flush against history instead
    // of stranding an unbacked gap above a latched bottom position.
    let screen = Size::new(80, 40);
    let terminal = terminal_with_viewport(screen, Rect::new(0, 8, 80, 4));

    let decision = terminal.seat_viewport(inputs(screen, 4));

    assert_eq!(decision.pin, ViewportPin::Flowing);
    assert_eq!(decision.committed_viewport, Rect::new(0, 8, 80, 4));
}

// ─── seat_geometry: pin/anchor math ──────────────────────────────────────────

#[test]
fn geometry_keeps_later_height_changes_bottom_pinned() {
    let screen = Size::new(80, 24);
    // Tall history (anchor at/above the pinned row) pins the viewport, and a
    // later height grow stays pinned because the anchor still reaches the row.
    let (first_area, first_pin) =
        seat_geometry(/*anchor_y*/ 22, screen, /*height*/ 4, false);
    let (grown_area, grown_pin) =
        seat_geometry(/*anchor_y*/ 20, screen, /*height*/ 10, false);

    assert_eq!(first_pin, ViewportPin::BottomPinned);
    assert_eq!(first_area.bottom(), 24);
    assert_eq!(grown_pin, ViewportPin::BottomPinned);
    assert_eq!(grown_area.bottom(), 24);
    assert!(
        grown_area.top() < first_area.top(),
        "after pinning, larger live surfaces grow upward from the terminal bottom"
    );
}

#[test]
fn geometry_pins_when_extent_backs_pinned_row_despite_high_anchor() {
    let (area, pin) = seat_geometry(
        /*anchor_y*/ 14,
        Size::new(80, 24),
        /*height*/ 4,
        /*history_backs_pinned_row*/ true,
    );

    assert_eq!(pin, ViewportPin::BottomPinned);
    assert_eq!(area, Rect::new(0, 20, 80, 4));
}

#[test]
fn geometry_handles_zero_height_screen() {
    let (area, pin) = seat_geometry(10, Size::new(80, 0), 0, false);
    assert_eq!(area, Rect::new(0, 0, 80, 0));
    assert_eq!(pin, ViewportPin::Flowing);
}

// ─── seat_viewport_height: policy clamp ─────────────────────────────────────

#[test]
fn height_uses_minimum_for_idle_composer() {
    assert_eq!(
        seat_viewport_height(Size::new(80, 24), 1, MIN_H, MAX_H),
        MIN_H
    );
}

#[test]
fn height_caps_to_max() {
    assert_eq!(
        seat_viewport_height(Size::new(80, 80), 80, MIN_H, MAX_H),
        MAX_H
    );
}

#[test]
fn height_clamps_to_small_terminal() {
    assert_eq!(seat_viewport_height(Size::new(80, 3), 12, MIN_H, MAX_H), 3);
}

#[test]
fn height_zero_screen_is_zero() {
    assert_eq!(seat_viewport_height(Size::new(80, 0), 12, MIN_H, MAX_H), 0);
}

// ─── commit_seat: bottom-pinned shrink arbitration ──────────────────────────

#[test]
fn shrink_commits_only_backed_rows() {
    let decision = commit_seat(
        ViewportPin::BottomPinned,
        Rect::new(0, 8, 80, 16),
        Rect::new(0, 20, 80, 4),
        /*terminal_height*/ 24,
        /*tail_reveal_rows*/ 3,
        /*guaranteed_append_rows*/ 4,
    );

    assert_eq!(decision.shrink_requested_rows, 12);
    assert_eq!(decision.shrink_committed_rows, 7);
    assert_eq!(decision.reveal_tail_rows, 3);
    assert_eq!(decision.append_fill_rows, 4);
    assert_eq!(decision.shrink_deferred_rows, 5);
    assert_eq!(decision.committed_viewport, Rect::new(0, 15, 80, 9));
}

#[test]
fn shrink_uses_append_rows_after_tail_reveal() {
    let decision = commit_seat(
        ViewportPin::BottomPinned,
        Rect::new(0, 12, 80, 12),
        Rect::new(0, 20, 80, 4),
        /*terminal_height*/ 24,
        /*tail_reveal_rows*/ 2,
        /*guaranteed_append_rows*/ 6,
    );

    assert_eq!(decision.shrink_requested_rows, 8);
    assert_eq!(decision.shrink_committed_rows, 8);
    assert_eq!(decision.reveal_tail_rows, 2);
    assert_eq!(decision.append_fill_rows, 6);
    assert_eq!(decision.shrink_deferred_rows, 0);
    assert_eq!(decision.committed_viewport, decision.desired_viewport);
}

#[test]
fn shrink_defers_fully_when_nothing_backs_it() {
    let decision = commit_seat(
        ViewportPin::BottomPinned,
        Rect::new(0, 10, 80, 14),
        Rect::new(0, 20, 80, 4),
        /*terminal_height*/ 24,
        /*tail_reveal_rows*/ 0,
        /*guaranteed_append_rows*/ 0,
    );

    assert_eq!(decision.shrink_requested_rows, 10);
    assert_eq!(decision.shrink_committed_rows, 0);
    assert_eq!(decision.shrink_deferred_rows, 10);
    assert_eq!(decision.committed_viewport, Rect::new(0, 10, 80, 14));
}

// ─── flowing-seat invariant (I-V1) ──────────────────────────────────────────

#[test]
fn flowing_seat_invariant_guards_off_bottom_pinned() {
    // A Flowing viewport with a gap is the /clear-class regression and MUST
    // be flagged; a Flowing viewport seated flush is fine; a BottomPinned
    // viewport with a transient backed gap is exempt — the pin guard is
    // load-bearing.
    assert!(!flowing_viewport_seats_flush(ViewportPin::Flowing, 18, 4));
    assert!(flowing_viewport_seats_flush(ViewportPin::Flowing, 4, 4));
    assert!(flowing_viewport_seats_flush(
        ViewportPin::BottomPinned,
        18,
        4
    ));
}
