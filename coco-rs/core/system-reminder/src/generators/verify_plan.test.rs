use super::*;
use crate::generator::GeneratorContext;
use crate::types::AttachmentType;
use crate::types::ReminderTier;
use coco_config::SystemReminderConfig;

fn cfg() -> SystemReminderConfig {
    let mut c = SystemReminderConfig::default();
    c.attachments.verify_plan_reminder = true;
    c
}

#[tokio::test]
async fn skips_when_config_disabled() {
    let c = SystemReminderConfig::default();
    assert!(!VerifyPlanReminderGenerator.is_enabled(&c));
}

#[tokio::test]
async fn skips_when_no_pending_verification() {
    let c = cfg();
    let ctx = GeneratorContext::builder(&c)
        .has_pending_plan_verification(false)
        .turns_since_plan_exit(10)
        .build();
    assert!(
        VerifyPlanReminderGenerator
            .generate(&ctx)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn skips_when_turn_count_zero() {
    let c = cfg();
    let ctx = GeneratorContext::builder(&c)
        .has_pending_plan_verification(true)
        .turns_since_plan_exit(0)
        .build();
    assert!(
        VerifyPlanReminderGenerator
            .generate(&ctx)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn skips_when_not_on_10_turn_boundary() {
    let c = cfg();
    for n in [1, 5, 9, 11, 15, 19, 21] {
        let ctx = GeneratorContext::builder(&c)
            .has_pending_plan_verification(true)
            .turns_since_plan_exit(n)
            .build();
        assert!(
            VerifyPlanReminderGenerator
                .generate(&ctx)
                .await
                .unwrap()
                .is_none(),
            "turn {n} should not fire"
        );
    }
}

#[tokio::test]
async fn fires_on_10_turn_boundaries() {
    let c = cfg();
    for n in [10, 20, 30, 100] {
        let ctx = GeneratorContext::builder(&c)
            .has_pending_plan_verification(true)
            .turns_since_plan_exit(n)
            .build();
        let r = VerifyPlanReminderGenerator
            .generate(&ctx)
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("turn {n} should fire"));
        assert_eq!(r.attachment_type, AttachmentType::VerifyPlanReminder);
        let text = r.content().unwrap();
        assert!(text.contains("VerifyPlanExecution"));
        assert!(text.contains("NOT the Agent tool"));
    }
}

#[tokio::test]
async fn tier_is_main_agent_only() {
    assert_eq!(
        VerifyPlanReminderGenerator.tier(),
        ReminderTier::MainAgentOnly
    );
}

#[tokio::test]
async fn identity_accessors() {
    assert_eq!(
        VerifyPlanReminderGenerator.attachment_type(),
        AttachmentType::VerifyPlanReminder
    );
    assert_eq!(
        VerifyPlanReminderGenerator.name(),
        "VerifyPlanReminderGenerator"
    );
}
