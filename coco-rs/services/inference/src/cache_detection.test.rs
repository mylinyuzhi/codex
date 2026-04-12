use super::*;
use pretty_assertions::assert_eq;
use std::collections::HashMap;

fn make_input(model: &str, system_hash: u64, tools_hash: u64) -> PromptStateInput {
    PromptStateInput {
        system_hash,
        tools_hash,
        cache_control_hash: 0,
        tool_names: vec!["Read".into(), "Write".into(), "Bash".into()],
        per_tool_hashes: HashMap::from([
            ("Read".into(), 100),
            ("Write".into(), 200),
            ("Bash".into(), 300),
        ]),
        system_char_count: 5000,
        model: model.into(),
        query_source: "repl_main_thread".into(),
        fast_mode: false,
    }
}

#[test]
fn test_cold_start_no_previous_state() {
    let mut detector = CacheBreakDetector::new();
    let result = detector.check_response_for_cache_break(
        "repl_main_thread",
        /*cache_read_tokens*/ 10000,
        /*cache_creation_tokens*/ 5000,
        None,
    );
    assert_eq!(result.state, CacheState::Cold);
}

#[test]
fn test_warm_cache_stable_tokens() {
    let mut detector = CacheBreakDetector::new();
    let input = make_input("claude-sonnet-4-20250514", 111, 222);
    detector.record_prompt_state(input);

    // First call — Cold (sets baseline)
    let result = detector.check_response_for_cache_break("repl_main_thread", 10000, 5000, None);
    assert_eq!(result.state, CacheState::Cold);

    // Second call with same state, similar cache tokens — Warm
    let input2 = make_input("claude-sonnet-4-20250514", 111, 222);
    detector.record_prompt_state(input2);
    let result = detector.check_response_for_cache_break(
        "repl_main_thread",
        9800, // within 5% of 10000
        200,
        None,
    );
    assert_eq!(result.state, CacheState::Warm);
}

#[test]
fn test_cache_break_detected_with_system_change() {
    let mut detector = CacheBreakDetector::new();

    // First call
    let input1 = make_input("claude-sonnet-4-20250514", 111, 222);
    detector.record_prompt_state(input1);
    let _ = detector.check_response_for_cache_break("repl_main_thread", 50000, 5000, None);

    // Second call with changed system prompt
    let input2 = make_input("claude-sonnet-4-20250514", 999, 222);
    detector.record_prompt_state(input2);
    let result = detector.check_response_for_cache_break(
        "repl_main_thread",
        1000, // big drop from 50000
        49000,
        Some(1000), // 1 second ago
    );
    assert_eq!(result.state, CacheState::Broken);
    assert!(result.reason.contains("system prompt changed"));
    assert!(
        result
            .changes
            .as_ref()
            .expect("changes")
            .system_prompt_changed
    );
}

#[test]
fn test_cache_break_model_change() {
    let mut detector = CacheBreakDetector::new();

    let input1 = make_input("claude-sonnet-4-20250514", 111, 222);
    detector.record_prompt_state(input1);
    let _ = detector.check_response_for_cache_break("repl_main_thread", 50000, 5000, None);

    // Switch model
    let input2 = make_input("claude-opus-4-20250514", 111, 222);
    detector.record_prompt_state(input2);
    let result = detector.check_response_for_cache_break(
        "repl_main_thread",
        0, // full miss
        55000,
        Some(1000),
    );
    assert_eq!(result.state, CacheState::Broken);
    assert!(result.reason.contains("model changed"));
}

#[test]
fn test_cache_break_ttl_expiry() {
    let mut detector = CacheBreakDetector::new();

    let input1 = make_input("claude-sonnet-4-20250514", 111, 222);
    detector.record_prompt_state(input1);
    let _ = detector.check_response_for_cache_break("repl_main_thread", 50000, 5000, None);

    // Same state but >1h gap
    let input2 = make_input("claude-sonnet-4-20250514", 111, 222);
    detector.record_prompt_state(input2);
    let result = detector.check_response_for_cache_break(
        "repl_main_thread",
        0,
        55000,
        Some(3_700_000), // ~1h 1min
    );
    assert_eq!(result.state, CacheState::Broken);
    assert!(result.reason.contains("1h TTL"));
}

#[test]
fn test_cache_deletion_suppresses_break() {
    let mut detector = CacheBreakDetector::new();

    let input1 = make_input("claude-sonnet-4-20250514", 111, 222);
    detector.record_prompt_state(input1);
    let _ = detector.check_response_for_cache_break("repl_main_thread", 50000, 5000, None);

    // Notify cache deletion
    detector.notify_cache_deletion("repl_main_thread");

    let input2 = make_input("claude-sonnet-4-20250514", 111, 222);
    detector.record_prompt_state(input2);
    let result = detector.check_response_for_cache_break(
        "repl_main_thread",
        10000, // big drop but expected
        40000,
        None,
    );
    assert_eq!(result.state, CacheState::Warm);
    assert!(result.reason.contains("expected drop"));
}

