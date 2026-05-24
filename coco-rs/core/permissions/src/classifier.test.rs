use super::*;

const EMPTY_MESSAGES: &[coco_messages::Message] = &[];

#[test]
fn test_is_safe_tool() {
    assert!(is_safe_tool("Read"));
    assert!(is_safe_tool("Glob"));
    assert!(is_safe_tool("TaskCreate"));
    assert!(!is_safe_tool("Bash"));
    assert!(!is_safe_tool("Write"));
    assert!(!is_safe_tool("Edit"));
}

#[test]
fn test_format_action() {
    let action = format_action_for_classifier("Bash", &serde_json::json!({"command": "rm -rf /"}));
    assert!(action.contains("<action>"));
    assert!(action.contains("tool: Bash"));
    assert!(action.contains("rm -rf"));
}

#[test]
fn test_parse_xml_block_yes() {
    assert_eq!(parse_xml_block("<block>yes</block>"), Some(true));
    assert_eq!(parse_xml_block("<block>YES</block>"), Some(true));
    // Stage-1 closing tag absent (stopped on `</block>`).
    assert_eq!(parse_xml_block("<block>yes"), Some(true));
}

#[test]
fn test_parse_xml_block_no() {
    assert_eq!(parse_xml_block("<block>no</block>"), Some(false));
    assert_eq!(parse_xml_block("<block>No</block>"), Some(false));
    assert_eq!(parse_xml_block("<block>no"), Some(false));
}

#[test]
fn test_parse_xml_block_unparseable() {
    assert_eq!(parse_xml_block(""), None);
    assert_eq!(parse_xml_block("I'm not sure"), None);
    assert_eq!(parse_xml_block("<block>maybe</block>"), None);
}

#[test]
fn test_parse_xml_reason() {
    assert_eq!(
        parse_xml_reason("<reason>Destructive operation</reason>"),
        Some("Destructive operation".into())
    );
    assert_eq!(
        parse_xml_reason("<reason>  trim spaces  </reason>"),
        Some("trim spaces".into())
    );
    assert_eq!(parse_xml_reason("no reason"), None);
}

#[test]
fn test_strip_thinking_does_not_match_inner_tags() {
    // Regression: a `<block>` *inside* `<thinking>` must not be parsed.
    let text = "<thinking>I'd say <block>yes</block> for safety</thinking><block>no</block>";
    assert_eq!(parse_xml_block(text), Some(false));
}

#[test]
fn test_strip_thinking_handles_unterminated_thinking() {
    // If the response truncates inside <thinking>, the unterminated
    // segment must be ignored so we don't match its contents.
    let text = "<block>no</block>\n\n<thinking>then I would also...";
    assert_eq!(parse_xml_block(text), Some(false));
}

#[test]
fn test_build_system_prompt_with_rules_and_xml_format() {
    let rules = AutoModeRules {
        allow: vec!["git status".into(), "cargo test".into()],
        soft_deny: vec!["rm -rf".into()],
        environment: vec!["Rust project".into()],
    };
    let prompt = build_classifier_system_prompt(&rules);
    assert!(prompt.contains("git status"));
    assert!(prompt.contains("rm -rf"));
    assert!(prompt.contains("Rust project"));
    // TS-faithful output-format block.
    assert!(prompt.contains("## Output Format"));
    assert!(prompt.contains("<block>yes</block><reason>"));
    assert!(prompt.contains("<block>no</block>"));
    assert!(prompt.contains("Your ENTIRE response MUST begin with <block>"));
}

#[tokio::test]
async fn test_classify_safe_tool_skips_llm() {
    let result = classify_yolo_action(
        EMPTY_MESSAGES,
        "Read",
        &serde_json::json!({"file_path": "foo.rs"}),
        &AutoModeRules::default(),
        |_req: ClassifyRequest| async { panic!("should not call LLM for safe tool") },
    )
    .await;
    assert!(!result.should_block);
}

#[tokio::test]
async fn test_classify_stage1_allow_short_circuits() {
    let result = classify_yolo_action(
        EMPTY_MESSAGES,
        "Bash",
        &serde_json::json!({"command": "ls -la"}),
        &AutoModeRules::default(),
        |req: ClassifyRequest| async move {
            // Stage 1 must request `</block>` stop and 64-token budget.
            assert_eq!(req.stage, 1);
            assert_eq!(req.max_tokens, 64);
            assert_eq!(req.stop_sequences, Some(vec!["</block>".to_string()]));
            // Stage-1 stop_sequences truncate before the closer.
            Ok("<block>no".to_string())
        },
    )
    .await;
    assert!(!result.should_block);
    assert_eq!(result.stage, Some(1));
}

#[tokio::test]
async fn test_classify_stage1_block_escalates_to_stage2() {
    let call_count = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(0));
    let cc = call_count.clone();
    let result = classify_yolo_action(
        EMPTY_MESSAGES,
        "Bash",
        &serde_json::json!({"command": "rm -rf /"}),
        &AutoModeRules::default(),
        move |req: ClassifyRequest| {
            let cc = cc.clone();
            async move {
                let stage = cc.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                if stage == 1 {
                    assert_eq!(req.stage, 1);
                    Ok("<block>yes".to_string())
                } else {
                    // Stage 2 drops stop sequences and bumps budget.
                    assert_eq!(req.stage, 2);
                    assert_eq!(req.max_tokens, 4096);
                    assert_eq!(req.stop_sequences, None);
                    Ok("<block>yes</block><reason>Destructive</reason>".to_string())
                }
            }
        },
    )
    .await;
    assert!(result.should_block);
    assert_eq!(result.reason, "Destructive");
    assert_eq!(result.stage, Some(2));
}

#[tokio::test]
async fn test_classify_stage1_unparseable_escalates_to_stage2() {
    let call_count = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(0));
    let cc = call_count.clone();
    let result = classify_yolo_action(
        EMPTY_MESSAGES,
        "Write",
        &serde_json::json!({"file_path": "/etc/passwd", "content": "hack"}),
        &AutoModeRules::default(),
        move |_req: ClassifyRequest| {
            let cc = cc.clone();
            async move {
                let stage = cc.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                if stage == 1 {
                    Ok("I'm not sure about this one.".to_string())
                } else {
                    Ok("<block>yes</block><reason>Writes to system file</reason>".to_string())
                }
            }
        },
    )
    .await;
    assert!(result.should_block);
    assert_eq!(result.stage, Some(2));
}

#[tokio::test]
async fn test_classify_stage2_unparseable_blocks_for_safety() {
    let result = classify_yolo_action(
        EMPTY_MESSAGES,
        "Bash",
        &serde_json::json!({"command": "rm -rf /"}),
        &AutoModeRules::default(),
        |_req: ClassifyRequest| async { Ok("garbage output".to_string()) },
    )
    .await;
    assert!(result.should_block);
    assert_eq!(result.stage, Some(2));
    assert!(result.reason.contains("unparseable"));
}

#[tokio::test]
async fn test_classify_error_defaults_to_block() {
    let result = classify_yolo_action(
        EMPTY_MESSAGES,
        "Bash",
        &serde_json::json!({"command": "rm -rf /"}),
        &AutoModeRules::default(),
        |_req: ClassifyRequest| async { Err("API error".to_string()) },
    )
    .await;
    assert!(result.should_block);
    assert_eq!(result.stage, Some(2));
}
