use super::*;

// TS formula: effectiveWindow = contextWindow - min(maxOutput, 20K)
//             threshold = effectiveWindow - 13K
// For 200K window, 16K max output:
//   effective = 200K - 16K = 184K
//   threshold = 184K - 13K = 171K

const CTX: i64 = 200_000;
const MAX_OUT: i64 = 16_384;

#[test]
fn test_effective_context_window() {
    assert_eq!(effective_context_window(CTX, MAX_OUT), CTX - MAX_OUT);
    // Max output capped at 20K
    assert_eq!(effective_context_window(CTX, 30_000), CTX - 20_000);
}

#[test]
fn test_auto_compact_threshold_formula() {
    let threshold = auto_compact_threshold(CTX, MAX_OUT);
    // 200K - 16384 - 13000 = 170616
    assert_eq!(threshold, CTX - MAX_OUT - 13_000);
}

#[test]
fn test_should_compact_at_threshold() {
    let threshold = auto_compact_threshold(CTX, MAX_OUT);
    assert!(should_auto_compact(threshold, CTX, MAX_OUT));
    assert!(should_auto_compact(threshold + 1, CTX, MAX_OUT));
}

#[test]
fn test_should_not_compact_below_threshold() {
    let threshold = auto_compact_threshold(CTX, MAX_OUT);
    assert!(!should_auto_compact(threshold - 1, CTX, MAX_OUT));
    assert!(!should_auto_compact(0, CTX, MAX_OUT));
}

#[test]
fn test_zero_context_window() {
    assert!(!should_auto_compact(100, 0, MAX_OUT));
}

#[test]
fn test_calculate_token_warning_state() {
    let state =
        calculate_token_warning_state(170_000, CTX, MAX_OUT, /*auto_compact_enabled*/ true);
    let effective = effective_context_window(CTX, MAX_OUT);
    // 170K is close to effective (~184K)
    assert!(state.percent_left < 10, "should have <10% left");
    assert!(state.is_above_warning_threshold, "above warning threshold");

    // Well below threshold
    let state_low = calculate_token_warning_state(50_000, CTX, MAX_OUT, true);
    assert!(state_low.percent_left > 50);
    assert!(!state_low.is_above_warning_threshold);
    assert!(!state_low.is_above_auto_compact_threshold);
}

#[test]
fn test_warning_state_auto_compact_disabled() {
    let threshold = auto_compact_threshold(CTX, MAX_OUT);
    let state = calculate_token_warning_state(
        threshold + 1000,
        CTX,
        MAX_OUT,
        /*auto_compact_enabled*/ false,
    );
    assert!(
        !state.is_above_auto_compact_threshold,
        "auto compact disabled should not trigger"
    );
}

#[test]
fn test_time_based_mc_defaults() {
    let config = TimeBasedMcConfig::default();
    assert!(!config.enabled);
    assert_eq!(config.gap_threshold_minutes, 60);
    assert_eq!(config.keep_recent, 5);
}
