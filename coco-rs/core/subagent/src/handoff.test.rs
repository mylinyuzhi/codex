use super::*;

#[test]
fn read_only_agents_recognised() {
    assert!(is_read_only_agent("Explore"));
    assert!(is_read_only_agent("Plan"));
    assert!(is_read_only_agent("claude-code-guide"));
    assert!(!is_read_only_agent("general-purpose"));
    assert!(!is_read_only_agent("statusline-setup"));
}

#[test]
fn should_classify_skips_read_only_and_zero_tool_runs() {
    assert!(!should_classify("Explore", 5));
    assert!(!should_classify("general-purpose", 0));
    assert!(should_classify("general-purpose", 1));
}

#[test]
fn stage1_prompts_include_agent_metadata_and_transcript() {
    let (sys, user) = stage1_prompts("worker", "[user] hello\n[assistant] hi", 3);
    assert!(sys.contains("safety classifier"));
    assert!(user.contains("Agent type: worker"));
    assert!(user.contains("Tool uses: 3"));
    assert!(user.contains("[user] hello"));
    assert!(user.contains("Respond with SAFE"));
}

#[test]
fn stage2_prompts_carry_stage1_verdict() {
    let (sys, user) = stage2_prompts("BLOCKED: rm -rf /", "transcript body");
    assert!(sys.contains("second-stage"));
    assert!(user.contains("BLOCKED: rm -rf /"));
    assert!(user.contains("transcript body"));
}

#[test]
fn parse_safe_response_returns_safe() {
    assert_eq!(
        parse_classifier_response("SAFE"),
        HandoffClassification::Safe
    );
    assert_eq!(
        parse_classifier_response("  SAFE — looks clean"),
        HandoffClassification::Safe
    );
    assert_eq!(
        parse_classifier_response("safe"),
        HandoffClassification::Safe
    );
}

#[test]
fn parse_empty_response_fails_open_to_safe() {
    assert_eq!(
        parse_classifier_response("   "),
        HandoffClassification::Safe
    );
}

#[test]
fn parse_blocked_response_strips_prefix() {
    let v = parse_classifier_response("BLOCKED: ran a destructive shell command");
    match v {
        HandoffClassification::Blocked { reason } => {
            assert_eq!(reason, "ran a destructive shell command");
        }
        _ => panic!("expected Blocked"),
    }
}

#[test]
fn parse_unmatched_response_treats_as_blocked() {
    let v = parse_classifier_response("the agent did something weird");
    matches!(v, HandoffClassification::Blocked { .. });
}

#[test]
fn render_block_message_wraps_reason() {
    let m = render_block_message(&HandoffClassification::Blocked {
        reason: "deleted prod credentials".to_string(),
    });
    let msg = m.expect("Some for blocked");
    assert!(msg.starts_with("SECURITY:"));
    assert!(msg.contains("deleted prod credentials"));
}

#[test]
fn render_block_message_returns_none_for_safe() {
    assert!(render_block_message(&HandoffClassification::Safe).is_none());
}

#[test]
fn render_block_message_handles_empty_reason() {
    // Classifier returning bare "BLOCKED" parses to `reason = ""`. Without
    // a fallback the rendered payload would end on a dangling em-dash —
    // collapse to "unspecified safety concern" so the model gets a
    // self-contained sentence.
    let bare_blocked = parse_classifier_response("BLOCKED");
    let msg = render_block_message(&bare_blocked).expect("Some for blocked");
    assert!(
        !msg.ends_with("— "),
        "rendered payload must not end on a dangling em-dash; got {msg:?}"
    );
    assert!(msg.contains("unspecified safety concern"));
}

#[test]
fn build_transcript_summary_compresses_tool_blocks() {
    let messages = vec![
        serde_json::json!({"role": "user", "content": "do the thing"}),
        serde_json::json!({"role": "assistant", "content": [
            {"type": "text", "text": "Let me search"},
            {"type": "tool_use", "name": "Grep", "input": {}}
        ]}),
        serde_json::json!({"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": "tu_1", "content": "file1.rs\nfile2.rs"}
        ]}),
    ];
    let summary = build_transcript_summary(&messages);
    assert!(summary.contains("[user] do the thing"));
    assert!(summary.contains("[assistant] Let me search"));
    assert!(summary.contains("[assistant] tool_use: Grep"));
    assert!(summary.contains("[user] tool_result"));
    // Tool result body NOT included.
    assert!(!summary.contains("file1.rs"));
}
