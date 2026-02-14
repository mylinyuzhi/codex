//! Throttle management for system reminder generators.
//!
//! This module provides rate limiting for generators to prevent
//! excessive reminder injection.

use std::collections::HashMap;
use std::sync::RwLock;

use crate::types::AttachmentType;

/// Throttle configuration for a generator.
#[derive(Debug, Clone, Copy)]
pub struct ThrottleConfig {
    /// Minimum turns between generating this reminder.
    pub min_turns_between: i32,
    /// Minimum turns after a triggering event before generating.
    pub min_turns_after_trigger: i32,
    /// Maximum times to generate per session (None = unlimited).
    pub max_per_session: Option<i32>,
    /// Full content every N-th generation, sparse otherwise.
    /// `None` = always full content (default).
    /// `Some(n)` = full content on 1st generation and every n-th thereafter.
    pub full_content_every_n: Option<i32>,
}

impl Default for ThrottleConfig {
    fn default() -> Self {
        Self {
            min_turns_between: 0,
            min_turns_after_trigger: 0,
            max_per_session: None,
            full_content_every_n: None,
        }
    }
}

impl ThrottleConfig {
    /// No throttling - generate every turn.
    pub fn none() -> Self {
        Self::default()
    }

    /// Standard throttle for plan mode reminders.
    pub fn plan_mode() -> Self {
        Self {
            min_turns_between: 5,
            full_content_every_n: Some(5),
            ..Default::default()
        }
    }

    /// Standard throttle for plan tool reminders.
    pub fn plan_tool_reminder() -> Self {
        Self {
            min_turns_between: 3,
            min_turns_after_trigger: 5,
            ..Default::default()
        }
    }

    /// Standard throttle for todo reminders.
    pub fn todo_reminder() -> Self {
        Self {
            min_turns_between: 5,
            ..Default::default()
        }
    }

    /// Standard throttle for output style reinforcement.
    /// The output style is in the system prompt, so we only reinforce periodically.
    pub fn output_style() -> Self {
        Self {
            min_turns_between: 15,
            ..Default::default()
        }
    }

    /// Standard throttle for security guidelines.
    /// Full content every 5th generation, sparse otherwise.
    pub fn security_guidelines() -> Self {
        Self {
            full_content_every_n: Some(5),
            ..Default::default()
        }
    }
}

/// State tracking for a single attachment type.
#[derive(Debug, Clone, Default)]
pub struct ThrottleState {
    /// Turn number when this was last generated.
    pub last_generated_turn: Option<i32>,
    /// Number of times generated this session.
    pub session_count: i32,
    /// Turn number when the trigger event occurred.
    pub trigger_turn: Option<i32>,
}

/// Manager for tracking throttle state across generators.
#[derive(Debug, Default)]
pub struct ThrottleManager {
    state: RwLock<HashMap<AttachmentType, ThrottleState>>,
}

impl ThrottleManager {
    /// Create a new throttle manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if a generator should be allowed to run.
    ///
    /// # Arguments
    ///
    /// * `attachment_type` - The type of attachment being generated
    /// * `config` - The throttle configuration for this generator
    /// * `current_turn` - The current turn number
    /// * `trigger_turn` - Optional turn when a trigger event occurred
    pub fn should_generate(
        &self,
        attachment_type: AttachmentType,
        config: &ThrottleConfig,
        current_turn: i32,
    ) -> bool {
        let state = self.state.read().expect("lock poisoned");
        let entry = state.get(&attachment_type);

        match entry {
            None => true, // Never generated, allow
            Some(s) => {
                // Check min_turns_after_trigger
                if let Some(trigger) = s.trigger_turn {
                    if current_turn - trigger < config.min_turns_after_trigger {
                        return false;
                    }
                }

                // Check min_turns_between
                if let Some(last) = s.last_generated_turn {
                    if current_turn - last < config.min_turns_between {
                        return false;
                    }
                }

                // Check max_per_session
                if let Some(max) = config.max_per_session {
                    if s.session_count >= max {
                        return false;
                    }
                }

                true
            }
        }
    }

    /// Mark that a generator successfully generated output.
    pub fn mark_generated(&self, attachment_type: AttachmentType, turn: i32) {
        let mut state = self.state.write().expect("lock poisoned");
        let entry = state.entry(attachment_type).or_default();
        entry.last_generated_turn = Some(turn);
        entry.session_count += 1;
    }

    /// Set the trigger turn for an attachment type.
    pub fn set_trigger_turn(&self, attachment_type: AttachmentType, turn: i32) {
        let mut state = self.state.write().expect("lock poisoned");
        let entry = state.entry(attachment_type).or_default();
        entry.trigger_turn = Some(turn);
    }

    /// Clear the trigger turn for an attachment type.
    pub fn clear_trigger_turn(&self, attachment_type: AttachmentType) {
        let mut state = self.state.write().expect("lock poisoned");
        if let Some(entry) = state.get_mut(&attachment_type) {
            entry.trigger_turn = None;
        }
    }

    /// Check if a generator should produce full (vs sparse) content this generation.
    /// Based on session_count: full on 1st generation and every n-th thereafter.
    pub fn should_use_full_content(
        &self,
        attachment_type: AttachmentType,
        config: &ThrottleConfig,
    ) -> bool {
        match config.full_content_every_n {
            None => true,
            Some(n) => {
                let state = self.state.read().expect("lock poisoned");
                let count = state
                    .get(&attachment_type)
                    .map(|s| s.session_count)
                    .unwrap_or(0);
                // Full on first generation (count 0) and every n-th
                count == 0 || (count % n == 0)
            }
        }
    }

    /// Reset all throttle state (e.g., at session start).
    pub fn reset(&self) {
        let mut state = self.state.write().expect("lock poisoned");
        state.clear();
    }

    /// Get the current state for an attachment type.
    pub fn get_state(&self, attachment_type: AttachmentType) -> Option<ThrottleState> {
        let state = self.state.read().expect("lock poisoned");
        state.get(&attachment_type).cloned()
    }
}

#[cfg(test)]
#[path = "throttle.test.rs"]
mod tests;
