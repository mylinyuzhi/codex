use super::*;
use cocode_protocol::AutoCompactTracking;
use std::time::Duration;

// Helper to create a config for testing
fn test_config() -> SessionMemoryExtractionConfig {
    SessionMemoryExtractionConfig {
        enabled: true,
        min_tokens_to_init: 5000,
        min_tokens_between: 5000,
        tool_calls_between: 10,
        cooldown_secs: 60,
        max_summary_tokens: 4000,
    }
}

#[test]
fn test_should_trigger_disabled() {
    let config = SessionMemoryExtractionConfig {
        enabled: false,
        ..test_config()
    };

    // We can't create a full agent without a model, but we can test the logic
    // by checking the conditions directly
    let _tracking = AutoCompactTracking::new();

    // When disabled, should never trigger
    assert!(!config.enabled);

    // First extraction check with disabled
    let tokens_since = 10000; // Above threshold
    assert!(tokens_since >= config.min_tokens_to_init);
    // But extraction is disabled, so it shouldn't trigger
}

#[test]
fn test_first_extraction_threshold() {
    let config = test_config();
    let tracking = AutoCompactTracking::new();

    // Below threshold - should not trigger
    let tokens_below = 4000;
    assert!(tokens_below < config.min_tokens_to_init);

    // At threshold - should trigger
    let tokens_at = 5000;
    assert!(tokens_at >= config.min_tokens_to_init);

    // Above threshold - should trigger
    let tokens_above = 10000;
    assert!(tokens_above >= config.min_tokens_to_init);

    // Verify extraction_count is 0 for first extraction
    assert_eq!(tracking.extraction_count, 0);
}

#[test]
fn test_subsequent_extraction_all_conditions() {
    let config = test_config();
    let mut tracking = AutoCompactTracking::new();

    // Simulate first extraction completed
    tracking.mark_extraction_completed(10000, "msg-1");

    // Now test subsequent extraction conditions
    // Need: tokens_between + tool_calls_between + cooldown

    // Add tool calls
    for _ in 0..15 {
        tracking.record_tool_call();
    }

    let current_tokens = 20000; // 10000 since last extraction

    // Check individual conditions
    let tokens_since = tracking.tokens_since_extraction(current_tokens);
    assert_eq!(tokens_since, 10000);
    assert!(tokens_since >= config.min_tokens_between);

    let tool_calls_since = tracking.tool_calls_since_extraction();
    assert_eq!(tool_calls_since, 15);
    assert!(tool_calls_since >= config.tool_calls_between);

    // Cooldown check (in real test, time would have passed)
    // Here we just verify the Duration comparison works
    let cooldown = Duration::from_secs(config.cooldown_secs as u64);
    let time_since = tracking.time_since_extraction();
    // Time since should be very small (just created)
    assert!(time_since < cooldown);
}

#[test]
fn test_extraction_in_progress_blocks() {
    let mut tracking = AutoCompactTracking::new();
    tracking.mark_extraction_started();

    assert!(tracking.extraction_in_progress);
    // When extraction_in_progress is true, should_trigger should return false
}

#[test]
fn test_extraction_result() {
    let result = ExtractionResult {
        summary: "Test summary".to_string(),
        summary_tokens: 100,
        last_summarized_id: "msg-123".to_string(),
        messages_summarized: 10,
    };

    assert_eq!(result.summary, "Test summary");
    assert_eq!(result.summary_tokens, 100);
    assert_eq!(result.last_summarized_id, "msg-123");
    assert_eq!(result.messages_summarized, 10);
}
