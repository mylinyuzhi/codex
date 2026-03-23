use super::*;
use pretty_assertions::assert_eq;
use std::time::Duration;
use std::time::Instant;

fn snapshot(queued_lines: i32, oldest_age_ms: u64) -> QueueSnapshot {
    QueueSnapshot {
        queued_lines,
        oldest_age: Some(Duration::from_millis(oldest_age_ms)),
    }
}

#[test]
fn test_smooth_mode_is_default() {
    let mut policy = AdaptiveChunkingPolicy::default();
    let now = Instant::now();

    let decision = policy.decide(snapshot(1, 10), now);
    assert_eq!(decision.mode, ChunkingMode::Smooth);
    assert!(!decision.entered_catch_up);
    assert_eq!(decision.drain_plan, DrainPlan::Single);
}

#[test]
fn test_enters_catch_up_on_depth_threshold() {
    let mut policy = AdaptiveChunkingPolicy::default();
    let now = Instant::now();

    let decision = policy.decide(snapshot(8, 10), now);
    assert_eq!(decision.mode, ChunkingMode::CatchUp);
    assert!(decision.entered_catch_up);
    assert_eq!(decision.drain_plan, DrainPlan::Batch(8));
}

#[test]
fn test_enters_catch_up_on_age_threshold() {
    let mut policy = AdaptiveChunkingPolicy::default();
    let now = Instant::now();

    let decision = policy.decide(snapshot(2, 120), now);
    assert_eq!(decision.mode, ChunkingMode::CatchUp);
    assert!(decision.entered_catch_up);
    assert_eq!(decision.drain_plan, DrainPlan::Batch(2));
}

#[test]
fn test_catch_up_batch_drains_current_backlog() {
    let mut policy = AdaptiveChunkingPolicy::default();
    let now = Instant::now();
    let decision = policy.decide(snapshot(512, 400), now);
    assert_eq!(decision.mode, ChunkingMode::CatchUp);
    assert_eq!(decision.drain_plan, DrainPlan::Batch(512));
}

#[test]
fn test_exits_catch_up_after_hysteresis_hold() {
    let mut policy = AdaptiveChunkingPolicy::default();
    let t0 = Instant::now();

    let _ = policy.decide(snapshot(9, 10), t0);
    assert_eq!(policy.mode(), ChunkingMode::CatchUp);

    // Still in catch-up before hold expires
    let pre_hold = policy.decide(snapshot(2, 40), t0 + Duration::from_millis(200));
    assert_eq!(pre_hold.mode, ChunkingMode::CatchUp);

    // Exits after hold
    let post_hold = policy.decide(snapshot(2, 40), t0 + Duration::from_millis(460));
    assert_eq!(post_hold.mode, ChunkingMode::Smooth);
    assert_eq!(post_hold.drain_plan, DrainPlan::Single);
}

#[test]
fn test_drops_back_to_smooth_when_idle() {
    let mut policy = AdaptiveChunkingPolicy::default();
    let now = Instant::now();
    let _ = policy.decide(snapshot(9, 10), now);
    assert_eq!(policy.mode(), ChunkingMode::CatchUp);

    let decision = policy.decide(
        QueueSnapshot {
            queued_lines: 0,
            oldest_age: None,
        },
        now + Duration::from_millis(20),
    );
    assert_eq!(decision.mode, ChunkingMode::Smooth);
    assert_eq!(decision.drain_plan, DrainPlan::Single);
}

#[test]
fn test_holds_reentry_after_catch_up_exit() {
    let mut policy = AdaptiveChunkingPolicy::default();
    let t0 = Instant::now();

    let entered = policy.decide(snapshot(8, 20), t0);
    assert_eq!(entered.mode, ChunkingMode::CatchUp);

    // Drain to zero → exit catch-up
    let drained = policy.decide(
        QueueSnapshot {
            queued_lines: 0,
            oldest_age: None,
        },
        t0 + Duration::from_millis(20),
    );
    assert_eq!(drained.mode, ChunkingMode::Smooth);

    // Re-entry suppressed during hold
    let held = policy.decide(snapshot(8, 20), t0 + Duration::from_millis(120));
    assert_eq!(held.mode, ChunkingMode::Smooth);
    assert_eq!(held.drain_plan, DrainPlan::Single);

    // Re-entry allowed after hold expires
    let reentered = policy.decide(snapshot(8, 20), t0 + Duration::from_millis(320));
    assert_eq!(reentered.mode, ChunkingMode::CatchUp);
    assert_eq!(reentered.drain_plan, DrainPlan::Batch(8));
}

#[test]
fn test_severe_backlog_bypasses_reentry_hold() {
    let mut policy = AdaptiveChunkingPolicy::default();
    let t0 = Instant::now();

    let _ = policy.decide(snapshot(8, 20), t0);
    let _ = policy.decide(
        QueueSnapshot {
            queued_lines: 0,
            oldest_age: None,
        },
        t0 + Duration::from_millis(20),
    );

    // Severe backlog bypasses re-entry hold
    let severe = policy.decide(snapshot(64, 20), t0 + Duration::from_millis(120));
    assert_eq!(severe.mode, ChunkingMode::CatchUp);
    assert_eq!(severe.drain_plan, DrainPlan::Batch(64));
}

#[test]
fn test_reset_clears_state() {
    let mut policy = AdaptiveChunkingPolicy::default();
    let now = Instant::now();
    let _ = policy.decide(snapshot(9, 10), now);
    assert_eq!(policy.mode(), ChunkingMode::CatchUp);

    policy.reset();
    assert_eq!(policy.mode(), ChunkingMode::Smooth);
}
