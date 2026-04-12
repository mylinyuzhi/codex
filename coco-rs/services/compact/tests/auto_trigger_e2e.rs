//! Integration tests for auto-compact threshold calculations.

use coco_compact::auto_trigger::TimeBasedMcConfig;
use coco_compact::auto_trigger::auto_compact_threshold;
use coco_compact::auto_trigger::calculate_token_warning_state;
use coco_compact::auto_trigger::effective_context_window;
use coco_compact::auto_trigger::should_auto_compact;
use coco_compact::reactive::ReactiveCompactConfig;
use coco_compact::reactive::should_reactive_compact;

#[test]
fn test_200k_window_threshold() {
    // TS formula: effective = 200K - min(16K, 20K) = 184K
    //             threshold = 184K - 13K = 171K
    let ctx = 200_000;
    let max_out = 16_384;

    assert_eq!(effective_context_window(ctx, max_out), ctx - max_out);
    assert_eq!(auto_compact_threshold(ctx, max_out), ctx - max_out - 13_000);

    // Just below threshold: no compact
    let threshold = auto_compact_threshold(ctx, max_out);
    assert!(!should_auto_compact(threshold - 1, ctx, max_out));
    // At threshold: compact
    assert!(should_auto_compact(threshold, ctx, max_out));
}

#[test]
fn test_1m_window_threshold() {
    // effective = 1M - min(16K, 20K) = 983616
    // threshold = 983616 - 13000 = 970616
    let ctx = 1_000_000;
    let max_out = 16_384;

    let effective = effective_context_window(ctx, max_out);
    assert_eq!(effective, 983_616);

    let threshold = auto_compact_threshold(ctx, max_out);
    assert_eq!(threshold, 970_616);
}

#[test]
fn test_max_output_capped_at_20k() {
    // If max_output_tokens > 20K, it's capped at 20K
    let ctx = 200_000;
    let max_out = 30_000;

    // effective = 200K - min(30K, 20K) = 200K - 20K = 180K
    assert_eq!(effective_context_window(ctx, max_out), 180_000);
}

#[test]
fn test_warning_state_progression() {
    let ctx = 200_000;
    let max_out = 16_384;
    let effective = effective_context_window(ctx, max_out);

    // Well below all thresholds
    let low = calculate_token_warning_state(50_000, ctx, max_out, true);
    assert!(low.percent_left > 50);
    assert!(!low.is_above_warning_threshold);
    assert!(!low.is_above_error_threshold);
    assert!(!low.is_above_auto_compact_threshold);
    assert!(!low.is_at_blocking_limit);

    // Above warning threshold (within 20K of effective)
    let warning = calculate_token_warning_state(effective - 15_000, ctx, max_out, true);
    assert!(warning.is_above_warning_threshold);
    assert!(warning.is_above_error_threshold); // same buffer (20K)

    // Above auto-compact threshold
    let threshold = auto_compact_threshold(ctx, max_out);
    let auto = calculate_token_warning_state(threshold + 1000, ctx, max_out, true);
    assert!(auto.is_above_auto_compact_threshold);

    // At blocking limit
    let blocking_limit = effective - 3_000; // MANUAL_COMPACT_BUFFER
    let blocked = calculate_token_warning_state(blocking_limit + 1, ctx, max_out, true);
    assert!(blocked.is_at_blocking_limit);
}

#[test]
fn test_reactive_higher_than_auto() {
    let ctx = 200_000;
    let max_out = 16_384;

    let auto_threshold = auto_compact_threshold(ctx, max_out);
    let effective = effective_context_window(ctx, max_out);
    let reactive_threshold = effective * 95 / 100; // ReactiveCompactConfig default

    assert!(
        reactive_threshold > auto_threshold,
        "reactive ({reactive_threshold}) should be higher than auto ({auto_threshold})"
    );

    // Token count between auto and reactive: auto triggers, reactive doesn't
    let between = (auto_threshold + reactive_threshold) / 2;
    assert!(should_auto_compact(between, ctx, max_out));

    let reactive_config = ReactiveCompactConfig {
        context_window: ctx,
        max_output_tokens: max_out,
        ..Default::default()
    };
    assert!(!should_reactive_compact(between, &reactive_config));
}

#[test]
fn test_time_based_mc_defaults() {
    let config = TimeBasedMcConfig::default();
    assert!(!config.enabled);
    assert_eq!(config.gap_threshold_minutes, 60);
    assert_eq!(config.keep_recent, 5);
}