#[test]
fn test_compaction_resets_baseline() {
    let mut detector = CacheBreakDetector::new();

    let input1 = make_input("claude-sonnet-4-20250514", 111, 222);
    detector.record_prompt_state(input1);
    let _ = detector.check_response_for_cache_break("repl_main_thread", 50000, 5000, None);

    // Compaction resets
    detector.notify_compaction("repl_main_thread");

    let input2 = make_input("claude-sonnet-4-20250514", 111, 222);
    detector.record_prompt_state(input2);
    let result = detector.check_response_for_cache_break(
        "repl_main_thread",
        5000, // big drop from 50000 but baseline was reset
        45000,
        None,
    );
    assert_eq!(result.state, CacheState::Cold);
}

#[test]
fn test_untracked_source_ignored() {
    let mut detector = CacheBreakDetector::new();
    let result = detector.check_response_for_cache_break("speculation", 0, 10000, None);
    assert_eq!(result.state, CacheState::Cold);
}

#[test]
fn test_compact_shares_repl_tracking() {
    let mut detector = CacheBreakDetector::new();

    // Record state via repl
    let input = make_input("claude-sonnet-4-20250514", 111, 222);
    detector.record_prompt_state(input);
    let _ = detector.check_response_for_cache_break("repl_main_thread", 50000, 5000, None);

    // Check via compact — shares the same tracking key
    let input2 = {
        let mut inp = make_input("claude-sonnet-4-20250514", 111, 222);
        inp.query_source = "compact".into();
        inp
    };
    detector.record_prompt_state(input2);
    let result = detector.check_response_for_cache_break("compact", 49000, 1000, Some(1000));
    // Should be warm (not cold) because it shares repl's baseline
    assert_eq!(result.state, CacheState::Warm);
}

#[test]
fn test_tool_schema_change_detection() {
    let mut detector = CacheBreakDetector::new();

    let input1 = make_input("claude-sonnet-4-20250514", 111, 222);
    detector.record_prompt_state(input1);
    let _ = detector.check_response_for_cache_break("repl_main_thread", 50000, 5000, None);

    // Change tools hash and one per-tool hash
    let input2 = {
        let mut inp = make_input("claude-sonnet-4-20250514", 111, 333);
        inp.per_tool_hashes.insert("Write".into(), 999);
        inp
    };
    detector.record_prompt_state(input2);
    let result =
        detector.check_response_for_cache_break("repl_main_thread", 1000, 49000, Some(1000));
    assert_eq!(result.state, CacheState::Broken);
    let changes = result.changes.expect("should have changes");
    assert!(changes.tool_schemas_changed);
    assert_eq!(changes.changed_tool_schemas, vec!["Write"]);
}

#[test]
fn test_pending_changes_explain() {
    let changes = PendingChanges {
        system_prompt_changed: true,
        tool_schemas_changed: false,
        model_changed: true,
        fast_mode_changed: false,
        cache_control_changed: false,
        added_tool_count: 0,
        removed_tool_count: 0,
        system_char_delta: 150,
        added_tools: vec![],
        removed_tools: vec![],
        changed_tool_schemas: vec![],
        previous_model: "old-model".into(),
        new_model: "new-model".into(),
    };
    let explanation = changes.explain();
    assert_eq!(explanation.len(), 2);
    assert!(explanation[0].contains("model changed"));
    assert!(explanation[1].contains("system prompt changed (+150 chars)"));
}

#[test]
fn test_reset_clears_all_state() {
    let mut detector = CacheBreakDetector::new();
    let input = make_input("claude-sonnet-4-20250514", 111, 222);
    detector.record_prompt_state(input);
    assert!(!detector.states.is_empty());

    detector.reset();
    assert!(detector.states.is_empty());
    assert!(detector.pending_changes.is_empty());
}

#[test]
fn test_small_token_drop_below_threshold_is_warm() {
    let mut detector = CacheBreakDetector::new();

    let input1 = make_input("claude-sonnet-4-20250514", 111, 222);
    detector.record_prompt_state(input1);
    let _ = detector.check_response_for_cache_break("repl_main_thread", 5000, 1000, None);

    // Drop of 1500 tokens (below MIN_CACHE_MISS_TOKENS=2000), even though >5%
    let input2 = make_input("claude-sonnet-4-20250514", 999, 222);
    detector.record_prompt_state(input2);
    let result =
        detector.check_response_for_cache_break("repl_main_thread", 3500, 1500, Some(1000));
    assert_eq!(result.state, CacheState::Warm);
}
