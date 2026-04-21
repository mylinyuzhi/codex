use super::*;
use crate::types::AttachmentType;
use pretty_assertions::assert_eq;

// ── Preset constants must match TS verbatim ──

#[test]
fn plan_mode_preset_matches_ts_constants() {
    let c = ThrottleConfig::plan_mode();
    // TS PLAN_MODE_ATTACHMENT_CONFIG.TURNS_BETWEEN_ATTACHMENTS = 5
    assert_eq!(c.min_turns_between, 5);
    // TS PLAN_MODE_ATTACHMENT_CONFIG.FULL_REMINDER_EVERY_N_ATTACHMENTS = 5
    assert_eq!(c.full_content_every_n, Some(5));
    assert_eq!(c.max_per_session, None);
}

#[test]
fn auto_mode_preset_matches_plan_mode() {
    assert_eq!(
        ThrottleConfig::auto_mode().min_turns_between,
        ThrottleConfig::plan_mode().min_turns_between
    );
    assert_eq!(
        ThrottleConfig::auto_mode().full_content_every_n,
        ThrottleConfig::plan_mode().full_content_every_n
    );
}

#[test]
fn todo_reminder_preset_matches_ts_10_turns() {
    assert_eq!(ThrottleConfig::todo_reminder().min_turns_between, 10);
}

#[test]
fn verify_plan_reminder_preset_matches_ts_10_turns() {
    assert_eq!(ThrottleConfig::verify_plan_reminder().min_turns_between, 10);
}

// ── Gate logic ──

#[test]
fn first_generation_always_allowed() {
    let m = ThrottleManager::new();
    assert!(m.should_generate(AttachmentType::PlanMode, &ThrottleConfig::plan_mode(), 0));
    assert!(m.should_generate(AttachmentType::PlanMode, &ThrottleConfig::plan_mode(), 1000));
}

#[test]
fn min_turns_between_blocks_too_soon() {
    let m = ThrottleManager::new();
    let cfg = ThrottleConfig::plan_mode();
    m.mark_generated(AttachmentType::PlanMode, 10);
    // 10 + 5 = 15; anything before 15 is blocked.
    assert!(!m.should_generate(AttachmentType::PlanMode, &cfg, 11));
    assert!(!m.should_generate(AttachmentType::PlanMode, &cfg, 14));
    assert!(m.should_generate(AttachmentType::PlanMode, &cfg, 15));
    assert!(m.should_generate(AttachmentType::PlanMode, &cfg, 100));
}

#[test]
fn max_per_session_blocks_after_cap() {
    let m = ThrottleManager::new();
    let cfg = ThrottleConfig {
        max_per_session: Some(2),
        ..ThrottleConfig::none()
    };
    m.mark_generated(AttachmentType::PlanMode, 1);
    m.mark_generated(AttachmentType::PlanMode, 2);
    assert!(!m.should_generate(AttachmentType::PlanMode, &cfg, 3));
}

#[test]
fn min_turns_after_trigger_blocks_during_cooldown() {
    let m = ThrottleManager::new();
    let cfg = ThrottleConfig {
        min_turns_between: 0,
        min_turns_after_trigger: 5,
        ..ThrottleConfig::none()
    };
    m.set_trigger_turn(AttachmentType::PlanMode, 10);
    assert!(!m.should_generate(AttachmentType::PlanMode, &cfg, 11));
    assert!(!m.should_generate(AttachmentType::PlanMode, &cfg, 14));
    assert!(m.should_generate(AttachmentType::PlanMode, &cfg, 15));
}

#[test]
fn clear_trigger_removes_cooldown() {
    let m = ThrottleManager::new();
    let cfg = ThrottleConfig {
        min_turns_after_trigger: 5,
        ..ThrottleConfig::none()
    };
    m.set_trigger_turn(AttachmentType::PlanMode, 10);
    assert!(!m.should_generate(AttachmentType::PlanMode, &cfg, 12));
    m.clear_trigger_turn(AttachmentType::PlanMode);
    assert!(m.should_generate(AttachmentType::PlanMode, &cfg, 12));
}

