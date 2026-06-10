//! Engine-owned viewport seating (tui-v2 §6.3).
//!
//! The seat/pin decision is computed INSIDE the engine: [`seat_viewport`]
//! anchors on the owned viewport top (the previous settled seat — history
//! emission is the single seat-mover) and consumes the engine-internal,
//! unclamped finalized-history extent for the pin predicate. The extent never
//! leaves the engine, so a viewport-clamped proxy can never be substituted
//! for it (I-V2, the C1 lesson). The shell supplies per-frame *intent*
//! ([`SeatInputs`]: screen, desired height, policy bounds, shrink backing)
//! and applies the returned [`SeatDecision`]; overlay policy (which surface
//! wants alt-screen) stays in the shell — alt frames cover the screen and do
//! not seat.
//!
//! Invariants owned here:
//! - **I-V1** — a `Flowing` viewport seats flush on finalized history
//!   (`viewport_top == history_bottom_y`); see
//!   [`SurfaceTerminal::flowing_seats_flush`].
//! - **I-V2** — the pin predicate consumes the overflow-aware history extent
//!   (`history_backs_row`), never a viewport-clamped quantity.

use ratatui::layout::Rect;
use ratatui::layout::Size;

use crate::engine::terminal::SurfaceBackend;
use crate::engine::terminal::SurfaceTerminal;

/// Whether the inline viewport is glued to the bottom of the screen
/// (history has reached or overflowed it) or flows directly under the
/// finalized history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewportPin {
    Flowing,
    BottomPinned,
}

/// Per-frame seating intent supplied by the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeatInputs {
    /// Current screen size.
    pub screen: Size,
    /// Content-derived viewport height the shell wants this frame.
    pub desired_height: u16,
    /// Shell policy bounds for the viewport height clamp.
    pub min_height: u16,
    pub max_height: u16,
    /// Rows the shell can repaint from its history tail cache when a
    /// bottom-pinned shrink frees rows above the new viewport top.
    pub tail_reveal_rows: u16,
    /// Rows this frame's history emission is guaranteed to append.
    pub guaranteed_append_rows: u16,
}

/// The committed seat for one frame, plus the shrink/reveal arbitration that
/// produced it. `committed_viewport` is what the shell applies; the row
/// counters drive the tail-gap fill and feed geometry diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeatDecision {
    pub pin: ViewportPin,
    pub previous_viewport: Rect,
    pub desired_viewport: Rect,
    pub committed_viewport: Rect,
    pub shrink_requested_rows: u16,
    pub shrink_committed_rows: u16,
    pub reveal_tail_rows: u16,
    pub append_fill_rows: u16,
    pub shrink_deferred_rows: u16,
}

impl SeatDecision {
    /// A decision that commits `desired_viewport` verbatim with no shrink
    /// arbitration — full-screen overlay frames and other paths where no
    /// seating question exists.
    pub fn without_shrink(
        previous_viewport: Rect,
        desired_viewport: Rect,
        pin: ViewportPin,
    ) -> Self {
        Self {
            pin,
            previous_viewport,
            desired_viewport,
            committed_viewport: desired_viewport,
            shrink_requested_rows: 0,
            shrink_committed_rows: 0,
            reveal_tail_rows: 0,
            append_fill_rows: 0,
            shrink_deferred_rows: 0,
        }
    }
}

impl<B> SurfaceTerminal<B>
where
    B: SurfaceBackend,
{
    /// Decide this frame's inline viewport seat. Pure — no terminal writes.
    ///
    /// Anchors on the OWNED viewport top, not `history_bottom_y()`:
    /// `history_bottom_y` mutates mid-frame (clear/insert/reveal) and the
    /// sync pass runs BEFORE the history emission, so the owned top is the
    /// previous frame's settled seat and the emission stays the single
    /// seat-mover. The pin predicate reads the unclamped finalized-history
    /// extent (`history_backs_row`) directly off the engine (I-V2).
    pub fn seat_viewport(&self, inputs: SeatInputs) -> SeatDecision {
        let previous_viewport = self.viewport_area();
        let height = seat_viewport_height(
            inputs.screen,
            inputs.desired_height,
            inputs.min_height,
            inputs.max_height,
        );
        let bottom_pinned_y = inputs.screen.height.saturating_sub(height);
        let (desired_viewport, pin) = seat_geometry(
            previous_viewport.top(),
            inputs.screen,
            height,
            self.history_backs_row(bottom_pinned_y),
        );
        commit_seat(
            pin,
            previous_viewport,
            desired_viewport,
            inputs.screen.height,
            inputs.tail_reveal_rows,
            inputs.guaranteed_append_rows,
        )
    }

    /// The I-V1 flowing-seat invariant evaluated against current terminal
    /// state: a `Flowing` viewport must sit flush on finalized history
    /// (`viewport_top == history_bottom_y`). `BottomPinned` viewports are
    /// exempt — they may carry a transient backed gap pending tail-reveal.
    pub fn flowing_seats_flush(&self, pin: ViewportPin) -> bool {
        flowing_viewport_seats_flush(pin, self.viewport_area().top(), self.history_bottom_y())
    }
}

