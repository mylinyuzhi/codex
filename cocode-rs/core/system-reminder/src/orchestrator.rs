//! Orchestrator for parallel generator execution.
//!
//! This module provides the main orchestration logic for running
//! multiple generators in parallel with timeout protection.

use std::sync::Arc;
use std::time::Duration;

use futures::future;
use tokio::time::timeout;
use tracing::debug;
use tracing::warn;

use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::generators::AgentMentionsGenerator;
use crate::generators::AlreadyReadFilesGenerator;
use crate::generators::AtMentionedFilesGenerator;
use crate::generators::AvailableSkillsGenerator;
use crate::generators::BudgetUsdGenerator;
use crate::generators::ChangedFilesGenerator;
use crate::generators::CollabNotificationsGenerator;
use crate::generators::CompactFileReferenceGenerator;
use crate::generators::DelegateModeGenerator;
use crate::generators::LspDiagnosticsGenerator;
use crate::generators::NestedMemoryGenerator;
use crate::generators::OutputStyleGenerator;
use crate::generators::PlanModeApprovedGenerator;
use crate::generators::PlanModeEnterGenerator;
use crate::generators::PlanModeExitGenerator;
use crate::generators::PlanToolReminderGenerator;
use crate::generators::PlanVerificationGenerator;
use crate::generators::QueuedCommandsGenerator;
use crate::generators::SecurityGuidelinesGenerator;
use crate::generators::TodoRemindersGenerator;
use crate::generators::TokenUsageGenerator;
use crate::generators::UnifiedTasksGenerator;
use crate::throttle::ThrottleManager;
use crate::types::ReminderTier;
use crate::types::SystemReminder;

/// Default timeout for generator execution (1 second).
const DEFAULT_TIMEOUT_MS: i64 = 1000;

/// Orchestrator for running system reminder generators.
///
/// The orchestrator manages a collection of generators, running them
/// in parallel with timeout protection and tier-based filtering.
pub struct SystemReminderOrchestrator {
    /// Registered generators.
    generators: Vec<Arc<dyn AttachmentGenerator>>,
    /// Throttle manager for rate limiting.
    throttle_manager: ThrottleManager,
    /// Timeout duration for each generator.
    timeout_duration: Duration,
    /// Configuration.
    config: SystemReminderConfig,
}

impl std::fmt::Debug for SystemReminderOrchestrator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemReminderOrchestrator")
            .field("generator_count", &self.generators.len())
            .field("timeout_ms", &self.timeout_duration.as_millis())
            .finish()
    }
}

impl SystemReminderOrchestrator {
    /// Create a new orchestrator with the given configuration.
    pub fn new(config: SystemReminderConfig) -> Self {
        let timeout_ms = if config.timeout_ms > 0 {
            config.timeout_ms
        } else {
            DEFAULT_TIMEOUT_MS
        };

        let generators = Self::create_default_generators();

        Self {
            generators,
            throttle_manager: ThrottleManager::new(),
            timeout_duration: Duration::from_millis(timeout_ms as u64),
            config,
        }
    }

    /// Create the default set of generators.
    fn create_default_generators() -> Vec<Arc<dyn AttachmentGenerator>> {
        vec![
            // Core tier
            Arc::new(SecurityGuidelinesGenerator),
            Arc::new(ChangedFilesGenerator),
            Arc::new(PlanModeEnterGenerator),
            Arc::new(PlanModeApprovedGenerator),
            Arc::new(PlanModeExitGenerator),
            Arc::new(PlanToolReminderGenerator),
            Arc::new(NestedMemoryGenerator),
            // MainAgentOnly tier
            Arc::new(AvailableSkillsGenerator),
            Arc::new(LspDiagnosticsGenerator),
            Arc::new(OutputStyleGenerator),
            Arc::new(TodoRemindersGenerator),
            Arc::new(UnifiedTasksGenerator),
            Arc::new(DelegateModeGenerator),
            Arc::new(CollabNotificationsGenerator),
            Arc::new(PlanVerificationGenerator),
            Arc::new(TokenUsageGenerator),
            Arc::new(QueuedCommandsGenerator),
            // New generators for enhanced features
            Arc::new(AlreadyReadFilesGenerator),
            Arc::new(BudgetUsdGenerator),
            Arc::new(CompactFileReferenceGenerator),
            // UserPrompt tier
            Arc::new(AtMentionedFilesGenerator),
            Arc::new(AgentMentionsGenerator),
        ]
    }

    /// Add a custom generator.
    pub fn add_generator(&mut self, generator: Arc<dyn AttachmentGenerator>) {
        self.generators.push(generator);
    }

