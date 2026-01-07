//! Plan file reference generator.
//!
//! Injects plan content after compaction restores the plan file.
//! Matches Claude Code's "plan_file_reference" attachment type.

use crate::config::system_reminder::SystemReminderConfig;
use crate::error::Result;
use crate::system_reminder::generator::AttachmentGenerator;
use crate::system_reminder::generator::GeneratorContext;
use crate::system_reminder::throttle::ThrottleConfig;
use crate::system_reminder::types::AttachmentType;
use crate::system_reminder::types::SystemReminder;
use async_trait::async_trait;

/// Plan file reference generator.
///
/// Generates a reminder after compaction when a plan file is restored.
/// Contains the full plan content with instructions to continue if relevant.
///
/// Matches Claude Code's plan_file_reference attachment behavior.
#[derive(Debug)]
pub struct PlanFileReferenceGenerator;

impl PlanFileReferenceGenerator {
    /// Create a new plan file reference generator.
    pub fn new() -> Self {
        Self
    }
}

impl Default for PlanFileReferenceGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AttachmentGenerator for PlanFileReferenceGenerator {
    fn name(&self) -> &str {
        "plan_file_reference"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::PlanFileReference
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        // Only generate if there's a restored plan after compaction
        let Some(plan) = &ctx.restored_plan else {
            return Ok(None);
        };

        // Format content matching Claude Code's plan_file_reference format
        let content = format!(
            "A plan file exists from plan mode at: {}\n\n\
             Plan contents:\n\n{}\n\n\
             If this plan is relevant to the current work and not already complete, \
             continue working on it.",
            plan.file_path, plan.content
        );

        tracing::info!(
            generator = "plan_file_reference",
            file_path = %plan.file_path,
            "Generating plan file reference reminder after compaction"
        );

        Ok(Some(SystemReminder::new(
            AttachmentType::PlanFileReference,
            content,
        )))
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        // Always enabled when system reminders are enabled
        config.enabled
    }

    fn throttle_config(&self) -> ThrottleConfig {
        // No throttling - one-time injection controlled by restored_plan presence
        ThrottleConfig {
            min_turns_between: 0,
            min_turns_after_trigger: 0,
            max_per_session: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::system_reminder::LspDiagnosticsMinSeverity;
    use crate::system_reminder::file_tracker::FileTracker;
    use crate::system_reminder::generator::PlanState;
    use crate::system_reminder::generator::RestoredPlanInfo;
    use crate::system_reminder::types::ReminderTier;
    use std::path::Path;

    fn make_context<'a>(
        restored_plan: Option<RestoredPlanInfo>,
        file_tracker: &'a FileTracker,
        plan_state: &'a PlanState,
    ) -> GeneratorContext<'a> {
        GeneratorContext {
            turn_number: 1,
            is_main_agent: true,
            has_user_input: true,
            user_prompt: None,
            cwd: Path::new("/test"),
            agent_id: "test-agent",
            file_tracker,
            is_plan_mode: false,
            plan_file_path: None,
            is_plan_reentry: false,
            plan_state,
            background_tasks: &[],
            critical_instruction: None,
            diagnostics_store: None,
            lsp_diagnostics_min_severity: LspDiagnosticsMinSeverity::default(),
            output_style: None,
            approved_plan: None,
            restored_plan,
        }
    }

    #[tokio::test]
    async fn test_generates_when_plan_restored() {
        let generator = PlanFileReferenceGenerator::new();
        let tracker = FileTracker::new();
        let plan_state = PlanState::default();

        let restored_plan = RestoredPlanInfo {
            content: "# My Plan\n\n1. Step one\n2. Step two".to_string(),
            file_path: "/home/user/.claude/plans/bright-aurora.md".to_string(),
        };

        let ctx = make_context(Some(restored_plan), &tracker, &plan_state);
        let result = generator.generate(&ctx).await.unwrap();

        assert!(result.is_some());
        let reminder = result.unwrap();
        assert_eq!(reminder.attachment_type, AttachmentType::PlanFileReference);
        assert!(
            reminder
                .content
                .contains("A plan file exists from plan mode at:")
        );
        assert!(reminder.content.contains("Step one"));
        assert!(reminder.content.contains("bright-aurora.md"));
        assert!(reminder.content.contains("If this plan is relevant"));
    }

    #[tokio::test]
    async fn test_returns_none_without_restored_plan() {
        let generator = PlanFileReferenceGenerator::new();
        let tracker = FileTracker::new();
        let plan_state = PlanState::default();

        let ctx = make_context(None, &tracker, &plan_state);
        let result = generator.generate(&ctx).await.unwrap();

        assert!(result.is_none());
    }

    #[test]
    fn test_no_throttling() {
        let generator = PlanFileReferenceGenerator::new();
        let config = generator.throttle_config();
        assert_eq!(config.min_turns_between, 0);
        assert_eq!(config.min_turns_after_trigger, 0);
    }

    #[test]
    fn test_attachment_type() {
        let generator = PlanFileReferenceGenerator::new();
        assert_eq!(
            generator.attachment_type(),
            AttachmentType::PlanFileReference
        );
        assert_eq!(generator.tier(), ReminderTier::Core);
    }

    #[test]
    fn test_name() {
        let generator = PlanFileReferenceGenerator::new();
        assert_eq!(generator.name(), "plan_file_reference");
    }
}