/// The flowing-seat invariant as a pure predicate. A Flowing gap is the
/// `/clear`-class stale-anchor / second-writer regression; the pin guard is
/// load-bearing — do NOT drop it.
pub fn flowing_viewport_seats_flush(
    pin: ViewportPin,
    viewport_top: u16,
    history_bottom_y: u16,
) -> bool {
    pin != ViewportPin::Flowing || viewport_top == history_bottom_y
}

/// Clamp the shell's desired viewport height to its policy bounds and the
/// screen.
pub fn seat_viewport_height(
    screen: Size,
    desired_height: u16,
    min_height: u16,
    max_height: u16,
) -> u16 {
    if screen.height == 0 {
        return 0;
    }
    desired_height
        .clamp(min_height, max_height.max(min_height))
        .min(screen.height)
}

/// Convenience for shells and test harnesses: the seat rect for a bare
/// anchor with no overflow backing (the pin follows from the anchor position
/// alone). Production seating goes through [`SurfaceTerminal::seat_viewport`],
/// which reads the engine's unclamped history extent.
pub fn seat_viewport_area(
    anchor_y: u16,
    screen: Size,
    desired_height: u16,
    min_height: u16,
    max_height: u16,
) -> Rect {
    let height = seat_viewport_height(screen, desired_height, min_height, max_height);
    seat_geometry(
        anchor_y, screen, height, /*history_backs_pinned_row*/ false,
    )
    .0
}

/// Compute the desired viewport rect and pin for a frame.
///
/// The bottom-pin state is a pure function of whether finalized history still
/// reaches the bottom-pinned row: either the anchor itself sits at/below it,
/// or the unclamped history extent backs it (`history_backs_pinned_row` — the
/// overflow case where `history_bottom_y` has been clamped to the viewport
/// top). It is intentionally NOT sticky — a latched pin that outlived its
/// history is exactly what strands an unbacked gap when history shrinks
/// (`/clear`, reflow, display-toggle, rewind).
fn seat_geometry(
    anchor_y: u16,
    screen: Size,
    height: u16,
    history_backs_pinned_row: bool,
) -> (Rect, ViewportPin) {
    if screen.height == 0 {
        return (Rect::new(0, 0, screen.width, 0), ViewportPin::Flowing);
    }
    let bottom_pinned_y = screen.height.saturating_sub(height);
    let pin = if anchor_y >= bottom_pinned_y || history_backs_pinned_row {
        ViewportPin::BottomPinned
    } else {
        ViewportPin::Flowing
    };
    let y = match pin {
        ViewportPin::Flowing => anchor_y,
        ViewportPin::BottomPinned => bottom_pinned_y,
    };
    (Rect::new(0, y, screen.width, height), pin)
}

/// Arbitrate a bottom-pinned shrink: the freed rows may only be committed to
/// the extent the shell can back them (tail-cache reveal + this frame's
/// guaranteed append); the rest of the shrink defers so the viewport never
/// jumps off the screen bottom over rows nothing will repaint.
fn commit_seat(
    pin: ViewportPin,
    previous_viewport: Rect,
    desired_viewport: Rect,
    terminal_height: u16,
    tail_reveal_rows: u16,
    guaranteed_append_rows: u16,
) -> SeatDecision {
    let mut committed_viewport = desired_viewport;
    let mut shrink_requested_rows = 0;
    let mut shrink_committed_rows = 0;
    let mut reveal_tail_rows = 0;
    let mut append_fill_rows = 0;

    let bottom_pinned_shrink = pin == ViewportPin::BottomPinned
        && previous_viewport.bottom() == terminal_height
        && desired_viewport.bottom() == terminal_height
        && desired_viewport.top() > previous_viewport.top();

    if bottom_pinned_shrink {
        shrink_requested_rows = desired_viewport.top() - previous_viewport.top();
        let backed_rows = tail_reveal_rows.saturating_add(guaranteed_append_rows);
        shrink_committed_rows = shrink_requested_rows.min(backed_rows);
        if shrink_committed_rows < shrink_requested_rows {
            committed_viewport.y = previous_viewport
                .top()
                .saturating_add(shrink_committed_rows);
            committed_viewport.height = terminal_height.saturating_sub(committed_viewport.y);
        }
        reveal_tail_rows = tail_reveal_rows.min(shrink_committed_rows);
        append_fill_rows = shrink_committed_rows.saturating_sub(reveal_tail_rows);
    }

    SeatDecision {
        pin,
        previous_viewport,
        desired_viewport,
        committed_viewport,
        shrink_requested_rows,
        shrink_committed_rows,
        reveal_tail_rows,
        append_fill_rows,
        shrink_deferred_rows: shrink_requested_rows.saturating_sub(shrink_committed_rows),
    }
}

#[cfg(test)]
#[path = "seat.test.rs"]
mod tests;
