use std::time::Duration;
use std::time::Instant;

use pretty_assertions::assert_eq;

use super::*;

#[test]
fn test_default_does_not_clamp() {
    let limiter = FrameRateLimiter::default();
    let now = Instant::now();
    assert_eq!(limiter.clamp_deadline(now), now);
}

#[test]
fn test_clamps_to_min_interval_after_emit() {
    let mut limiter = FrameRateLimiter::default();
    let now = Instant::now();
    limiter.mark_emitted(now);

    // Request immediately after emit — should be clamped forward.
    let clamped = limiter.clamp_deadline(now);
    assert_eq!(clamped, now + MIN_FRAME_INTERVAL);

    // Request far in the future — should not be clamped.
    let future = now + Duration::from_secs(1);
    assert_eq!(limiter.clamp_deadline(future), future);
}
