# Viewport Seating Regression Fixes

Status: technical-decision record and fix plan for the owned-top viewport-seating
refactor (`fix(tui): seat viewport flush after shrinking replays via owned top`,
commit `4be8942ca9`). Scopes a confirmed HIGH regression plus supporting
hardening. Constraints inherited from `terminal-surface-design.md` and
`rendering-hardening-and-rollback.md` remain in force.

Document map: this sits under `terminal-surface-design.md` (surface constraints)
and `native-scrollback-architecture.md` (backend / history emission). It records
decisions for the geometry seating path in
`coco-rs/app/tui/src/terminal.rs` and the paint engine in
`coco-rs/tui-ui/src/engine/terminal.rs`.

## 1. Background: the refactor under review

The refactor reworked inline-viewport geometry to remove the `/clear` blank-gap
class of bug (a stale-anchor / second-writer race). It made four changes:

1. **Owned-top anchoring.** `sync_surface_area` now derives the desired viewport
   position from the *owned* viewport top (`self.terminal.viewport_area().top()`,
   `terminal.rs:600`) instead of `history_bottom_y()`. Rationale: `history_bottom_y`
   mutates mid-frame (clear → insert → reveal), and the sync pass runs *before*
   the history emission, so anchoring on it re-derived a stale position and raced
   the emission. The emission is treated as the single writer that seats the
   viewport; sync preserves the previous settled seat.
2. **Pin de-stick.** `native_viewport_geometry_with_max` (`terminal.rs:769`) made
   the bottom-pin a pure function of `anchor_y >= bottom_pinned_y`. The previous
   sticky term (`pin == BottomPinned || …`) was removed.
3. **Flowing-seat invariant.** A `debug_assert` in `draw_native_frame`
   (`terminal.rs:437`) asserts that a `Flowing` viewport seats flush
   (`viewport_top == history_bottom_y`), via the `flowing_viewport_seats_flush`
   predicate (`terminal.rs:753`).
4. **Replay clamp removal.** `history_driver::replay_rows` (`history_driver.rs:424`)
   deleted the `restored_replay_viewport` reseat; `clear_owned_scrollback` +
   `insert_history_rows` are expected to seat the viewport flush on their own.

The refactor achieves its stated goal: in settled frames `owned-top ==
history_bottom_y`, and the `/clear` gap is structurally eliminated. The pure
geometry math is unit-testable and is the refactor's main asset. The fixes below
do not revert it; they close one real regression it introduced and harden the
seam.

## 2. Findings summary

| ID | Severity | Status | Area |
|----|----------|--------|------|
| C1 | **HIGH** | confirmed | de-stick un-pins an overflow-backed viewport on height shrink → lost history + blank rows |
| A4 | Medium | confirmed | `sync_surface_area` reads `previous_viewport` before leaving alt-screen → spurious shrink on the alt-leave frame |
| OBS | Low | confirmed | flowing-seat invariant guarded only by `debug_assert`; release builds have no signal (was A2/R4/C2) |
| DEAD | Low | confirmed | `reseat_viewport_to_history_row` is dead `pub` engine API after the clamp deletion (was A6) |
| TEST | Low | confirmed | no frame-level test exercises `sync_surface_area` / `draw_native_frame`; C1 and A4 live in untested code (was A9) |
| N1 | Nit | confirmed | `history_bottom_y_before` is a diagnostics-only param threaded through fn + struct (was C4/A7/R2) |
| N2 | Nit | confirmed | `commit_native_viewport_geometry` takes 7 positional args (2 `Rect` + 4 `u16` adjacent) — transposition hazard (was R1) |
| N3 | Nit | confirmed | alt-screen frames feed a stale `main_screen_viewport_pin` to `commit_native_viewport_geometry`; safety is an unasserted geometric coincidence (was C5/R3) |

