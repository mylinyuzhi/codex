//! System reminder module.
//!
//! Provides contextual injection of metadata, state information, and instructions
//! into conversations at strategic points. This mechanism:
//! - Provides rich context to the LLM without cluttering user-visible output
//! - Uses XML-tagged messages (`<system-reminder>`, `<system-notification>`, etc.)
//! - Runs parallel generators with timeout protection (1 second max)
//! - Supports throttling to avoid spam
//!
//! Based on Claude Code v2.0.59's attachment system.

pub mod attachments;
pub mod file_tracker;
pub mod generator;
pub mod throttle;
pub mod types;

pub use file_tracker::FileTracker;
pub use generator::{
    AttachmentGenerator, BackgroundTaskInfo, BackgroundTaskStatus, BackgroundTaskType,
    GeneratorContext, TodoItem, TodoState,
};
pub use throttle::{ThrottleConfig, ThrottleManager};
pub use types::{
    AttachmentType, ReminderTier, SystemReminder, XmlTag, SYSTEM_NOTIFICATION_CLOSE_TAG,
    SYSTEM_NOTIFICATION_OPEN_TAG, SYSTEM_REMINDER_CLOSE_TAG, SYSTEM_REMINDER_OPEN_TAG,
};

use crate::config::system_reminder::SystemReminderConfig;
use attachments::{
    BackgroundTaskGenerator, ChangedFilesGenerator, CriticalInstructionGenerator,
    PlanModeGenerator, TodoReminderGenerator,
};
use futures::future::join_all;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;

/// Default timeout for generator execution (1 second).
const DEFAULT_TIMEOUT_MS: i64 = 1000;

/// Telemetry sampling rate (5%).
const TELEMETRY_SAMPLE_RATE: f64 = 0.05;

/// Main system reminder orchestrator.
///
/// Matches JH5() in Claude Code chunks.107.mjs:1813-1829.
pub struct SystemReminderOrchestrator {
    generators: Vec<Arc<dyn AttachmentGenerator>>,
    throttle_manager: ThrottleManager,
    timeout_duration: Duration,
    config: SystemReminderConfig,
}

impl SystemReminderOrchestrator {
    /// Create a new orchestrator with the given configuration.
    pub fn new(config: SystemReminderConfig) -> Self {
        let timeout_ms = config.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);

        let generators: Vec<Arc<dyn AttachmentGenerator>> = vec![
            Arc::new(CriticalInstructionGenerator::new()),
            Arc::new(PlanModeGenerator::new()),
            Arc::new(TodoReminderGenerator::new()),
            Arc::new(ChangedFilesGenerator::new()),
            Arc::new(BackgroundTaskGenerator::new()),
        ];

        Self {
            generators,
            throttle_manager: ThrottleManager::new(),
            timeout_duration: Duration::from_millis(timeout_ms as u64),
            config,
        }
    }

    /// Generate all applicable system reminders for a turn.
    ///
    /// Matches JH5 execution flow in Claude Code.
    pub async fn generate_all(&self, ctx: &GeneratorContext<'_>) -> Vec<SystemReminder> {
        // Step 1: Check global disable
        if !self.config.enabled {
            return Vec::new();
        }

        // Step 2: Build futures for all applicable generators
        let futures: Vec<_> = self
            .generators
            .iter()
            .filter(|g| self.should_run(g.as_ref(), ctx))
            .map(|g| {
                let g = Arc::clone(g);
                let timeout_duration = self.timeout_duration;
                let should_sample = rand::random::<f64>() < TELEMETRY_SAMPLE_RATE;
                let start_time = std::time::Instant::now();

                async move {
                    // Step 3: Execute with timeout (1 second max)
                    let result = match timeout(timeout_duration, g.generate(ctx)).await {
                        Ok(Ok(Some(reminder))) => {
                            tracing::debug!("Generator {} produced reminder", g.name());
                            Some(reminder)
                        }
                        Ok(Ok(None)) => {
                            tracing::trace!("Generator {} returned None", g.name());
                            None
                        }
                        Ok(Err(e)) => {
                            // Graceful degradation
                            tracing::warn!("Generator {} failed: {}", g.name(), e);
                            None
                        }
                        Err(_) => {
                            tracing::warn!("Generator {} timed out", g.name());
                            None
                        }
                    };

                    // Step 4: Record telemetry (5% sample)
                    if should_sample {
                        let duration = start_time.elapsed();
                        tracing::info!(
                            target: "telemetry",
                            generator = g.name(),
                            duration_ms = duration.as_millis() as i64,
                            success = result.is_some(),
                            "attachment_compute_duration"
                        );
                    }

                    result
                }
            })
            .collect();

        // Step 5: Run all generators in parallel
        join_all(futures).await.into_iter().flatten().collect()
    }

    /// Check if a generator should run.
    fn should_run(&self, generator: &dyn AttachmentGenerator, ctx: &GeneratorContext<'_>) -> bool {
        // Check if enabled in config
        if !generator.is_enabled(&self.config) {
            return false;
        }

        // Check tier requirements
        match generator.tier() {
            ReminderTier::Core => true,
            ReminderTier::MainAgentOnly => ctx.is_main_agent,
            ReminderTier::UserPrompt => ctx.has_user_input,
        }
    }

    /// Reset orchestrator state (call at session start).
    pub fn reset(&self) {
        self.throttle_manager.reset();
    }

    /// Get reference to the throttle manager.
    pub fn throttle_manager(&self) -> &ThrottleManager {
        &self.throttle_manager
    }

    /// Get reference to the config.
    pub fn config(&self) -> &SystemReminderConfig {
        &self.config
    }
}

