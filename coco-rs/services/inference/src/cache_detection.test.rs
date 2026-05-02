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
        agent_id: None,
        fast_mode: false,
        betas: Vec::new(),
        extra_body_hash: 0,
        extra_body_serialized: None,
        effort_value: String::new(),
        global_cache_strategy: String::new(),
        auto_mode_active: false,
        is_using_overage: false,
        cached_mc_enabled: false,
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
    let result =
        detector.check_response_for_cache_break("repl_main_thread", 10000, 5000, None, None);
    assert_eq!(result.state, CacheState::Cold);

    // Second call with same state, similar cache tokens — Warm
    let input2 = make_input("claude-sonnet-4-20250514", 111, 222);
    detector.record_prompt_state(input2);
    let result = detector.check_response_for_cache_break(
        "repl_main_thread",
        9800, // within 5% of 10000
        200,
        None,
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
    let _ = detector.check_response_for_cache_break("repl_main_thread", 50000, 5000, None, None);

    // Second call with changed system prompt
    let input2 = make_input("claude-sonnet-4-20250514", 999, 222);
    detector.record_prompt_state(input2);
    let result = detector.check_response_for_cache_break(
        "repl_main_thread",
        1000, // big drop from 50000
        49000,
        Some(1000), // 1 second ago
        None,
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
    let _ = detector.check_response_for_cache_break("repl_main_thread", 50000, 5000, None, None);

    // Switch model
    let input2 = make_input("claude-opus-4-20250514", 111, 222);
    detector.record_prompt_state(input2);
    let result = detector.check_response_for_cache_break(
        "repl_main_thread",
        0, // full miss
        55000,
        Some(1000),
        None,
    );
    assert_eq!(result.state, CacheState::Broken);
    assert!(result.reason.contains("model changed"));
}

#[test]
fn test_cache_break_ttl_expiry() {
    let mut detector = CacheBreakDetector::new();

    let input1 = make_input("claude-sonnet-4-20250514", 111, 222);
    detector.record_prompt_state(input1);
    let _ = detector.check_response_for_cache_break("repl_main_thread", 50000, 5000, None, None);

    // Same state but >1h gap
    let input2 = make_input("claude-sonnet-4-20250514", 111, 222);
    detector.record_prompt_state(input2);
    let result = detector.check_response_for_cache_break(
        "repl_main_thread",
        0,
        55000,
        Some(3_700_000), // ~1h 1min
        None,
    );
    assert_eq!(result.state, CacheState::Broken);
    assert!(result.reason.contains("1h TTL"));
}

#[test]
fn test_cache_deletion_suppresses_break() {
    let mut detector = CacheBreakDetector::new();

    let input1 = make_input("claude-sonnet-4-20250514", 111, 222);
    detector.record_prompt_state(input1);
    let _ = detector.check_response_for_cache_break("repl_main_thread", 50000, 5000, None, None);

    // Notify cache deletion
    detector.notify_cache_deletion("repl_main_thread", None);

    let input2 = make_input("claude-sonnet-4-20250514", 111, 222);
    detector.record_prompt_state(input2);
    let result = detector.check_response_for_cache_break(
        "repl_main_thread",
        10000, // big drop but expected
        40000,
        None,
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
    let _ = detector.check_response_for_cache_break("repl_main_thread", 50000, 5000, None, None);

    // Compaction resets
    detector.notify_compaction("repl_main_thread", None);

    let input2 = make_input("claude-sonnet-4-20250514", 111, 222);
    detector.record_prompt_state(input2);
    let result = detector.check_response_for_cache_break(
        "repl_main_thread",
        5000, // big drop from 50000 but baseline was reset
        45000,
        None,
        None,
    );
    assert_eq!(result.state, CacheState::Cold);
}

#[test]
fn test_untracked_source_ignored() {
    let mut detector = CacheBreakDetector::new();
    let result = detector.check_response_for_cache_break("speculation", 0, 10000, None, None);
    assert_eq!(result.state, CacheState::Cold);
}

#[test]
fn test_compact_shares_repl_tracking() {
    let mut detector = CacheBreakDetector::new();

    // Record state via repl
    let input = make_input("claude-sonnet-4-20250514", 111, 222);
    detector.record_prompt_state(input);
    let _ = detector.check_response_for_cache_break("repl_main_thread", 50000, 5000, None, None);

    // Check via compact — shares the same tracking key
    let input2 = {
        let mut inp = make_input("claude-sonnet-4-20250514", 111, 222);
        inp.query_source = "compact".into();
        inp
    };
    detector.record_prompt_state(input2);
    let result = detector.check_response_for_cache_break("compact", 49000, 1000, Some(1000), None);
    // Should be warm (not cold) because it shares repl's baseline
    assert_eq!(result.state, CacheState::Warm);
}

#[test]
fn test_tool_schema_change_detection() {
    let mut detector = CacheBreakDetector::new();

    let input1 = make_input("claude-sonnet-4-20250514", 111, 222);
    detector.record_prompt_state(input1);
    let _ = detector.check_response_for_cache_break("repl_main_thread", 50000, 5000, None, None);

    // Change tools hash and one per-tool hash
    let input2 = {
        let mut inp = make_input("claude-sonnet-4-20250514", 111, 333);
        inp.per_tool_hashes.insert("Write".into(), 999);
        inp
    };
    detector.record_prompt_state(input2);
    let result =
        detector.check_response_for_cache_break("repl_main_thread", 1000, 49000, Some(1000), None);
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
        betas_changed: false,
        extra_body_changed: false,
        effort_changed: false,
        global_cache_strategy_changed: false,
        auto_mode_changed: false,
        overage_changed: false,
        cached_mc_changed: false,
        added_tool_count: 0,
        removed_tool_count: 0,
        system_char_delta: 150,
        added_tools: vec![],
        removed_tools: vec![],
        changed_tool_schemas: vec![],
        added_betas: vec![],
        removed_betas: vec![],
        previous_model: "old-model".into(),
        new_model: "new-model".into(),
        prev_effort_value: String::new(),
        new_effort_value: String::new(),
        prev_global_cache_strategy: String::new(),
        new_global_cache_strategy: String::new(),
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
    let _ = detector.check_response_for_cache_break("repl_main_thread", 5000, 1000, None, None);

    // Drop of 1500 tokens (below MIN_CACHE_MISS_TOKENS=2000), even though >5%
    let input2 = make_input("claude-sonnet-4-20250514", 999, 222);
    detector.record_prompt_state(input2);
    let result =
        detector.check_response_for_cache_break("repl_main_thread", 3500, 1500, Some(1000), None);
    assert_eq!(result.state, CacheState::Warm);
}

#[test]
fn test_haiku_model_excluded() {
    let mut detector = CacheBreakDetector::new();

    let input1 = make_input("claude-haiku-4-5-20251001", 111, 222);
    detector.record_prompt_state(input1);
    let _ = detector.check_response_for_cache_break("repl_main_thread", 50000, 5000, None, None);

    // Big drop on a haiku model — must NOT trigger Broken. Phase 1
    // skips for excluded models so phase 2 sees "no prior state"
    // instead of "excluded model" — both yield Cold which is what
    // matters for the false-positive-suppression contract.
    let input2 = make_input("claude-haiku-4-5-20251001", 999, 222);
    detector.record_prompt_state(input2);
    let result =
        detector.check_response_for_cache_break("repl_main_thread", 100, 49000, Some(1000), None);
    assert_eq!(result.state, CacheState::Cold);
}

#[test]
fn test_agent_id_isolates_concurrent_subagents() {
    let mut detector = CacheBreakDetector::new();

    // Two concurrent agents of the same type but different ids — must NOT
    // share a tracking entry.
    let mut a1 = make_input("claude-sonnet-4-20250514", 111, 222);
    a1.query_source = "agent:custom".into();
    a1.agent_id = Some("agent-aaa".into());
    detector.record_prompt_state(a1);
    let _ = detector.check_response_for_cache_break(
        "agent:custom",
        50000,
        5000,
        None,
        Some("agent-aaa"),
    );

    let mut a2 = make_input("claude-sonnet-4-20250514", 333, 444);
    a2.query_source = "agent:custom".into();
    a2.agent_id = Some("agent-bbb".into());
    detector.record_prompt_state(a2);
    // First call for agent bbb: Cold even though a1's hashes differ.
    let result = detector.check_response_for_cache_break(
        "agent:custom",
        50000,
        5000,
        None,
        Some("agent-bbb"),
    );
    assert_eq!(result.state, CacheState::Cold);
}

#[test]
fn test_cleanup_agent_drops_state() {
    let mut detector = CacheBreakDetector::new();
    let mut input = make_input("claude-sonnet-4-20250514", 111, 222);
    input.query_source = "agent:custom".into();
    input.agent_id = Some("agent-zzz".into());
    detector.record_prompt_state(input);
    assert!(!detector.states.is_empty());

    detector.cleanup_agent("agent-zzz");
    assert!(detector.states.is_empty());
}

#[test]
fn test_extra_body_change_attributed() {
    let mut detector = CacheBreakDetector::new();

    let mut input1 = make_input("claude-sonnet-4-20250514", 111, 222);
    input1.extra_body_hash = 1111;
    detector.record_prompt_state(input1);
    let _ = detector.check_response_for_cache_break("repl_main_thread", 50000, 5000, None, None);

    let mut input2 = make_input("claude-sonnet-4-20250514", 111, 222);
    input2.extra_body_hash = 2222;
    detector.record_prompt_state(input2);
    let result =
        detector.check_response_for_cache_break("repl_main_thread", 100, 49000, Some(1000), None);
    assert_eq!(result.state, CacheState::Broken);
    assert!(result.reason.contains("provider options changed"));
    assert!(result.changes.expect("changes").extra_body_changed);
}

#[test]
fn test_canonical_extra_body_hash_stable_under_key_reorder() {
    // serde_json::Map (BTreeMap default) serializes alphabetically, but a
    // round-trip through canonicalize_value must produce identical bytes
    // regardless of HashMap-style insertion noise. Two equivalent payloads
    // hash the same.
    let v1: serde_json::Value = serde_json::json!({
        "anthropic": {
            "betas": ["a", "b"],
            "thinking": {"type": "enabled", "budgetTokens": 8000}
        }
    });
    let v2: serde_json::Value = serde_json::json!({
        "anthropic": {
            "thinking": {"budgetTokens": 8000, "type": "enabled"},
            "betas": ["a", "b"]
        }
    });
    assert_eq!(
        canonical_extra_body_hash(&v1),
        canonical_extra_body_hash(&v2)
    );
}

#[test]
fn test_canonical_extra_body_hash_zero_for_null() {
    assert_eq!(canonical_extra_body_hash(&serde_json::Value::Null), 0);
}

#[test]
fn test_canonical_extra_body_hash_distinguishes_distinct() {
    let v1: serde_json::Value = serde_json::json!({"a": 1});
    let v2: serde_json::Value = serde_json::json!({"a": 2});
    assert_ne!(
        canonical_extra_body_hash(&v1),
        canonical_extra_body_hash(&v2)
    );
}

#[test]
fn test_djb2_hash_deterministic() {
    assert_eq!(djb2_hash(b"hello"), djb2_hash(b"hello"));
    assert_ne!(djb2_hash(b"hello"), djb2_hash(b"world"));
}

#[test]
fn test_excluded_model_skipped_in_phase1() {
    // Recording state for an excluded model must NOT populate the
    // `states` map — otherwise haiku-only sessions accumulate
    // snapshots that phase 2 always discards anyway (wasted memory).
    let mut detector = CacheBreakDetector::new();
    let input = make_input("claude-haiku-4-5-20251001", 111, 222);
    detector.record_prompt_state(input);
    assert!(
        detector.states.is_empty(),
        "excluded model should not populate states"
    );
}