Refuted during adversarial verification (recorded so they are not re-raised):
**C3** (`shrink_deferred_rows` is diagnostics-only, not cross-frame carry; the
real carry is engine `move_viewport_down_for_history`), **A3** (alt-enter
`set_viewport_area`'s `.min(area.top())` already zeroes `history_bottom_y`, so the
assert evaluates `(Flowing, 0, 0) = true` and cannot false-panic), **A8** (the
`Flowing → BottomPinned` flip is gated such that the new seat is `<=
history_bottom_y`, so `unbacked_gap_rows == 0`; height change cannot mis-pin
upward).

## 3. C1 — overflow-backed shrink un-pins the viewport (HIGH, blocker)

### 3.1 Mechanism (verified against source)

The pin de-stick and the deferred-shrink machinery are **mutually
contradictory**, and the engine erases the signal needed to tell them apart.

The paint engine clamps `history_bottom_y` to the viewport top once history
overflows the screen. In `insert_history_rows` the overflow path
(`scroll_region_up`) runs and the function ends with:

```rust
// coco-rs/tui-ui/src/engine/terminal.rs:618
self.history_bottom_y = viewport_top;
```

`set_viewport_area` (`engine/terminal.rs:293-294`) and `note_history_rows_inserted`
(`:470-474`) apply the same `.min(viewport_area.top())` clamp. There is **no
separate record of the true history extent** once it overflows: after overflow,
`history_bottom_y == viewport_top`, identical to a short flowing history that
seats flush. The de-stick predicate consumes exactly this clamped value
(through `anchor_y`), so it cannot distinguish:

- *bottom-pinned over tall (overflowing) history* — must stay pinned and reveal
  history as the viewport shrinks; from
- *flowing flush with short history* — must seat flush, no reveal.

Failing trace (screen height 24, long session that fills the screen):

1. A permission / `AskUserQuestion` prompt raises `desired_height` to 10
   (`interactive_viewport_max_height` grows the cap while a prompt is active,
   `terminal.rs:727`). The viewport is bottom-pinned: `top=14, height=10,
   bottom=24`. History fills rows `0..14` and overflows into scrollback.
   `history_bottom_y` is clamped to `14`.
2. The prompt closes; `desired_height` drops to `4`.
   `native_viewport_geometry_with_max(anchor_y=14, screen=24, desired_height=4)`:
   `bottom_pinned_y = 24 - 4 = 20`; `pin = (14 >= 20)` → **`Flowing`**;
   `y = anchor_y = 14`; `area = (0,14,4)`, `bottom = 18`.
3. `commit_native_viewport_geometry` gates the deferred-shrink reveal on
   `pin == NativeViewportPin::BottomPinned` (`terminal.rs:670`). Pin is now
   `Flowing`, so **the entire deferred-shrink block is skipped**:
   `reveal_tail_rows = 0`, `committed = desired = (0,14,4)`.
4. `apply_viewport_area((0,14,4))`: `area.y == previous.y` so no history scroll;
   `clear_after_position(14)` clears rows `14..23`; the viewport lands at
   `14..17`. Rows `18..23` are **blank**, and the 6 history rows that were
   scrolled into scrollback when the prompt expanded are **not revealed**.

The input box jumps from the screen bottom to mid-screen, leaves a blank band
below it, and hides history — the same observable signature class as the
original `/clear` gap, and it persists until the next full redraw (a width
change / resize), because a height-only change does not trigger a replay
(`history_reflow` is keyed on width).

### 3.2 Why the deferred-shrink machinery cannot save it

For `bottom_pinned_shrink` to fire, `pin` must be `BottomPinned` **and**
`desired.top() > previous.top()`. On a genuine pinned shrink the new
`bottom_pinned_y` is strictly larger than the old one, and `anchor_y` equals the
old `bottom_pinned_y` (the previous pinned top), so
`anchor_y < new bottom_pinned_y` → the predicate flips to `Flowing` **before**
`commit_native_viewport_geometry` ever sees a `BottomPinned` pin. The
deferred-shrink reveal is therefore effectively dead code under the current
de-stick. This also explains why production logs show `shrink_requested_rows ==
shrink_committed_rows == 0` on every frame: the machinery is unreachable, not
idle. The analyzed sessions never reached a `BottomPinned` frame, so C1 is a
latent defect the logs did not exercise.

