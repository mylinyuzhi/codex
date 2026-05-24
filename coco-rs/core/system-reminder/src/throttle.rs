//! Rate-limit state for reminder generators.
//!
//! Constants and semantics are **TS-first**:
//!
//! - [`ThrottleConfig::plan_mode`] / [`ThrottleConfig::auto_mode`] — TS
//!   `PLAN_MODE_ATTACHMENT_CONFIG` / `AUTO_MODE_ATTACHMENT_CONFIG`
//!   (`attachments.ts:259-267`). 5-turn throttle with a full reminder every
//!   5th attachment (so attachments #1, #6, #11, … are Full).
//! - [`ThrottleConfig::todo_reminder`] — TS `TODO_REMINDER_CONFIG` (`attachments.ts:254-257`).
//!   10-turn throttle on reminder cadence. TS additionally gates on
//!   `turnsSinceLastTodoWrite >= 10`; that's an absence check on history and
//!   stays in the generator's `generate()` path (not the throttle manager).
//! - [`ThrottleConfig::verify_plan_reminder`] — TS `VERIFY_PLAN_REMINDER_CONFIG`
//!   (`attachments.ts:291-293`). 10-turn throttle.
//!
//! The manager uses interior mutability (`std::sync::RwLock`) because callers
//! hold it by shared reference inside async tasks. Lock guards are never held
//! across an await.

use std::collections::HashMap;
use std::sync::RwLock;

use crate::types::AttachmentType;

/// Per-generator rate-limit configuration.
#[derive(Debug, Clone, Copy, Default)]
pub struct ThrottleConfig {
    /// Minimum turns between successive generations. `0` = allow every turn.
    pub min_turns_between: i32,

    /// Minimum turns after an external trigger event before generating.
    ///
    /// Example: `plan_tool_reminder` in cocode-rs uses this to skip the
    /// reminder for 5 turns after the agent actually called the plan tool.
    /// Set via [`ThrottleManager::set_trigger_turn`] from outside.
    pub min_turns_after_trigger: i32,

    /// Upper bound on total generations per session. `None` = unlimited.
    pub max_per_session: Option<i32>,

    /// Cadence for the Full vs Sparse content split. `None` = always Full.
    ///
    /// `Some(n)` → Full on the 1st generation and every n-th thereafter
    /// (i.e. `session_count == 0 || session_count % n == 0`). Matches TS
    /// `FULL_REMINDER_EVERY_N_ATTACHMENTS`, which makes attachments
    /// #1, #6, #11, … Full when `n = 5`.
    pub full_content_every_n: Option<i32>,
}

impl ThrottleConfig {
    /// No throttling — generate every turn with full content.
    pub const fn none() -> Self {
        Self {
            min_turns_between: 0,
            min_turns_after_trigger: 0,
            max_per_session: None,
            full_content_every_n: None,
        }
    }

    /// TS `PLAN_MODE_ATTACHMENT_CONFIG`: 5-turn throttle, full every 5th.
    pub const fn plan_mode() -> Self {
        Self {
            min_turns_between: 5,
            min_turns_after_trigger: 0,
            max_per_session: None,
            full_content_every_n: Some(5),
        }
    }

    /// TS `AUTO_MODE_ATTACHMENT_CONFIG`: identical to plan_mode.
    pub const fn auto_mode() -> Self {
        Self::plan_mode()
    }

    /// TS `TODO_REMINDER_CONFIG.TURNS_BETWEEN_REMINDERS`: 10.
    pub const fn todo_reminder() -> Self {
        Self {
            min_turns_between: 10,
            min_turns_after_trigger: 0,
            max_per_session: None,
            full_content_every_n: None,
        }
    }

    /// TS `VERIFY_PLAN_REMINDER_CONFIG.TURNS_BETWEEN_REMINDERS`: 10.
    pub const fn verify_plan_reminder() -> Self {
        Self {
            min_turns_between: 10,
            min_turns_after_trigger: 0,
            max_per_session: None,
            full_content_every_n: None,
        }
    }
}

/// Per-attachment accumulator used by the manager.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThrottleState {
    /// Turn number at which this attachment was most recently generated.
    pub last_generated_turn: Option<i32>,
    /// Number of times generated in the current session.
    pub session_count: i32,
    /// Turn number of the last external trigger event (see `min_turns_after_trigger`).
    pub trigger_turn: Option<i32>,
}

