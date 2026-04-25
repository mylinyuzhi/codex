//! Parallel generator orchestration with per-generator timeout.
//!
//! Mirrors TS `getAttachments` (`attachments.ts:743-1003`): each generator is
//! a candidate, filtered by tier + config + throttle, and all survivors run
//! concurrently under a 1000ms AbortController (TS `attachments.ts:767`).
//! Generators that time out or return `Err` contribute zero reminders; the
//! turn always proceeds.
//!
//! Gate order (each must pass):
//!
//! 1. [`SystemReminderConfig::enabled`] — master switch.
//! 2. [`AttachmentGenerator::is_enabled`] — per-generator config flag.
//! 3. [`ReminderTier`] — filter subagent-only / user-prompt-only generators.
//! 4. [`ThrottleManager::should_generate`] — rate-limit gate.
//!
//! Full-content decisions are **pre-computed** before running generators so
//! a generator observing "I'm Full" always sees the throttle state from the
//! start of the turn, even if another generator mutates the manager mid-turn.

use std::sync::Arc;
use std::time::Duration;

use futures::future;
use tracing::debug;

use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleManager;
use crate::types::ReminderTier;
use crate::types::SystemReminder;
use coco_config::SYSTEM_REMINDER_DEFAULT_TIMEOUT_MS as DEFAULT_TIMEOUT_MS;
use coco_config::SystemReminderConfig;

/// The orchestrator owns the generator registry + throttle state for one
/// session. It's constructed once and reused across turns.
pub struct SystemReminderOrchestrator {
    generators: Vec<Arc<dyn AttachmentGenerator>>,
    throttle: ThrottleManager,
    timeout: Duration,
    config: SystemReminderConfig,
}

impl std::fmt::Debug for SystemReminderOrchestrator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemReminderOrchestrator")
            .field("generators", &self.generators.len())
            .field("timeout_ms", &self.timeout.as_millis())
            .field("enabled", &self.config.enabled)
            .finish()
    }
}

impl SystemReminderOrchestrator {
    /// Construct with `config` and no generators. Callers use
    /// [`add_generator`](Self::add_generator) to register each generator —
    /// Phase B+ wires the default registry at a higher level.
    pub fn new(config: SystemReminderConfig) -> Self {
        let timeout_ms = if config.timeout_ms > 0 {
            config.timeout_ms
        } else {
            DEFAULT_TIMEOUT_MS
        };
        Self {
            generators: Vec::new(),
            throttle: ThrottleManager::new(),
            timeout: Duration::from_millis(timeout_ms as u64),
            config,
        }
    }

    /// Register a generator. Generators run in parallel, but `join_all`
    /// returns results in registration order, so the registry order is also
    /// the injection order for reminders that fire on the same turn.
    pub fn add_generator(&mut self, g: Arc<dyn AttachmentGenerator>) {
        self.generators.push(g);
    }

    /// Register all built-in generators in TS return order:
    /// user-input batch, all-thread batch, then main-thread batch.
    ///
    /// Engine callers use this in preference to hand-wiring each generator.
    pub fn with_default_generators(mut self) -> Self {
        self.register_default_generators();
        self
    }

