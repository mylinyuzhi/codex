use super::*;
use crate::generator::GeneratorContext;
use crate::types::AttachmentType;
use coco_config::SystemReminderConfig;
use coco_context::Phase4Variant;
use coco_context::PlanWorkflow;
use pretty_assertions::assert_eq;
use std::path::PathBuf;

fn cfg() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

// ── PlanModeEnterGenerator ──

#[tokio::test]
async fn enter_returns_none_when_not_in_plan_mode() {
    let c = cfg();
    let ctx = GeneratorContext::builder(&c).is_plan_mode(false).build();
    let g = PlanModeEnterGenerator;
    assert!(g.generate(&ctx).await.unwrap().is_none());
}

#[tokio::test]
async fn enter_emits_full_when_full_flag_set() {
    let c = cfg();
    let ctx = GeneratorContext::builder(&c)
        .is_plan_mode(true)
        .plan_file_path(Some(PathBuf::from("/tmp/plan.md")))
        .plan_exists(false)
        .set_full_content(AttachmentType::PlanMode, true)
        .build();
    let g = PlanModeEnterGenerator;
    let r = g.generate(&ctx).await.unwrap().expect("emits");
    assert_eq!(r.attachment_type, AttachmentType::PlanMode);
    let text = r.content().expect("text");
    // Full content contains the phase labels (5-phase workflow by default).
    assert!(text.contains("Phase 1"), "full = 5-phase w/ labels: {text}");
}

#[tokio::test]
async fn enter_emits_sparse_when_full_flag_unset() {
    let c = cfg();
    let ctx = GeneratorContext::builder(&c)
        .is_plan_mode(true)
        .plan_file_path(Some(PathBuf::from("/tmp/plan.md")))
        .plan_exists(false)
        .set_full_content(AttachmentType::PlanMode, false)
        .build();
    let g = PlanModeEnterGenerator;
    let r = g.generate(&ctx).await.unwrap().expect("emits");
    let text = r.content().expect("text");
    // Sparse content does NOT include "Phase 1" / "Phase 2" / etc. labels.
    assert!(
        !text.contains("### Phase 1"),
        "sparse must not include phase headers: {text}"
    );
}

#[tokio::test]
async fn enter_attachment_type_is_plan_mode() {
    let g = PlanModeEnterGenerator;
    assert_eq!(g.attachment_type(), AttachmentType::PlanMode);
    assert_eq!(g.name(), "PlanModeEnterGenerator");
}

#[tokio::test]
async fn enter_uses_plan_mode_throttle() {
    let g = PlanModeEnterGenerator;
    let t = g.throttle_config();
    assert_eq!(t.min_turns_between, 5);
    assert_eq!(t.full_content_every_n, Some(5));
}

#[tokio::test]
async fn enter_is_disabled_when_config_flag_off() {
    let mut c = cfg();
    c.attachments.plan_mode = false;
    let g = PlanModeEnterGenerator;
    assert!(!g.is_enabled(&c));
}

#[tokio::test]
async fn enter_full_sub_agent_path_is_independent_of_workflow() {
    let c = cfg();
    // Sub-agent flag overrides normal 5-phase rendering.
    let ctx = GeneratorContext::builder(&c)
        .is_plan_mode(true)
        .is_sub_agent(true)
        .plan_file_path(Some(PathBuf::from("/tmp/plan.md")))
        .plan_exists(false)
        .set_full_content(AttachmentType::PlanMode, true)
        .build();
    let g = PlanModeEnterGenerator;
    let r = g.generate(&ctx).await.unwrap().expect("emits");
    let text = r.content().expect("text");
    // Sub-agent rendering does not reference parallel Explore agent count.
    assert!(
        !text.contains("Explore agents in parallel"),
        "sub-agent prompt should not mention parallel Explore: {text}"
    );
}

#[tokio::test]
async fn enter_interview_workflow_full_content_differs_from_five_phase() {
    let c = cfg();
    let five_phase = GeneratorContext::builder(&c)
        .is_plan_mode(true)
        .plan_workflow(PlanWorkflow::FivePhase)
        .plan_file_path(Some(PathBuf::from("/tmp/plan.md")))
        .plan_exists(false)
        .set_full_content(AttachmentType::PlanMode, true)
        .build();
    let interview = GeneratorContext::builder(&c)
        .is_plan_mode(true)
        .plan_workflow(PlanWorkflow::Interview)
        .plan_file_path(Some(PathBuf::from("/tmp/plan.md")))
        .plan_exists(false)
        .set_full_content(AttachmentType::PlanMode, true)
        .build();

    let g = PlanModeEnterGenerator;
    let five_text = g
        .generate(&five_phase)
        .await
        .unwrap()
        .unwrap()
        .content()
        .unwrap()
        .to_string();
    let interview_text = g
        .generate(&interview)
        .await
        .unwrap()
        .unwrap()
        .content()
        .unwrap()
        .to_string();
    assert_ne!(five_text, interview_text, "workflows must differ");
}

