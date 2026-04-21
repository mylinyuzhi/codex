use super::*;
use crate::error::Result;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::ReminderTier;
use crate::types::SystemReminder;
use async_trait::async_trait;
use pretty_assertions::assert_eq;

// ── Minimal mock generator for trait-shape tests ──

#[derive(Debug)]
struct MockGen {
    name: &'static str,
    at: AttachmentType,
    tier_override: Option<ReminderTier>,
    enabled: bool,
    output: Option<String>,
}

#[async_trait]
impl AttachmentGenerator for MockGen {
    fn name(&self) -> &str {
        self.name
    }

    fn attachment_type(&self) -> AttachmentType {
        self.at
    }

    fn tier(&self) -> ReminderTier {
        self.tier_override
            .unwrap_or_else(|| self.attachment_type().tier())
    }

    fn is_enabled(&self, _config: &SystemReminderConfig) -> bool {
        self.enabled
    }

    fn throttle_config(&self) -> ThrottleConfig {
        ThrottleConfig::plan_mode()
    }

    async fn generate(&self, _ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        Ok(self
            .output
            .as_ref()
            .map(|s| SystemReminder::new(self.at, s)))
    }
}

#[test]
fn default_tier_delegates_to_attachment_type() {
    let g = MockGen {
        name: "M",
        at: AttachmentType::PlanMode,
        tier_override: None,
        enabled: true,
        output: None,
    };
    assert_eq!(g.tier(), ReminderTier::Core);
}

#[test]
fn tier_override_takes_precedence() {
    let g = MockGen {
        name: "M",
        at: AttachmentType::PlanMode,
        tier_override: Some(ReminderTier::MainAgentOnly),
        enabled: true,
        output: None,
    };
    assert_eq!(g.tier(), ReminderTier::MainAgentOnly);
}

#[tokio::test]
async fn generate_returns_none_when_no_output() {
    let cfg = SystemReminderConfig::default();
    let ctx = GeneratorContext::builder(&cfg).build();
    let g = MockGen {
        name: "M",
        at: AttachmentType::PlanMode,
        tier_override: None,
        enabled: true,
        output: None,
    };
    assert!(g.generate(&ctx).await.unwrap().is_none());
}

#[tokio::test]
async fn generate_returns_reminder_with_content() {
    let cfg = SystemReminderConfig::default();
    let ctx = GeneratorContext::builder(&cfg).build();
    let g = MockGen {
        name: "M",
        at: AttachmentType::PlanMode,
        tier_override: None,
        enabled: true,
        output: Some("hello".to_string()),
    };
    let r = g.generate(&ctx).await.unwrap().expect("has reminder");
    assert_eq!(r.attachment_type, AttachmentType::PlanMode);
    assert_eq!(r.content(), Some("hello"));
}

#[test]
fn throttle_config_for_context_defaults_to_static() {
    let g = MockGen {
        name: "M",
        at: AttachmentType::PlanMode,
        tier_override: None,
        enabled: true,
        output: None,
    };
    let cfg = SystemReminderConfig::default();
    let ctx = GeneratorContext::builder(&cfg).build();
    assert_eq!(
        g.throttle_config_for_context(&ctx).min_turns_between,
        g.throttle_config().min_turns_between
    );
}

// ── Builder defaults + setters ──

#[test]
fn builder_defaults_are_sane() {
    let cfg = SystemReminderConfig::default();
    let ctx = GeneratorContext::builder(&cfg).build();
    assert_eq!(ctx.turn_number, 0);
    assert!(ctx.is_main_agent);
    assert!(!ctx.has_user_input);
    assert!(!ctx.is_plan_mode);
    assert!(!ctx.is_plan_reentry);
    assert!(!ctx.is_plan_interview_phase);
    assert!(!ctx.needs_plan_mode_exit_attachment);
    assert!(!ctx.needs_auto_mode_exit_attachment);
    assert!(!ctx.plan_exists);
    assert!(!ctx.is_sub_agent);
    assert_eq!(ctx.explore_agent_count, super::DEFAULT_EXPLORE_AGENT_COUNT);
    assert_eq!(ctx.plan_agent_count, super::DEFAULT_PLAN_AGENT_COUNT);
    assert_eq!(ctx.agent_id, None);
    assert_eq!(ctx.last_human_turn_uuid, None);
    assert!(ctx.full_content_flags.is_empty());
}

#[test]
fn agent_counts_are_clamped_at_build() {
    let cfg = SystemReminderConfig::default();
    let lo = GeneratorContext::builder(&cfg).agent_counts(0, 0).build();
    assert_eq!(lo.explore_agent_count, super::MIN_AGENTS);
    assert_eq!(lo.plan_agent_count, super::MIN_AGENTS);

    let hi = GeneratorContext::builder(&cfg)
        .agent_counts(100, 100)
        .build();
    assert_eq!(hi.explore_agent_count, super::MAX_AGENTS);
    assert_eq!(hi.plan_agent_count, super::MAX_AGENTS);
}

#[test]
fn builder_chains_all_setters() {
    use std::path::PathBuf;
    use uuid::Uuid;
    let cfg = SystemReminderConfig::default();
    let uuid = Uuid::new_v4();
    let ctx = GeneratorContext::builder(&cfg)
        .turn_number(7)
        .is_main_agent(false)
        .has_user_input(true)
        .is_plan_mode(true)
        .is_plan_reentry(true)
        .is_plan_interview_phase(true)
        .plan_file_path(Some(PathBuf::from("/tmp/plan.md")))
        .agent_id(Some("sub-1".to_string()))
        .last_human_turn_uuid(Some(uuid))
        .user_input(Some("hello".to_string()))
        .set_full_content(AttachmentType::PlanMode, true)
        .build();
    assert_eq!(ctx.turn_number, 7);
    assert!(!ctx.is_main_agent);
    assert!(ctx.has_user_input);
    assert!(ctx.is_plan_mode);
    assert!(ctx.is_plan_reentry);
    assert!(ctx.is_plan_interview_phase);
    assert_eq!(ctx.plan_file_path, Some(PathBuf::from("/tmp/plan.md")));
    assert_eq!(ctx.agent_id.as_deref(), Some("sub-1"));
    assert_eq!(ctx.last_human_turn_uuid, Some(uuid));
    assert_eq!(ctx.user_input.as_deref(), Some("hello"));
    assert_eq!(ctx.should_use_full_content(AttachmentType::PlanMode), true);
}

#[test]
fn should_use_full_content_defaults_to_true_when_unset() {
    let cfg = SystemReminderConfig::default();
    let ctx = GeneratorContext::builder(&cfg).build();
    // Unset → default Full (matches full_content_every_n = None semantics).
    assert!(ctx.should_use_full_content(AttachmentType::PlanMode));
}

#[test]
fn full_content_flags_replace_wholesale() {
    use std::collections::HashMap;
    let cfg = SystemReminderConfig::default();
    let mut flags = HashMap::new();
    flags.insert(AttachmentType::PlanMode, false);
    flags.insert(AttachmentType::PlanModeExit, true);
    let ctx = GeneratorContext::builder(&cfg)
        .full_content_flags(flags)
        .build();
    assert!(!ctx.should_use_full_content(AttachmentType::PlanMode));
    assert!(ctx.should_use_full_content(AttachmentType::PlanModeExit));
}