// ── Full/sparse cadence matches TS attachments.ts:1229 ──

#[test]
fn full_content_on_first_generation() {
    let m = ThrottleManager::new();
    let cfg = ThrottleConfig::plan_mode();
    assert!(m.should_use_full_content(AttachmentType::PlanMode, &cfg));
}

#[test]
fn full_sparse_cycle_matches_ts_1_6_11_pattern() {
    // TS: attachmentCount % 5 === 1 is Full → attachments #1, #6, #11 are Full.
    // Our session_count increments AFTER generation, so we check
    // should_use_full_content BEFORE mark_generated each cycle.
    let m = ThrottleManager::new();
    let cfg = ThrottleConfig::plan_mode();
    let at = AttachmentType::PlanMode;

    // #1 (session_count=0): Full
    assert!(m.should_use_full_content(at, &cfg), "#1 must be Full");
    m.mark_generated(at, 1);
    // #2 (session_count=1): Sparse
    assert!(!m.should_use_full_content(at, &cfg), "#2 must be Sparse");
    m.mark_generated(at, 6);
    // #3 (session_count=2)
    assert!(!m.should_use_full_content(at, &cfg), "#3 must be Sparse");
    m.mark_generated(at, 11);
    // #4
    assert!(!m.should_use_full_content(at, &cfg), "#4 must be Sparse");
    m.mark_generated(at, 16);
    // #5
    assert!(!m.should_use_full_content(at, &cfg), "#5 must be Sparse");
    m.mark_generated(at, 21);
    // #6 (session_count=5): Full again
    assert!(m.should_use_full_content(at, &cfg), "#6 must be Full");
}

#[test]
fn full_content_every_n_none_is_always_full() {
    let m = ThrottleManager::new();
    let cfg = ThrottleConfig::none();
    m.mark_generated(AttachmentType::PlanMode, 1);
    m.mark_generated(AttachmentType::PlanMode, 2);
    m.mark_generated(AttachmentType::PlanMode, 3);
    assert!(m.should_use_full_content(AttachmentType::PlanMode, &cfg));
}

#[test]
fn full_content_every_n_zero_is_always_full() {
    // Guard against a divide-by-zero or nonsense config.
    let m = ThrottleManager::new();
    let cfg = ThrottleConfig {
        full_content_every_n: Some(0),
        ..ThrottleConfig::none()
    };
    m.mark_generated(AttachmentType::PlanMode, 1);
    assert!(m.should_use_full_content(AttachmentType::PlanMode, &cfg));
}

// ── Reset + introspection ──

#[test]
fn reset_clears_all_state() {
    let m = ThrottleManager::new();
    m.mark_generated(AttachmentType::PlanMode, 10);
    m.set_trigger_turn(AttachmentType::PlanModeExit, 5);
    m.reset();
    assert_eq!(m.get_state(AttachmentType::PlanMode), None);
    assert_eq!(m.get_state(AttachmentType::PlanModeExit), None);
}

#[test]
fn get_state_snapshots_current_fields() {
    let m = ThrottleManager::new();
    m.mark_generated(AttachmentType::PlanMode, 7);
    let s = m.get_state(AttachmentType::PlanMode).expect("has state");
    assert_eq!(s.last_generated_turn, Some(7));
    assert_eq!(s.session_count, 1);
    assert_eq!(s.trigger_turn, None);
}

#[test]
fn independent_attachment_types_dont_interfere() {
    let m = ThrottleManager::new();
    let cfg = ThrottleConfig::plan_mode();
    m.mark_generated(AttachmentType::PlanMode, 5);
    // PlanModeExit has its own state → should_generate returns true.
    assert!(m.should_generate(AttachmentType::PlanModeExit, &cfg, 5));
}