    /// In-place variant of [`with_default_generators`](Self::with_default_generators).
    pub fn register_default_generators(&mut self) {
        use crate::generators::{
            AgentListingDeltaGenerator, AgentMentionsGenerator, AgentPendingMessagesGenerator,
            AlreadyReadFileGenerator, AsyncHookResponseGenerator, AtMentionedFilesGenerator,
            AutoModeEnterGenerator, AutoModeExitGenerator, BudgetUsdGenerator,
            CompactionReminderGenerator, CompanionIntroGenerator, CriticalSystemReminderGenerator,
            DateChangeGenerator, DeferredToolsDeltaGenerator, DiagnosticsGenerator,
            EditedImageFileGenerator, HookAdditionalContextGenerator, HookBlockingErrorGenerator,
            HookStoppedContinuationGenerator, HookSuccessGenerator, IdeOpenedFileGenerator,
            IdeSelectionGenerator, InvokedSkillsGenerator, McpInstructionsDeltaGenerator,
            McpResourcesGenerator, NestedMemoryGenerator, OutputStyleGenerator,
            OutputTokenUsageGenerator, PlanModeEnterGenerator, PlanModeExitGenerator,
            PlanModeReentryGenerator, QueuedCommandGenerator, RelevantMemoriesGenerator,
            SkillListingGenerator, TaskRemindersGenerator, TaskStatusGenerator,
            TeamContextGenerator, TeammateMailboxGenerator, TodoRemindersGenerator,
            TokenUsageGenerator, UltrathinkEffortGenerator, VerifyPlanReminderGenerator,
        };

        // TS userInputAttachments (`attachments.ts:773-814`).
        self.add_generator(Arc::new(AtMentionedFilesGenerator));
        self.add_generator(Arc::new(McpResourcesGenerator));
        self.add_generator(Arc::new(AgentMentionsGenerator));

        // TS allThreadAttachments (`attachments.ts:822-941`), plus
        // relevant_memories which TS prefetches outside getAttachments but
        // renders through the same reminder path.
        self.add_generator(Arc::new(QueuedCommandGenerator));
        self.add_generator(Arc::new(DateChangeGenerator));
        self.add_generator(Arc::new(UltrathinkEffortGenerator));
        self.add_generator(Arc::new(DeferredToolsDeltaGenerator));
        self.add_generator(Arc::new(AgentListingDeltaGenerator));
        self.add_generator(Arc::new(McpInstructionsDeltaGenerator));
        self.add_generator(Arc::new(CompanionIntroGenerator));
        self.add_generator(Arc::new(NestedMemoryGenerator));
        self.add_generator(Arc::new(RelevantMemoriesGenerator));
        self.add_generator(Arc::new(SkillListingGenerator));
        self.add_generator(Arc::new(PlanModeReentryGenerator));
        self.add_generator(Arc::new(PlanModeEnterGenerator));
        self.add_generator(Arc::new(PlanModeExitGenerator));
        self.add_generator(Arc::new(AutoModeEnterGenerator));
        self.add_generator(Arc::new(AutoModeExitGenerator));
        self.add_generator(Arc::new(TodoRemindersGenerator));
        self.add_generator(Arc::new(TaskRemindersGenerator));
        self.add_generator(Arc::new(TeammateMailboxGenerator));
        self.add_generator(Arc::new(TeamContextGenerator));
        self.add_generator(Arc::new(AgentPendingMessagesGenerator));
        self.add_generator(Arc::new(CriticalSystemReminderGenerator));
        self.add_generator(Arc::new(CompactionReminderGenerator));

        // TS mainThreadAttachments (`attachments.ts:944-995`) plus hook
        // attachments produced by hook executors and rendered here.
        self.add_generator(Arc::new(IdeSelectionGenerator));
        self.add_generator(Arc::new(IdeOpenedFileGenerator));
        self.add_generator(Arc::new(OutputStyleGenerator));
        self.add_generator(Arc::new(DiagnosticsGenerator));
        self.add_generator(Arc::new(TaskStatusGenerator));
        self.add_generator(Arc::new(HookSuccessGenerator));
        self.add_generator(Arc::new(HookBlockingErrorGenerator));
        self.add_generator(Arc::new(HookAdditionalContextGenerator));
        self.add_generator(Arc::new(HookStoppedContinuationGenerator));
        self.add_generator(Arc::new(AsyncHookResponseGenerator));
        self.add_generator(Arc::new(TokenUsageGenerator));
        self.add_generator(Arc::new(BudgetUsdGenerator));
        self.add_generator(Arc::new(OutputTokenUsageGenerator));
        self.add_generator(Arc::new(VerifyPlanReminderGenerator));
        self.add_generator(Arc::new(InvokedSkillsGenerator));

        // Silent reminder-native attachments: display/transcript only.
        self.add_generator(Arc::new(AlreadyReadFileGenerator));
        self.add_generator(Arc::new(EditedImageFileGenerator));
    }

    /// Borrow the throttle manager. Exposed so the engine can inject external
    /// trigger events (`set_trigger_turn` for cooldown-gated reminders).
    pub fn throttle(&self) -> &ThrottleManager {
        &self.throttle
    }

    /// Reset all throttle state. Call at session start or after a compaction
    /// boundary when reminder cadence should restart from scratch.
    pub fn reset_throttle(&self) {
        self.throttle.reset();
    }

    /// The configured per-generator timeout.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Number of registered generators (for diagnostics / tests).
    pub fn generator_count(&self) -> usize {
        self.generators.len()
    }

    /// The [`AttachmentType`](crate::types::AttachmentType) of every
    /// registered generator, in registration order. Used by the
    /// parity test to assert every enum variant has a default
    /// generator.
    pub fn registered_attachment_types(&self) -> Vec<crate::types::AttachmentType> {
        self.generators
            .iter()
            .map(|g| g.attachment_type())
            .collect()
    }

    /// Names of every registered generator, in registration order.
    /// Cross-checked against `AttachmentKind` coverage strings by the
    /// `every_reminder_coverage_names_a_registered_generator` test.
    pub fn generator_names(&self) -> Vec<&str> {
        self.generators.iter().map(|g| g.name()).collect()
    }