#[tokio::test]
async fn enter_phase4_variant_affects_full_five_phase_only() {
    let c = cfg();
    let standard = GeneratorContext::builder(&c)
        .is_plan_mode(true)
        .phase4_variant(Phase4Variant::Standard)
        .plan_file_path(Some(PathBuf::from("/tmp/plan.md")))
        .plan_exists(false)
        .set_full_content(AttachmentType::PlanMode, true)
        .build();
    let cut = GeneratorContext::builder(&c)
        .is_plan_mode(true)
        .phase4_variant(Phase4Variant::Cut)
        .plan_file_path(Some(PathBuf::from("/tmp/plan.md")))
        .plan_exists(false)
        .set_full_content(AttachmentType::PlanMode, true)
        .build();

    let g = PlanModeEnterGenerator;
    let s_text = g
        .generate(&standard)
        .await
        .unwrap()
        .unwrap()
        .content()
        .unwrap()
        .to_string();
    let c_text = g
        .generate(&cut)
        .await
        .unwrap()
        .unwrap()
        .content()
        .unwrap()
        .to_string();
    assert_ne!(
        s_text, c_text,
        "Phase-4 variants must render different text"
    );
}

// ── PlanModeExitGenerator ──

#[tokio::test]
async fn exit_none_without_flag() {
    let c = cfg();
    let ctx = GeneratorContext::builder(&c)
        .needs_plan_mode_exit_attachment(false)
        .build();
    assert!(
        PlanModeExitGenerator
            .generate(&ctx)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn exit_emits_when_flag_set() {
    let c = cfg();
    let ctx = GeneratorContext::builder(&c)
        .needs_plan_mode_exit_attachment(true)
        .plan_file_path(Some(PathBuf::from("/tmp/plan.md")))
        .plan_exists(true)
        .build();
    let r = PlanModeExitGenerator
        .generate(&ctx)
        .await
        .unwrap()
        .expect("emits");
    assert_eq!(r.attachment_type, AttachmentType::PlanModeExit);
    let text = r.content().expect("text");
    assert!(text.contains("Exited Plan Mode"), "banner text: {text}");
    assert!(
        text.contains("/tmp/plan.md"),
        "references plan file: {text}"
    );
}

#[tokio::test]
async fn exit_omits_plan_reference_when_no_file() {
    let c = cfg();
    let ctx = GeneratorContext::builder(&c)
        .needs_plan_mode_exit_attachment(true)
        .plan_exists(false)
        .build();
    let r = PlanModeExitGenerator
        .generate(&ctx)
        .await
        .unwrap()
        .expect("emits");
    let text = r.content().expect("text");
    assert!(
        !text.contains("plan file is located"),
        "no plan → no 'plan file is located' fragment: {text}"
    );
}

#[tokio::test]
async fn exit_has_no_throttle() {
    assert_eq!(PlanModeExitGenerator.throttle_config().min_turns_between, 0);
}

// ── PlanModeReentryGenerator ──

#[tokio::test]
async fn reentry_requires_plan_mode() {
    let c = cfg();
    let ctx = GeneratorContext::builder(&c)
        .is_plan_mode(false)
        .is_plan_reentry(true)
        .plan_exists(true)
        .build();
    assert!(
        PlanModeReentryGenerator
            .generate(&ctx)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn reentry_requires_reentry_flag() {
    let c = cfg();
    let ctx = GeneratorContext::builder(&c)
        .is_plan_mode(true)
        .is_plan_reentry(false)
        .plan_exists(true)
        .build();
    assert!(
        PlanModeReentryGenerator
            .generate(&ctx)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn reentry_requires_existing_plan() {
    let c = cfg();
    let ctx = GeneratorContext::builder(&c)
        .is_plan_mode(true)
        .is_plan_reentry(true)
        .plan_exists(false)
        .build();
    assert!(
        PlanModeReentryGenerator
            .generate(&ctx)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn reentry_skipped_for_sub_agent() {
    let c = cfg();
    let ctx = GeneratorContext::builder(&c)
        .is_plan_mode(true)
        .is_plan_reentry(true)
        .plan_exists(true)
        .is_sub_agent(true)
        .plan_file_path(Some(PathBuf::from("/tmp/plan.md")))
        .build();
    assert!(
        PlanModeReentryGenerator
            .generate(&ctx)
            .await
            .unwrap()
            .is_none(),
        "TS attachments.ts:1216 gates sub-agents out of Reentry"
    );
}

#[tokio::test]
async fn reentry_emits_when_all_conditions_hold() {
    let c = cfg();
    let ctx = GeneratorContext::builder(&c)
        .is_plan_mode(true)
        .is_plan_reentry(true)
        .plan_exists(true)
        .plan_file_path(Some(PathBuf::from("/tmp/plan.md")))
        .build();
    let r = PlanModeReentryGenerator
        .generate(&ctx)
        .await
        .unwrap()
        .expect("emits");
    assert_eq!(r.attachment_type, AttachmentType::PlanModeReentry);
    let text = r.content().expect("text");
    assert!(
        text.contains("Re-entering Plan Mode"),
        "banner text: {text}"
    );
    assert!(
        text.contains("/tmp/plan.md"),
        "references plan file: {text}"
    );
}
