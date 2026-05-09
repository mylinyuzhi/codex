use super::*;

#[test]
fn read_only_agents_recognised() {
    assert!(is_read_only_agent("Explore"));
    assert!(is_read_only_agent("Plan"));
    assert!(is_read_only_agent("coco-guide"));
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
fn stage1_prompts_include_handoff_review_text_and_metadata() {
    let (sys, user) = stage1_prompts("worker", "[user] hello\n[assistant] hi", 3);
    assert!(sys.contains("hand-off"));
    // TS-faithful hand-off review framing must appear verbatim — the
    // classifier's training surface anchors on this exact phrasing
    // (`agentToolUtils.ts:417`).
    assert!(user.contains(HANDOFF_REVIEW_USER_PROMPT));
    assert!(user.contains("Sub-agent type: worker"));
    assert!(user.contains("Tool uses: 3"));
    assert!(user.contains("[user] hello"));
    assert!(user.contains("`SAFE`"));
}

#[test]
fn stage2_prompts_carry_stage1_verdict_and_review_framing() {
    let (sys, user) = stage2_prompts("BLOCKED: rm -rf /", "transcript body");
    assert!(sys.contains("second-stage"));
    assert!(user.contains(HANDOFF_REVIEW_USER_PROMPT));
    assert!(user.contains("BLOCKED: rm -rf /"));
    assert!(user.contains("transcript body"));
}

#[test]
fn handoff_classifier_active_requires_auto_mode_and_feature() {
    assert!(handoff_classifier_active(Some("auto"), true));
    assert!(!handoff_classifier_active(Some("auto"), false));
    assert!(!handoff_classifier_active(Some("acceptEdits"), true));
    assert!(!handoff_classifier_active(None, true));
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
fn render_block_message_uses_ts_warning_format() {
    let m = render_block_message(&HandoffClassification::Blocked {
        reason: "deleted prod credentials".to_string(),
    });
    let msg = m.expect("Some for blocked");
    // TS-faithful prefix from `agentToolUtils.ts:476`.
    assert!(msg.starts_with("SECURITY WARNING:"));
    assert!(msg.contains("violate security policy"));
    assert!(msg.contains("Reason: deleted prod credentials"));
    assert!(msg.contains("Review the sub-agent's actions carefully"));
}

#[test]
fn render_block_message_returns_none_for_safe() {
    assert!(render_block_message(&HandoffClassification::Safe).is_none());
}

#[test]
fn render_block_message_handles_empty_reason() {
    // Classifier returning bare "BLOCKED" parses to `reason = ""`. The
    // payload still has to be a self-contained sentence — collapse the
    // empty reason to "unspecified safety concern" so neither the
    // `Reason: .` nor the trailing review hint reads broken.
    let bare_blocked = parse_classifier_response("BLOCKED");
    let msg = render_block_message(&bare_blocked).expect("Some for blocked");
    assert!(msg.contains("unspecified safety concern"));
    assert!(!msg.contains("Reason: ."), "got: {msg}");
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