    /// Config reference (for diagnostics / tests).
    pub fn config(&self) -> &SystemReminderConfig {
        &self.config
    }

    /// Run every applicable generator and collect the reminders they produce.
    ///
    /// `ctx` is taken by value so the orchestrator can pre-compute per-
    /// generator `full_content` flags into `ctx.full_content_flags` before
    /// running any generator. Generators then read the pre-computed flag via
    /// [`GeneratorContext::should_use_full_content`].
    pub async fn generate_all(&self, mut ctx: GeneratorContext<'_>) -> Vec<SystemReminder> {
        if !self.config.enabled {
            debug!("system reminders disabled globally");
            return Vec::new();
        }

        // Pre-compute Full/Sparse flags. Locking the throttle here is cheap
        // (one lookup per generator with `full_content_every_n`) and keeps
        // the decision stable for the entire turn.
        for g in &self.generators {
            let cfg = g.throttle_config_for_context(&ctx);
            if cfg.full_content_every_n.is_some() {
                let is_full = self
                    .throttle
                    .should_use_full_content(g.attachment_type(), &cfg);
                ctx.full_content_flags.insert(g.attachment_type(), is_full);
            }
        }

        // Filter by config + tier + throttle.
        let applicable: Vec<_> = self
            .generators
            .iter()
            .filter(|g| self.should_run(g.as_ref(), &ctx))
            .cloned()
            .collect();

        if applicable.is_empty() {
            debug!(turn = ctx.turn_number, "no applicable generators this turn");
            return Vec::new();
        }

        debug!(
            count = applicable.len(),
            turn = ctx.turn_number,
            generators = %applicable.iter().map(|g| g.name()).collect::<Vec<_>>().join(","),
            "running generators"
        );

        let ctx_ref = &ctx;
        let timeout_duration = self.timeout;
        let futures = applicable
            .iter()
            .map(|g| run_one_generator(Arc::clone(g), timeout_duration, ctx_ref));
        let results: Vec<Option<(crate::types::AttachmentType, SystemReminder)>> =
            future::join_all(futures).await;

        let mut reminders = Vec::new();
        for (at, reminder) in results.into_iter().flatten() {
            self.throttle.mark_generated(at, ctx.turn_number);
            reminders.push(reminder);
        }

        debug!(
            produced = reminders.len(),
            turn = ctx.turn_number,
            "orchestrator.generate_all done"
        );
        reminders
    }

    /// Combined gate: runs when config, tier and throttle all agree.
    #[allow(clippy::borrowed_box)]
    fn should_run(&self, g: &dyn AttachmentGenerator, ctx: &GeneratorContext<'_>) -> bool {
        if !g.is_enabled(&self.config) {
            return false;
        }
        match g.tier() {
            ReminderTier::Core => {}
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
        let throttle_cfg = g.throttle_config_for_context(ctx);
        if !self
            .throttle
            .should_generate(g.attachment_type(), &throttle_cfg, ctx.turn_number)
        {
            debug!(
                generator = g.name(),
                turn = ctx.turn_number,
                "generator throttled"
            );
            return false;
        }
        true
    }
}

/// Run one generator under a timeout, mapping every outcome to
/// `Option<(AttachmentType, SystemReminder)>`.
///
/// Extracted into a free function so the future's return type is a concrete
/// `Option<...>` — inline `async move` blocks inside `.map()` defeat
/// type inference in `future::join_all`.
async fn run_one_generator(
    generator: Arc<dyn AttachmentGenerator>,
    timeout_duration: std::time::Duration,
    ctx: &GeneratorContext<'_>,
) -> Option<(crate::types::AttachmentType, SystemReminder)> {
    let name = generator.name().to_string();
    let attachment_type = generator.attachment_type();
    match tokio::time::timeout(timeout_duration, generator.generate(ctx)).await {
        Ok(Ok(Some(reminder))) => {
            tracing::trace!(
                generator = %name,
                chars = reminder.content().map_or(0, str::len),
                "reminder produced"
            );
            Some((attachment_type, reminder))
        }
        Ok(Ok(None)) => None,
        Ok(Err(e)) => {
            tracing::warn!(generator = %name, error = %e, "generator failed");
            None
        }
        Err(_) => {
            tracing::warn!(
                generator = %name,
                timeout_ms = timeout_duration.as_millis() as u64,
                "generator timed out"
            );
            None
        }
    }
}

#[cfg(test)]
#[path = "orchestrator.test.rs"]
mod tests;