### 3.3 Decision and fix

The de-stick's *principle* is correct — "revert to Flowing once finalized
history can no longer back the pinned row" is the right rule. The bug is that the
predicate is fed a **clamped proxy** (`history_bottom_y`/owned-top), which loses
the overflow information the rule depends on. The fix is to feed the predicate an
**overflow-aware history-extent signal**, not to reintroduce blanket stickiness.

**Chosen approach (Option A): make the pin decision consume "does finalized
history still back the pinned row".**

1. The engine tracks finalized history extent independent of the viewport clamp.
   Add a monotonic counter updated where history is inserted / cleared /
   truncated, and expose a predicate:

   ```rust
   // coco-rs/tui-ui/src/engine/terminal.rs
   /// True while finalized history still reaches `row` (i.e. there are at least
   /// `row` finalized rows above the viewport, including rows scrolled into
   /// native scrollback). Unlike `history_bottom_y`, this is NOT clamped to the
   /// viewport top, so it survives overflow.
   pub fn history_backs_row(&self, row: u16) -> bool { /* counter >= row */ }
   ```

   The counter is reset by `clear_owned_scrollback` and decremented on history
   truncation so it can never outlive its rows (mirrors the `history_bottom_y`
   lifecycle, just un-clamped).

2. The geometry function takes the signal and keeps the viewport pinned while
   history backs the pinned row:

   ```rust
   // terminal.rs::native_viewport_geometry_with_max
   let pin = if anchor_y >= bottom_pinned_y || history_backs_pinned_row {
       NativeViewportPin::BottomPinned
   } else {
       NativeViewportPin::Flowing
   };
   ```

   `history_backs_pinned_row = history_backs_row(bottom_pinned_y)` is computed by
   the caller (`sync_surface_area`) from the engine and threaded in. When history
   has genuinely shrunk below the row (`/clear`, rewind, reflow), the predicate is
   false and de-stick still reverts to `Flowing` — the `/clear` fix is preserved.

3. With the pin correctly `BottomPinned` on an overflow-backed shrink,
   `commit_native_viewport_geometry`'s deferred-shrink reveal becomes reachable
   again and fills the freed rows from the history tail
   (`reveal_tail_rows` / `append_fill_rows`), keeping `bottom == screen.height`.

**Rejected — Option B: drop the per-frame de-stick and re-derive pin only on
replay (from the freshly rendered history).** Simpler, but it re-couples pin
correctness to replay timing (the exact coupling the owned-top model removed) and
loses per-frame responsiveness for non-replay height changes. Keep the per-frame
predicate; fix its input.

**Invariant codified:** the pin predicate must never consume a viewport-clamped
quantity as a proxy for finalized history extent. Any pin/seat input that the
engine clamps on overflow must be replaced by an un-clamped, overflow-aware
signal before it drives a geometry decision.

## 4. A4 — stale `previous_viewport` on the alt-screen-leave frame (Medium)

`sync_surface_area` captures the seating baseline at the top of the function:

```rust
// terminal.rs:560
let previous_viewport = self.terminal.viewport_area();
let history_bottom_y_before = self.terminal.history_bottom_y();
```

When the frame leaves alt-screen, `leave_modal_alt_screen` (`terminal.rs:573-575`)
restores the saved inline viewport via `set_viewport_area(saved)`, but
`previous_viewport` still holds the discarded alt full-screen rect (`top=0,
bottom == terminal_height`). If `saved` was bottom-pinned (the common inline
composer case), `commit_native_viewport_geometry` recomputes `BottomPinned` and
the four `bottom_pinned_shrink` conditions are all satisfied against the stale
baseline (`previous.bottom() == h`, `desired.bottom() == h`, `desired.top() > 0 ==
previous.top()`). With `backed_rows == 0` (an ordinary popup-close frame, and
alt-leave does not force a replay), `committed_viewport` is overwritten to a
full-screen top-anchored rect instead of the correct restored bottom seat. The
`debug_assert` does not catch it (BottomPinned is exempt). It is a single-frame
transient that self-heals on the next redraw.

