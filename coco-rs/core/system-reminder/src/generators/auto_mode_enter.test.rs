use super::*;
use crate::generator::GeneratorContext;
use crate::types::AttachmentType;
use coco_config::SystemReminderConfig;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn none_when_not_auto_mode() {
    let c = SystemReminderConfig::default();
    let ctx = GeneratorContext::builder(&c).is_auto_mode(false).build();
    assert!(
        AutoModeEnterGenerator
            .generate(&ctx)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn emits_full_content_when_full_flag_set() {
    let c = SystemReminderConfig::default();
    let ctx = GeneratorContext::builder(&c)
        .is_auto_mode(true)
        .set_full_content(AttachmentType::AutoMode, true)
        .build();
    let r = AutoModeEnterGenerator
        .generate(&ctx)
        .await
        .unwrap()
        .expect("emits");
    assert_eq!(r.attachment_type, AttachmentType::AutoMode);
    let text = r.content().unwrap();
    // Full text contains the 6-point list and the "## Auto Mode Active" header.
    assert!(text.starts_with("## Auto Mode Active"), "header: {text}");
    assert!(text.contains("Execute immediately"));
    assert!(text.contains("Avoid data exfiltration"));
}

#[tokio::test]
async fn emits_sparse_content_when_full_flag_unset() {
    let c = SystemReminderConfig::default();
    let ctx = GeneratorContext::builder(&c)
        .is_auto_mode(true)
        .set_full_content(AttachmentType::AutoMode, false)
        .build();
    let r = AutoModeEnterGenerator
        .generate(&ctx)
        .await
        .unwrap()
        .expect("emits");
    let text = r.content().unwrap();
    assert!(text.contains("Auto mode still active"));
    assert!(
        !text.contains("## Auto Mode Active"),
        "sparse must not include full header: {text}"
    );
}

#[tokio::test]
async fn respects_config_flag() {
    let mut c = SystemReminderConfig::default();
    c.attachments.auto_mode = false;
    assert!(!AutoModeEnterGenerator.is_enabled(&c));
}

#[tokio::test]
async fn uses_auto_mode_throttle() {
    let t = AutoModeEnterGenerator.throttle_config();
    assert_eq!(t.min_turns_between, 5);
    assert_eq!(t.full_content_every_n, Some(5));
}
