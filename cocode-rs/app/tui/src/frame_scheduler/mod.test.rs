use std::time::Duration;

use pretty_assertions::assert_eq;
use tokio::sync::broadcast;

use super::*;

/// Helper: collect draw notifications with a timeout.
async fn recv_draw(rx: &mut broadcast::Receiver<()>, timeout: Duration) -> bool {
    tokio::time::timeout(timeout, rx.recv()).await.is_ok()
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn test_schedule_frame_immediate_triggers_draw() {
    let (draw_tx, mut draw_rx) = broadcast::channel(16);
    let requester = FrameRequester::new(draw_tx);

    requester.schedule_frame();

    // The scheduler should emit within one rate-limit interval.
    let got = recv_draw(
        &mut draw_rx,
        rate_limiter::MIN_FRAME_INTERVAL + Duration::from_millis(5),
    )
    .await;
    assert!(got, "expected a draw notification");
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn test_coalesces_multiple_requests() {
    let (draw_tx, mut draw_rx) = broadcast::channel(16);
    let requester = FrameRequester::new(draw_tx);

    // Fire three requests rapidly.
    requester.schedule_frame();
    requester.schedule_frame();
    requester.schedule_frame();

    // Should produce exactly one draw.
    let got = recv_draw(
        &mut draw_rx,
        rate_limiter::MIN_FRAME_INTERVAL + Duration::from_millis(5),
    )
    .await;
    assert!(got, "expected first draw notification");

    // No second draw within another interval (nothing was requested).
    let got2 = recv_draw(
        &mut draw_rx,
        rate_limiter::MIN_FRAME_INTERVAL + Duration::from_millis(5),
    )
    .await;
    assert!(!got2, "expected no second draw notification");
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn test_schedule_frame_in_triggers_at_delay() {
    let (draw_tx, mut draw_rx) = broadcast::channel(16);
    let requester = FrameRequester::new(draw_tx);

    let delay = Duration::from_millis(100);
    requester.schedule_frame_in(delay);

    // Should NOT fire before the delay.
    let too_early = recv_draw(&mut draw_rx, delay - Duration::from_millis(10)).await;
    assert!(!too_early, "draw should not arrive before scheduled delay");

    // Should fire after the delay.
    let got = recv_draw(&mut draw_rx, Duration::from_millis(50)).await;
    assert!(got, "expected draw after scheduled delay");
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn test_rate_limits_to_120fps() {
    let (draw_tx, mut draw_rx) = broadcast::channel(16);
    let requester = FrameRequester::new(draw_tx);

    // First frame: immediate.
    requester.schedule_frame();
    let got = recv_draw(
        &mut draw_rx,
        rate_limiter::MIN_FRAME_INTERVAL + Duration::from_millis(5),
    )
    .await;
    assert!(got, "expected first draw");

    // Second frame requested immediately — should be clamped to MIN_FRAME_INTERVAL.
    requester.schedule_frame();

    // It should NOT arrive in less than half the min interval.
    let half = rate_limiter::MIN_FRAME_INTERVAL / 2;
    let too_fast = recv_draw(&mut draw_rx, half).await;
    assert!(
        !too_fast,
        "second frame should not arrive before rate limit"
    );

    // But it should arrive within the full interval plus margin.
    let got2 = recv_draw(
        &mut draw_rx,
        rate_limiter::MIN_FRAME_INTERVAL + Duration::from_millis(5),
    )
    .await;
    assert_eq!(got2, true, "expected second draw after rate limit interval");
}