**Fix:** read the seating baseline *after* the alt-screen transition block, so it
reflects the restored real seat:

```rust
// after the wants_alt / leave_modal_alt_screen block
let previous_viewport = self.terminal.viewport_area();
let history_bottom_y_before = self.terminal.history_bottom_y();
```

Then `desired.top() > previous.top()` is false on the restore frame and the
spurious shrink cannot trigger. (This pairs with N1: once the read moves, log the
value directly from `sync_surface_area` rather than threading it through `commit`.)

## 5. OBS + DEAD — observability for the flowing-seat invariant

The only runtime check on the load-bearing flowing-seat invariant is the
`debug_assert` (`terminal.rs:437`), which compiles out of release builds.
`unbacked_gap_rows` is computed in all builds (`terminal.rs:431`) but only emitted
via `debug!` on `target: "tui::surface::geometry"`; the default filter
(`coco=debug,info`) routes non-`coco` targets through the global `info` fallback
and suppresses it. So a release-build violation neither panics nor logs.

To be precise: the `debug_assert` is *verification*, not the gap-prevention
*mechanism* — the structural guarantee lives in `native_viewport_geometry_with_max`
+ owned-top anchoring + the `history_bottom_y` cap. Deleting the assert would not
reintroduce a gap; the gap is the loss of *detection* for future regressions
(exactly the C1 class).

**Fix (reuses the dead `reseat_viewport_to_history_row` API, closing DEAD):**

1. Keep the `debug_assert` — tests fail loudly.
2. Add a release-active guard in `draw_native_frame`: on a flowing-seat
   violation, `tracing::warn!(target: "tui::surface::geometry", …)` with the same
   fields. `warn` clears the default `info` fallback, so it surfaces without a
   custom filter.
3. Self-heal: reseat the viewport to `history_bottom_y_after` via the engine's
   `reseat_viewport_to_history_row` (`engine/terminal.rs:429`), which currently
   has zero production callers. This gives the dead `pub` API a real caller and
   converts a silent visual regression into a one-frame self-correcting event
   plus a telemetry signal. The `flowing_viewport_seats_flush` predicate is shared
   by the assert and the warn path at zero cost.

If the self-heal is judged too aggressive for an initial landing, ship steps 1-2
(assert + warn) and delete `reseat_viewport_to_history_row` instead — but do not
leave it as dead seam API.

## 6. TEST — frame-level coverage gap

Migrating the deleted `replay_all`/`replay_lines` mirror onto the real
`replay_rows` path was a genuine improvement (it now exercises the width-mismatch
`ReplayRequired` early-return that had no prior coverage). But the new
load-bearing pieces — the `debug_assert`, owned-top anchoring, alt-screen pin
handling, and the `sync → commit → fill → emission` ordering — are only tested at
the pure-helper layer. `terminal.test.rs` never calls `sync_surface_area` /
`draw_native_frame`; `testing.rs` drives the lower-level
`NativeSurfaceController::draw`, which does not run sync, hold the pin, or
evaluate the assert. C1 and A4 both live in this untested region.

**Required frame-level tests (drive `Tui::draw` / `draw_native_frame` over a
`TestBackend`):**

- **C1 (must-add):** bottom-pinned viewport over overflowing history → shrink
  `desired_height` → assert the committed viewport stays bottom-pinned
  (`bottom == screen.height`), the freed rows reveal history, and no blank band
  remains. This test fails on current code.
- **A4:** enter then leave alt-screen over a bottom-pinned inline viewport →
  assert the restore frame keeps the saved seat and does not full-screen-overwrite
  / fire the assert.
