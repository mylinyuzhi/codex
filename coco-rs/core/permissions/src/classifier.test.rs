use super::*;

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
fn test_parse_classifier_response_allow() {
    let result =
        parse_classifier_response(r#"{"should_block": false, "reason": "Safe git command"}"#);
    assert!(!result.should_block);
    assert_eq!(result.reason, "Safe git command");
}

#[test]
fn test_parse_classifier_response_block() {
    let result =
        parse_classifier_response(r#"{"should_block": true, "reason": "Destructive operation"}"#);
    assert!(result.should_block);
}

#[test]
fn test_parse_classifier_response_invalid_json() {
    let result = parse_classifier_response("not json");
    assert!(result.should_block); // Safe default
}

#[test]
fn test_build_system_prompt_with_rules() {
    let rules = AutoModeRules {
        allow: vec!["git status".into(), "cargo test".into()],
        soft_deny: vec!["rm -rf".into()],
        environment: vec!["Rust project".into()],
    };
    let prompt = build_classifier_system_prompt(&rules);
    assert!(prompt.contains("git status"));
    assert!(prompt.contains("rm -rf"));
    assert!(prompt.contains("Rust project"));
}

#[tokio::test]
async fn test_classify_safe_tool_skips_llm() {
    let result = classify_yolo_action(
        &[],
        "Read",
        &serde_json::json!({"file_path": "foo.rs"}),
        &AutoModeRules::default(),
        |_req: ClassifyRequest| async { panic!("should not call LLM for safe tool") },
    )
    .await;
    assert!(!result.should_block);
}

#[tokio::test]
async fn test_classify_bash_calls_llm() {
    let result = classify_yolo_action(
        &[],
        "Bash",
        &serde_json::json!({"command": "ls -la"}),
        &AutoModeRules::default(),
        |_req: ClassifyRequest| async {
            Ok("<answer>allow</answer><reason>read-only command</reason>".to_string())
        },
    )
    .await;
    assert!(!result.should_block);
    assert_eq!(result.reason, "read-only command");
}

#[tokio::test]
async fn test_classify_error_defaults_to_block() {
    let result = classify_yolo_action(
        &[],
        "Bash",
        &serde_json::json!({"command": "rm -rf /"}),
        &AutoModeRules::default(),
        |_req: ClassifyRequest| async { Err("API error".to_string()) },
    )
    .await;
    assert!(result.should_block);
}

#[tokio::test]
async fn test_classify_xml_block_response() {
    let result = classify_yolo_action(
        &[],
        "Bash",
        &serde_json::json!({"command": "rm -rf /"}),
        &AutoModeRules::default(),
        |_req: ClassifyRequest| async {
            Ok("<answer>block</answer><reason>Destructive operation</reason>".to_string())
        },
    )
    .await;
    assert!(result.should_block);
    assert_eq!(result.reason, "Destructive operation");
}

#[tokio::test]
async fn test_classify_stage2_fallback_on_ambiguous() {
    let call_count = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(0));
    let cc = call_count.clone();
    let result = classify_yolo_action(
        &[],
        "Write",
        &serde_json::json!({"file_path": "/etc/passwd", "content": "hack"}),
        &AutoModeRules::default(),
        move |req: ClassifyRequest| {
            let cc = cc.clone();
            async move {
                let stage = cc.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                if stage == 1 {
                    // Stage 1: ambiguous (no XML tags).
                    Ok("I'm not sure about this one.".to_string())
                } else {
                    // Stage 2: clear block.
                    Ok("<answer>block</answer><reason>Writes to system file</reason>".to_string())
                }
            }
        },
    )
    .await;
    assert!(result.should_block);
    assert_eq!(result.stage, Some(2));
}
