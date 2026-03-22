use super::*;

#[test]
fn test_new_tracker() {
    let tracker = SkillUsageTracker::new();
    assert_eq!(tracker.count("commit"), 0);
    assert_eq!(tracker.score("commit"), 0.0);
}

#[test]
fn test_track_increments() {
    let tracker = SkillUsageTracker::new();
    tracker.track("commit");
    assert_eq!(tracker.count("commit"), 1);
    tracker.track("commit");
    assert_eq!(tracker.count("commit"), 2);
}

#[test]
fn test_score_positive_after_track() {
    let tracker = SkillUsageTracker::new();
    tracker.track("commit");
    let score = tracker.score("commit");
    // Just-used skill: decay ≈ 1.0, so score ≈ 1.0
    assert!(score > 0.9, "score should be near 1.0, got {score}");
}

#[test]
fn test_independent_skills() {
    let tracker = SkillUsageTracker::new();
    tracker.track("commit");
    tracker.track("commit");
    tracker.track("review");
    assert_eq!(tracker.count("commit"), 2);
    assert_eq!(tracker.count("review"), 1);
    assert_eq!(tracker.count("deploy"), 0);
}

#[test]
fn test_score_floor() {
    // The floor is 0.1, so even with high decay, score >= count * 0.1
    let tracker = SkillUsageTracker::new();
    tracker.track("old-skill");
    let score = tracker.score("old-skill");
    // Score should be at least 0.1 (floor) regardless of time
    assert!(score >= 0.1);
}
