//! `ModelRuntime` — owns the model-call policy for a session.
//!
//! Holds an ordered chain `slots[0] = primary, slots[1..] = fallbacks`
//! and tracks which index is currently active. The engine consults
//! `current_client()` per turn, calls `advance()` after a streak of
//! capacity errors, and optionally issues half-open probes back to
//! primary via `attempt_probe_if_due` + `finalize_probe`.
//!
//! ## I13 (cache-state reset on provider switch)
//!
//! Provider-specific prompt-cache breakpoints are not transferable
//! across providers. When `QueryParams` construction eventually
//! consults a `CacheBreakDetector`, per-slot cache state MUST reset
//! on every active-slot transition. `on_switch_i13` is the hook
//! where that reset belongs; it is a no-op today because no
//! production caller reads per-slot cache state yet.
//!
//! ## State machine
//!
//! - `advance_at(now)`: forward hop. Slot 0→1→…→N. Preserves
//!   `RecoveryState.next_backoff` and `attempts` across the hop so
//!   the probe backoff ramp is monotonic across an entire session,
//!   not reset per tier. Only `switched_at` restarts.
//! - `attempt_probe_if_due(now)`: if policy configured, active != 0,
//!   backoff elapsed, attempts not exhausted → swap to slot 0 and
//!   mark the probe in-flight. `finalize_probe(outcome, now)` MUST
//!   follow. Skip otherwise.
//! - `finalize_probe(Success, now)`: clear recovery; stay on primary.
//! - `finalize_probe(Failure, now)`: revert to captured fallback;
//!   double backoff; increment attempts.
//!
//! Probe-in-flight state is stored on `RecoveryState` itself so the
//! engine can't forget to finalize. `current_client()` is safe to
//! call at any point and always returns the active slot.
//!
//! ## Scope
//!
//! Deliberately concrete — no trait. Per the project rule "no trait
//! without users", tests inject `Arc<ApiClient>` directly; there is
//! no runtime polymorphism need.

use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use coco_config::FallbackRecoveryPolicy;
use coco_inference::ApiClient;
use serde::Deserialize;
use serde::Serialize;

/// Outcome of calling [`ModelRuntime::advance`] after a capacity-error
/// streak in the active slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AdvanceOutcome {
    /// Successfully moved to the next slot. Carries the new active
    /// model id so the caller can emit a fallback notice.
    Switched(String),
    /// All slots in the chain have been tried — the session must
    /// surface a terminal error.
    Exhausted,
}

/// Typed telemetry reason for a fallback / recovery event. Keeps the
/// wire contract closed and reviewable (vs a free-form string).
///
/// Serializes as `snake_case` strings to remain compatible with
/// downstream `ModelFallbackParams.reason: String` consumers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ModelFallbackReason {
    /// Forward hop triggered by N consecutive capacity errors in
    /// the active slot.
    CapacityDegrade { consecutive_errors: i32 },
    /// Backward hop — half-open probe reached the primary.
    ProbeRecovery,
    /// All slots in the chain have been tried and failed. Terminal
    /// reason emitted before bubbling the error to the user.
    ChainExhausted,
}

/// Recovery bookkeeping; only populated when a [`FallbackRecoveryPolicy`]
/// is attached and the runtime is off primary.
#[derive(Debug, Clone, Copy)]
struct RecoveryState {
    /// When the runtime most recently transitioned off primary OR
    /// completed a probe attempt. Seeds the backoff window.
    switched_at: Instant,
    /// Wait time before the next probe attempt. Starts at
    /// `policy.initial_backoff` and doubles on each probe failure
    /// up to `policy.max_backoff`. PRESERVED across forward
    /// `advance_at` hops so a monotonically growing backoff is not
    /// reset by chain degradation.
    next_backoff: Duration,
    /// Total probe attempts this session. Capped by
    /// `policy.max_attempts`.
    attempts: i32,
    /// When `Some(slot)`, a probe is currently in-flight. `active`
    /// has been pre-swapped to 0; `slot` is the pre-probe fallback
    /// index. `finalize_probe` clears this. Owning the probe state
    /// here (instead of in the engine) makes it impossible to lose
    /// track of a probe by forgetting to call `finalize_probe` —
    /// `current_client()` still reflects the right slot, and any
    /// `attempt_probe_if_due` call while `probing.is_some()` is a
    /// no-op (asserts below).
    probing: Option<usize>,
}

/// Outcome of a probe decision at turn entry.
///
/// `Probe` carries no payload — the slot to revert to on failure is
/// stored internally in `RecoveryState`. Engines must pair `Probe`
/// with exactly one `finalize_probe(outcome, now)` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProbeDecision {
    Skip,
    Probe,
}

