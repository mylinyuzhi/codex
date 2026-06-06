use super::*;
use crate::generator::GeneratorContext;
use crate::types::AttachmentType;
use coco_config::SystemReminderConfig;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn none_when_current_date_unset() {
    let c = SystemReminderConfig::default();
    let ctx = GeneratorContext::builder(&c).current_date(None).build();
    assert!(UserContextGenerator.generate(&ctx).await.unwrap().is_none());
}

#[tokio::test]
async fn none_when_current_date_empty() {
    let c = SystemReminderConfig::default();
    let ctx = GeneratorContext::builder(&c)
        .current_date(Some(String::new()))
        .build();
    assert!(UserContextGenerator.generate(&ctx).await.unwrap().is_none());
}

#[tokio::test]
async fn emits_prepend_user_context_body_when_date_present() {
    let c = SystemReminderConfig::default();
    let ctx = GeneratorContext::builder(&c)
        .current_date(Some("2026-06-05".to_string()))
        .build();
    let r = UserContextGenerator
        .generate(&ctx)
        .await
        .unwrap()
        .expect("emits");
    assert_eq!(r.attachment_type, AttachmentType::UserContext);
    // TS `prependUserContext` inner body (sans outer <system-reminder>),
    // currentDate-only context map. Six-space indent before IMPORTANT is
    // the TS template-literal artifact.
    assert_eq!(
        r.content(),
        Some(
            "As you answer the user's questions, you can use the following context:\n# currentDate\nToday's date is 2026-06-05.\n\n      IMPORTANT: this context may or may not be relevant to your tasks. You should not respond to this context unless it is highly relevant to your task."
        )
    );
}

#[tokio::test]
async fn fires_every_turn_via_core_tier_no_throttle() {
    assert_eq!(UserContextGenerator.throttle_config().min_turns_between, 0);
    assert_eq!(
        UserContextGenerator.attachment_type().tier(),
        crate::types::ReminderTier::Core
    );
}

#[tokio::test]
async fn respects_config_flag() {
    let mut c = SystemReminderConfig::default();
    c.attachments.user_context = false;
    assert!(!UserContextGenerator.is_enabled(&c));
}