/// Manages throttle state across all generators.
///
/// All operations are synchronous and cheap (single hash-map lookup + scalar
/// compare). A poisoned lock falls back to "allow" to avoid wedging the turn —
/// reminders are soft state and a stale throttle is never worse than an error.
#[derive(Debug, Default)]
pub struct ThrottleManager {
    state: RwLock<HashMap<AttachmentType, ThrottleState>>,
}

impl ThrottleManager {
    /// Create an empty manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Return true if the generator for `attachment_type` should produce output
    /// at `current_turn` given `config`. This is the sole gate the orchestrator
    /// consults before running a generator.
    pub fn should_generate(
        &self,
        attachment_type: AttachmentType,
        config: &ThrottleConfig,
        current_turn: i32,
    ) -> bool {
        let state = self
            .state
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(entry) = state.get(&attachment_type) else {
            // Never generated → allow.
            return true;
        };

        if let Some(trigger) = entry.trigger_turn
            && current_turn - trigger < config.min_turns_after_trigger
        {
            return false;
        }

        if let Some(last) = entry.last_generated_turn
            && current_turn - last < config.min_turns_between
        {
            return false;
        }

        if let Some(max) = config.max_per_session
            && entry.session_count >= max
        {
            return false;
        }

        true
    }

    /// Record a successful generation at `turn`. Bumps `session_count`.
    pub fn mark_generated(&self, attachment_type: AttachmentType, turn: i32) {
        let mut state = self
            .state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = state.entry(attachment_type).or_default();
        entry.last_generated_turn = Some(turn);
        entry.session_count += 1;
    }

    /// Record that an external trigger event happened at `turn` — callers use
    /// this to drive `min_turns_after_trigger`-gated reminders (e.g. "nudge
    /// the user to use TodoWrite again 5 turns after they last used it").
    pub fn set_trigger_turn(&self, attachment_type: AttachmentType, turn: i32) {
        let mut state = self
            .state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = state.entry(attachment_type).or_default();
        entry.trigger_turn = Some(turn);
    }

    /// Clear the trigger timestamp for `attachment_type`.
    pub fn clear_trigger_turn(&self, attachment_type: AttachmentType) {
        let mut state = self
            .state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(entry) = state.get_mut(&attachment_type) {
            entry.trigger_turn = None;
        }
    }

    /// Decide Full vs Sparse for the *next* generation.
    ///
    /// Full when `session_count == 0` or `session_count % n == 0`. Matches TS
    /// `attachmentCount % FULL_REMINDER_EVERY_N_ATTACHMENTS === 1` at
    /// `attachments.ts:1229` — because TS increments count *before* the check,
    /// `count = 1,6,11,…` maps to our `session_count = 0,5,10,…`.
    pub fn should_use_full_content(
        &self,
        attachment_type: AttachmentType,
        config: &ThrottleConfig,
    ) -> bool {
        match config.full_content_every_n {
            None => true,
            Some(n) if n <= 0 => true,
            Some(n) => {
                let state = self
                    .state
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let count = state
                    .get(&attachment_type)
                    .map(|s| s.session_count)
                    .unwrap_or(0);
                count == 0 || count % n == 0
            }
        }
    }

    /// Seed persistent state for `attachment_type`, replacing any existing entry.
    ///
    /// Use case: the engine re-constructs the orchestrator per
    /// `run_session_loop` invocation but the reminder cadence must
    /// survive across runs. Engine reads cadence counters from
    /// `ToolAppState` and calls this method to populate the in-memory
    /// throttle before `generate_all`.
    pub fn seed_state(&self, attachment_type: AttachmentType, state: ThrottleState) {
        let mut map = self
            .state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        map.insert(attachment_type, state);
    }

    /// Clear all state. Call at session start.
    pub fn reset(&self) {
        let mut state = self
            .state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.clear();
    }

    /// Snapshot the state for `attachment_type` (for debugging / tests).
    pub fn get_state(&self, attachment_type: AttachmentType) -> Option<ThrottleState> {
        let state = self
            .state
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.get(&attachment_type).cloned()
    }
}

#[cfg(test)]
#[path = "throttle.test.rs"]
mod tests;