- `/clear` over tall (overflowing) history → assert the viewport seats flush
  (`viewport_top == history_bottom_y`), no gap — protects the original fix.
- live-height growth on short history → assert no mis-pin (guards the A8 boundary).

## 7. Nits

- **N1** — remove `history_bottom_y_before` from
  `commit_native_viewport_geometry`'s signature and `NativeViewportGeometryCommit`;
  it is consumed only by one `tracing::debug!`. Log it directly inside
  `sync_surface_area`, where the pre-emission value is available. Do **not**
  recompute `history_bottom_y()` at the log site — by then draw/fill have mutated
  it and it is no longer the "before" value.
- **N2** — replace `commit_native_viewport_geometry`'s 7 positional parameters
  (2 adjacent `Rect`, 4 adjacent `u16`) with a named `NativeViewportCommitInputs`
  value struct to remove the transposition hazard the compiler cannot catch.
- **N3** — the alt-screen branch feeds a stale `main_screen_viewport_pin` to
  `commit_native_viewport_geometry`; it is safe today only because alt geometry has
  `desired.top() == 0`, making `desired.top() > previous.top()` always false.
  Make this safe by construction: the alt-screen branch should not call
  `commit_native_viewport_geometry` at all (alt geometry has no shrink semantics).

## 8. Architecture notes

- **"Single writer" is imprecise.** `sync_surface_area` writes the backend via
  `apply_viewport_area` (`scroll_region_up` / `clear_after_position`), so it is a
  writer too. The accurate statement is "the emission is the single *seat-mover*":
  on a Flowing emission frame, sync only changes height and never moves the seat
  (the scroll branch needs `area.y < previous.y`, impossible when `y ==
  previous.top()`). Update the comment to "single seat-mover". Do **not** add a
  `!history_will_emit` gate — it would suppress the legitimate height resize the
  live tail needs.
- **Owned-top trades structural correctness for temporal + clamped-ledger
  correctness.** C1 is the cost: a clamped ledger value (`history_bottom_y`) erases
  the overflow information a downstream pure predicate depends on. General rule
  (codified in §3.3): when a pin/seat decision depends on an input the engine
  clamps on overflow, thread the un-clamped signal into the decision function.
- **`debug_assert` is the wrong sole guard for a user-visible, load-bearing
  invariant.** Shipping builds need active recovery or a `warn` (see §5).

## 9. Sequencing

| Priority | ID | Action |
|----------|-----|--------|
| **P0** | C1 | Thread an overflow-aware `history_backs_row` signal into `native_viewport_geometry_with_max`; restore the deferred-shrink reveal on overflow-backed shrink. Add the C1 regression test. |
| **P1** | A4 | Move the `previous_viewport` / `history_bottom_y_before` read after the alt-screen transition. |
| **P1** | OBS + DEAD | Add the release `warn!` + self-heal via `reseat_viewport_to_history_row` (or delete the dead API). |
| **P2** | TEST | Add the alt-enter, `/clear`-over-tall-history, and live-growth frame-level tests. |
| **P3** | N1 / N2 / N3 | Remove the diagnostics-only param; introduce the inputs struct; stop calling `commit` on alt frames. |

C1 is the only ship blocker. The rest are hardening and hygiene and may follow.

## 10. Invariants this work establishes

- **I-V1** A `Flowing` viewport seats flush on finalized history
  (`viewport_top == history_bottom_y`). Verified by `debug_assert` in debug and a
  `warn` + self-heal in release.
- **I-V2** The pin predicate is computed from an overflow-aware history-extent
  signal, never from a viewport-clamped quantity.
- **I-V3** The seating baseline (`previous_viewport`) is read after all
  alt-screen transitions for the frame, so `commit_native_viewport_geometry`
  always compares against the real current seat.
- **I-V4** Every geometry/seating change is covered by a frame-level test that
  drives the full `sync → commit → emission` path over a `TestBackend`, not only
  the pure helpers.