impl Default for SystemReminderOrchestrator {
    fn default() -> Self {
        Self::new(SystemReminderConfig::default())
    }
}

impl std::fmt::Debug for SystemReminderOrchestrator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemReminderOrchestrator")
            .field("generator_count", &self.generators.len())
            .field("timeout_ms", &self.timeout_duration.as_millis())
            .field("enabled", &self.config.enabled)
            .finish()
    }
}

// ============================================
// Tests
// ============================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::system_reminder::AttachmentSettings;
    use std::path::Path;

    fn make_context(
        is_main_agent: bool,
        is_plan_mode: bool,
    ) -> (FileTracker, TodoState, Vec<BackgroundTaskInfo>) {
        (FileTracker::new(), TodoState::default(), vec![])
    }

    #[tokio::test]
    async fn test_orchestrator_disabled() {
        let config = SystemReminderConfig {
            enabled: false,
            ..Default::default()
        };
        let orchestrator = SystemReminderOrchestrator::new(config);

        let (tracker, todo_state, bg_tasks) = make_context(true, false);
        let ctx = GeneratorContext {
            turn_number: 1,
            is_main_agent: true,
            has_user_input: true,
            cwd: Path::new("/test"),
            agent_id: "test",
            file_tracker: &tracker,
            is_plan_mode: false,
            plan_file_path: None,
            is_plan_reentry: false,
            todo_state: &todo_state,
            background_tasks: &bg_tasks,
            critical_instruction: Some("test instruction"),
        };

        let reminders = orchestrator.generate_all(&ctx).await;
        assert!(reminders.is_empty());
    }

    #[tokio::test]
    async fn test_orchestrator_generates_critical_instruction() {
        let config = SystemReminderConfig::default();
        let orchestrator = SystemReminderOrchestrator::new(config);

        let (tracker, todo_state, bg_tasks) = make_context(true, false);
        let ctx = GeneratorContext {
            turn_number: 1,
            is_main_agent: true,
            has_user_input: true,
            cwd: Path::new("/test"),
            agent_id: "test",
            file_tracker: &tracker,
            is_plan_mode: false,
            plan_file_path: None,
            is_plan_reentry: false,
            todo_state: &todo_state,
            background_tasks: &bg_tasks,
            critical_instruction: Some("Always run tests"),
        };

        let reminders = orchestrator.generate_all(&ctx).await;

        // Should have at least the critical instruction
        assert!(reminders
            .iter()
            .any(|r| r.attachment_type == AttachmentType::CriticalInstruction));
    }

    #[tokio::test]
    async fn test_orchestrator_generates_plan_mode() {
        let config = SystemReminderConfig::default();
        let orchestrator = SystemReminderOrchestrator::new(config);

        let (tracker, todo_state, bg_tasks) = make_context(true, true);
        let ctx = GeneratorContext {
            turn_number: 1,
            is_main_agent: true,
            has_user_input: true,
            cwd: Path::new("/test"),
            agent_id: "test",
            file_tracker: &tracker,
            is_plan_mode: true,
            plan_file_path: Some("/path/to/plan.md"),
            is_plan_reentry: false,
            todo_state: &todo_state,
            background_tasks: &bg_tasks,
            critical_instruction: None,
        };

        let reminders = orchestrator.generate_all(&ctx).await;

        // Should have plan mode reminder
        assert!(reminders
            .iter()
            .any(|r| r.attachment_type == AttachmentType::PlanMode));
    }

    #[tokio::test]
    async fn test_orchestrator_respects_attachment_settings() {
        let config = SystemReminderConfig {
            enabled: true,
            attachments: AttachmentSettings {
                critical_instruction: false,
                plan_mode: true,
                todo_reminder: false,
                changed_files: false,
                background_task: false,
            },
            ..Default::default()
        };
        let orchestrator = SystemReminderOrchestrator::new(config);

        let (tracker, todo_state, bg_tasks) = make_context(true, false);
        let ctx = GeneratorContext {
            turn_number: 1,
            is_main_agent: true,
            has_user_input: true,
            cwd: Path::new("/test"),
            agent_id: "test",
            file_tracker: &tracker,
            is_plan_mode: false,
            plan_file_path: None,
            is_plan_reentry: false,
            todo_state: &todo_state,
            background_tasks: &bg_tasks,
            critical_instruction: Some("test"),
        };

        let reminders = orchestrator.generate_all(&ctx).await;

        // Critical instruction should NOT be generated
        assert!(!reminders
            .iter()
            .any(|r| r.attachment_type == AttachmentType::CriticalInstruction));
    }

    #[test]
    fn test_orchestrator_reset() {
        let orchestrator = SystemReminderOrchestrator::default();
        orchestrator
            .throttle_manager()
            .mark_generated(AttachmentType::TodoReminder, 1);
        orchestrator.reset();

        // After reset, throttle state should be cleared
        assert!(orchestrator
            .throttle_manager()
            .should_generate(AttachmentType::TodoReminder, 2, None));
    }

    #[test]
    fn test_orchestrator_default() {
        let orchestrator = SystemReminderOrchestrator::default();
        assert!(orchestrator.config.enabled);
        assert_eq!(orchestrator.generators.len(), 5);
    }
}
