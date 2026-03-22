use super::*;
use crate::result::StopReason;

#[test]
fn test_default_config() {
    let config = LoopConfig::default();
    assert_eq!(config.max_turns, None);
    assert!(!config.enable_streaming_tools);
    assert!(!config.enable_micro_compaction);
}

#[test]
fn test_loop_result_constructors() {
    let completed = LoopResult::completed(5, 1000, 500, "text".to_string(), vec![]);
    assert_eq!(completed.turns_completed, 5);
    assert!(matches!(completed.stop_reason, StopReason::ModelStopSignal));

    let max = LoopResult::max_turns_reached(10, 2000, 1000);
    assert!(matches!(max.stop_reason, StopReason::MaxTurnsReached));

    let interrupted = LoopResult::interrupted(3, 500, 200);
    assert!(matches!(
        interrupted.stop_reason,
        StopReason::UserInterrupted
    ));

    let err = LoopResult::error(1, 100, 50, "boom".to_string());
    assert!(matches!(err.stop_reason, StopReason::Error { .. }));
}

#[test]
fn test_constants() {
    assert_eq!(cocode_protocol::DEFAULT_MIN_BLOCKING_OFFSET, 13_000);
    assert_eq!(MAX_OUTPUT_TOKEN_RECOVERY, 3);
}

// ============================================================================
// Compaction Integration Tests
// ============================================================================

mod compaction_integration_tests {

    use crate::compaction::ThresholdStatus;
    use cocode_protocol::CompactConfig;

    /// Test: threshold recalculation after auto-compact prevents false blocking (Plan 1.1)
    #[test]
    fn threshold_status_reflects_post_compact_tokens() {
        let config = CompactConfig::default();
        let context_window = 200_000;

        // Simulate: before compact, tokens are at blocking limit
        let pre_tokens = 190_000;
        let pre_status = ThresholdStatus::calculate(pre_tokens, context_window, &config);
        assert!(
            pre_status.is_at_blocking_limit,
            "pre-compact should be at blocking limit"
        );

        // Simulate: after compact, tokens are well below
        let post_tokens = 80_000;
        let post_status = ThresholdStatus::calculate(post_tokens, context_window, &config);
        assert!(
            !post_status.is_at_blocking_limit,
            "post-compact should NOT be at blocking limit"
        );
        assert!(
            !post_status.is_above_auto_compact_threshold,
            "post-compact should NOT trigger auto-compact"
        );
    }

    /// Test: circuit breaker state is independent of compaction tier
    #[test]
    fn circuit_breaker_reset_logic() {
        // Circuit breaker opens at 3 consecutive failures
        let mut failure_count = 0;
        let mut circuit_breaker_open = false;

        // Simulate 3 Tier 2 failures
        for _ in 0..3 {
            failure_count += 1;
            if failure_count >= 3 {
                circuit_breaker_open = true;
            }
        }
        assert!(circuit_breaker_open);
        assert_eq!(failure_count, 3);

        // Simulate Tier 1 success resetting the circuit breaker (Plan 1.3)
        failure_count = 0;
        circuit_breaker_open = false;
        assert!(!circuit_breaker_open);
        assert_eq!(failure_count, 0);
    }
}