/// Outcome the engine reports back via [`ModelRuntime::finalize_probe`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProbeOutcome {
    /// Probe call succeeded — primary is healthy; clear recovery
    /// state and stay on slot 0.
    Success,
    /// Probe call failed. Revert to the captured pre-probe slot;
    /// double backoff; increment attempts.
    Failure,
}

/// Concrete model runtime — no trait surface.
///
/// `slots[0]` is always the primary. `slots.len()` is `1 + fallbacks.len()`;
/// `active` indexes into `slots`. Invariant: `active < slots.len()`.
pub(crate) struct ModelRuntime {
    slots: Vec<Arc<ApiClient>>,
    active: usize,
    recovery_policy: Option<FallbackRecoveryPolicy>,
    recovery: Option<RecoveryState>,
}

impl ModelRuntime {
    /// Build a runtime. `fallbacks` may be empty (primary-only).
    /// This is the single constructor — prior
    /// `primary_only` / `with_fallback` convenience shims existed
    /// as `#[cfg(test)]` overlap and were removed; tests can wrap
    /// this call in a helper if needed.
    pub(crate) fn new(primary: Arc<ApiClient>, fallbacks: Vec<Arc<ApiClient>>) -> Self {
        let mut slots = Vec::with_capacity(1 + fallbacks.len());
        slots.push(primary);
        slots.extend(fallbacks);
        Self {
            slots,
            active: 0,
            recovery_policy: None,
            recovery: None,
        }
    }

    /// Attach a half-open recovery policy. When set, the runtime
    /// periodically probes the primary slot after a fallback
    /// switch; see [`Self::attempt_probe_if_due`] + [`Self::finalize_probe`].
    /// Sticky-fallback sessions leave this unset.
    pub(crate) fn with_recovery_policy(mut self, policy: FallbackRecoveryPolicy) -> Self {
        self.recovery_policy = Some(policy);
        self
    }

    /// Clone handle to the currently-active client.
    pub(crate) fn current_client(&self) -> Arc<ApiClient> {
        self.slots[self.active].clone()
    }

    /// Model id reported to tool context + telemetry.
    pub(crate) fn current_model_name(&self) -> &str {
        self.slots[self.active].model_id()
    }

    /// True when the runtime has at least one fallback slot (chain
    /// length > 1). Engine gates fallback-trigger logic on this.
    pub(crate) fn has_fallback(&self) -> bool {
        self.slots.len() > 1
    }

    /// Step forward in the chain. On success the active slot moves
    /// from N to N+1; `switched_at` refreshes but `next_backoff` and
    /// `attempts` carry forward so the probe ramp is monotonic
    /// across the session. On the last slot returns
    /// [`AdvanceOutcome::Exhausted`] without changing state.
    pub(crate) fn advance(&mut self) -> AdvanceOutcome {
        self.advance_at(Instant::now())
    }

    /// Time-injected variant of [`Self::advance`] for tests.
    pub(crate) fn advance_at(&mut self, now: Instant) -> AdvanceOutcome {
        // A forward hop while a probe is in-flight means the
        // probe's underlying turn hit a capacity error on primary.
        // Finalize the probe as a failure first so bookkeeping
        // stays consistent; the revert moves `active` to the
        // captured pre-probe slot, and THEN we check whether a
        // further advance is possible. (Without this pre-step the
        // bounds check would compare the probe's `active=0` to
        // slot count and wrongly permit advancing past the chain
        // end.)
        if self.recovery.and_then(|r| r.probing).is_some() {
            self.finalize_probe(ProbeOutcome::Failure, now);
        }
        if self.active + 1 >= self.slots.len() {
            return AdvanceOutcome::Exhausted;
        }
        self.active += 1;
        self.on_switch_i13(now);
        self.update_recovery_on_forward_advance(now);
        AdvanceOutcome::Switched(self.current_model_name().to_string())
    }