    /// Generate all applicable reminders for the current context.
    ///
    /// Takes `ctx` by value so the orchestrator can pre-compute
    /// per-generator full-content flags before running generators.
    ///
    /// Generators are filtered by:
    /// 1. Global enable flag
    /// 2. Per-generator enable flag
    /// 3. Tier requirements (Core, MainAgentOnly, UserPrompt)
    /// 4. Throttle rules
    ///
    /// All applicable generators run in parallel with timeout protection.
    pub async fn generate_all(&self, mut ctx: GeneratorContext<'_>) -> Vec<SystemReminder> {
        if !self.config.enabled {
            debug!("System reminders disabled globally");
            return Vec::new();
        }

        // Pre-compute full-content flags for generators that have full_content_every_n
        for g in &self.generators {
            let config = g.throttle_config();
            if config.full_content_every_n.is_some() {
                let is_full = self
                    .throttle_manager
                    .should_use_full_content(g.attachment_type(), &config);
                ctx.full_content_flags.insert(g.attachment_type(), is_full);
            }
        }

        // Filter generators that should run
        let applicable_generators: Vec<_> = self
            .generators
            .iter()
            .filter(|g| self.should_run_generator(g.as_ref(), &ctx))
            .cloned()
            .collect();

        if applicable_generators.is_empty() {
            debug!("No applicable generators for this turn");
            return Vec::new();
        }

        debug!(
            "Running {} generators for turn {}",
            applicable_generators.len(),
            ctx.turn_number
        );

        // Run all generators in parallel with timeout
        let ctx_ref = &ctx;
        let futures: Vec<_> = applicable_generators
            .iter()
            .map(|g| {
                let generator = Arc::clone(g);
                let timeout_duration = self.timeout_duration;
                async move {
                    let name = generator.name().to_string();
                    let attachment_type = generator.attachment_type();

                    match timeout(timeout_duration, generator.generate(ctx_ref)).await {
                        Ok(Ok(Some(reminder))) => {
                            debug!("Generator '{}' produced reminder", name);
                            Some((attachment_type, reminder))
                        }
                        Ok(Ok(None)) => None,
                        Ok(Err(e)) => {
                            warn!("Generator '{}' failed: {}", name, e);
                            None
                        }
                        Err(_) => {
                            warn!(
                                "Generator '{}' timed out after {}ms",
                                name,
                                timeout_duration.as_millis()
                            );
                            None
                        }
                    }
                }
            })
            .collect();

        let results = future::join_all(futures).await;

        // Mark successful generations and collect reminders
        let mut reminders = Vec::new();
        for result in results.into_iter().flatten() {
            let (attachment_type, reminder) = result;
            self.throttle_manager
                .mark_generated(attachment_type, ctx.turn_number);
            reminders.push(reminder);
        }

        debug!(
            "Generated {} reminders for turn {}",
            reminders.len(),
            ctx.turn_number
        );

        reminders
    }

    /// Check if a generator should run for the current context.
    fn should_run_generator(
        &self,
        generator: &dyn AttachmentGenerator,
        ctx: &GeneratorContext<'_>,
    ) -> bool {
        // Check if generator is enabled in config
        if !generator.is_enabled(&self.config) {
            return false;
        }

        // Check tier requirements
        let tier = generator.tier();
        match tier {
            ReminderTier::Core => {
                // Always run for all agents
            }
            ReminderTier::MainAgentOnly => {
                if !ctx.is_main_agent {
                    return false;
                }
            }
            ReminderTier::UserPrompt => {
                if !ctx.has_user_input {
                    return false;
                }
            }
        }

        // Check throttle
        let throttle_config = generator.throttle_config();
        if !self.throttle_manager.should_generate(
            generator.attachment_type(),
            &throttle_config,
            ctx.turn_number,
        ) {
            debug!(
                "Generator '{}' throttled at turn {}",
                generator.name(),
                ctx.turn_number
            );
            return false;
        }

        true
    }

    /// Get a reference to the throttle manager.
    pub fn throttle_manager(&self) -> &ThrottleManager {
        &self.throttle_manager
    }

    /// Reset the throttle manager (e.g., at session start).
    pub fn reset_throttle(&self) {
        self.throttle_manager.reset();
    }

    /// Get the number of registered generators.
    pub fn generator_count(&self) -> usize {
        self.generators.len()
    }

    /// Get the timeout duration.
    pub fn timeout_duration(&self) -> Duration {
        self.timeout_duration
    }

    /// Get the configuration.
    pub fn config(&self) -> &SystemReminderConfig {
        &self.config
    }
}

#[cfg(test)]
#[path = "orchestrator.test.rs"]
mod tests;