    /// Decide whether the upcoming turn should probe primary.
    ///
    /// Returns [`ProbeDecision::Probe`] when:
    /// - a recovery policy is configured, AND
    /// - the runtime is currently on a non-primary slot, AND
    /// - no probe is already in-flight, AND
    /// - the current backoff window has elapsed, AND
    /// - `attempts < policy.max_attempts`.
    ///
    /// **State mutation:** when returning `Probe`, the runtime
    /// stashes the pre-probe active slot internally and swaps
    /// `active = 0`. Engine must pair with exactly one
    /// [`Self::finalize_probe`] call before issuing the next
    /// probe-decision.
    pub(crate) fn attempt_probe_if_due(&mut self, now: Instant) -> ProbeDecision {
        let Some(policy) = self.recovery_policy else {
            return ProbeDecision::Skip;
        };
        if self.active == 0 {
            return ProbeDecision::Skip;
        }
        let Some(mut state) = self.recovery else {
            return ProbeDecision::Skip;
        };
        if state.probing.is_some() {
            // Already probing — caller forgot to finalize.
            // Skipping is the safe behavior: we don't double-swap
            // and don't tick attempts. The `finalize_probe`
            // contract documents this.
            return ProbeDecision::Skip;
        }
        if state.attempts >= policy.max_attempts {
            return ProbeDecision::Skip;
        }
        if now.duration_since(state.switched_at) < state.next_backoff {
            return ProbeDecision::Skip;
        }
        // Stash pre-probe slot and pre-swap to primary. Recovery
        // state must persist across the probe so a failure path
        // keeps `next_backoff` ready for doubling.
        state.probing = Some(self.active);
        self.recovery = Some(state);
        self.active = 0;
        self.on_switch_i13(now);
        ProbeDecision::Probe
    }

    /// Record the outcome of a probe started by
    /// [`Self::attempt_probe_if_due`]. No-op when called without an
    /// in-flight probe (defensive).
    pub(crate) fn finalize_probe(&mut self, outcome: ProbeOutcome, now: Instant) {
        let Some(state) = self.recovery else {
            return;
        };
        let Some(fallback_slot) = state.probing else {
            return;
        };
        match outcome {
            ProbeOutcome::Success => {
                // Primary is healthy — clear recovery entirely.
                self.recovery = None;
                self.on_switch_i13(now);
            }
            ProbeOutcome::Failure => {
                // Revert to the captured fallback tier. `fallback_slot`
                // was captured before the pre-swap so it's
                // always in-range; clamp defensively anyway.
                let target = fallback_slot.min(self.slots.len() - 1).max(1);
                self.active = target;
                // Double the backoff (based on the pre-probe
                // window so the ramp is monotonic), increment
                // attempts, clear probing, refresh switched_at.
                let policy = self.recovery_policy;
                let next_backoff = match policy {
                    Some(p) => doubled_backoff(state.next_backoff, p),
                    None => state.next_backoff,
                };
                self.recovery = Some(RecoveryState {
                    switched_at: now,
                    next_backoff,
                    attempts: state.attempts + 1,
                    probing: None,
                });
                self.on_switch_i13(now);
            }
        }
    }

    /// True when a probe is currently in-flight (active has been
    /// pre-swapped to primary; caller must finalize).
    pub(crate) fn probe_in_flight(&self) -> bool {
        self.recovery.and_then(|r| r.probing).is_some()
    }

    /// Accessor for the recovery-state attempts counter.
    /// `None` = no recovery state (sticky or on primary).
    #[cfg(test)]
    pub(crate) fn recovery_attempts(&self) -> Option<i32> {
        self.recovery.map(|r| r.attempts)
    }

    /// Accessor for the currently-active slot index.
    #[cfg(test)]
    pub(crate) fn active_index(&self) -> usize {
        self.active
    }

    /// Accessor for total slot count (primary + fallbacks).
    #[cfg(test)]
    pub(crate) fn slot_count(&self) -> usize {
        self.slots.len()
    }

    // ─── Internal state management ────────────────────────────

    /// Seed or refresh recovery state after a forward hop. Preserves
    /// `next_backoff` and `attempts` across the transition so the
    /// probe ramp grows monotonically across the entire session.
    fn update_recovery_on_forward_advance(&mut self, now: Instant) {
        let Some(policy) = self.recovery_policy else {
            return;
        };
        let (next_backoff, attempts) = match self.recovery {
            Some(r) => (r.next_backoff, r.attempts),
            None => (policy.initial_backoff(), 0),
        };
        self.recovery = Some(RecoveryState {
            switched_at: now,
            next_backoff,
            attempts,
            probing: None,
        });
    }

    /// I13 cache-state reset seam. Currently a no-op; lands real
    /// behavior when `QueryParams` construction consults a
    /// per-slot `CacheBreakDetector`. Kept as a single method so
    /// every slot transition (forward advance, probe swap, probe
    /// finalize) routes through one enforcement point.
    fn on_switch_i13(&mut self, _now: Instant) {
        // No-op today — no per-slot cache state yet.
    }
}

/// Double the current backoff, clamping to `policy.max_backoff`.
/// Free function so tests can exercise the ramp in isolation.
fn doubled_backoff(current: Duration, policy: FallbackRecoveryPolicy) -> Duration {
    let max = policy.max_backoff();
    let doubled = current.saturating_mul(2);
    if doubled > max { max } else { doubled }
}

#[cfg(test)]
#[path = "model_runtime.test.rs"]
mod tests;
