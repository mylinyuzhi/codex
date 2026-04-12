use super::*;

#[test]
fn test_build_fork_context_replaces_tool_results() {
    let messages = vec![
        serde_json::json!({
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Let me search"},
                {"type": "tool_use", "id": "tu_1", "name": "Bash", "input": {"command": "ls"}}
            ]
        }),
        serde_json::json!({
            "role": "user",
            "content": [
                {"type": "tool_result", "tool_use_id": "tu_1", "content": "file1.rs\nfile2.rs"},
                {"type": "text", "text": "Do this task"}
            ]
        }),
    ];

    let ctx = build_fork_context(&messages, "Research the codebase");
    assert_eq!(ctx.messages.len(), 2);
    assert!(ctx.use_exact_tools);
    assert_eq!(ctx.directive, "Research the codebase");

    let user_msg = &ctx.messages[1];
    let content = user_msg["content"].as_array().unwrap();
    let tool_result = &content[0];
    assert_eq!(tool_result["content"].as_str().unwrap(), FORK_PLACEHOLDER);

    let text_block = &content[1];
    assert_eq!(text_block["text"].as_str().unwrap(), "Do this task");
}

#[test]
fn test_build_fork_context_preserves_assistant() {
    let messages = vec![serde_json::json!({
        "role": "assistant",
        "content": [
            {"type": "text", "text": "I found something"},
            {"type": "tool_use", "id": "tu_2", "name": "Read", "input": {"path": "foo.rs"}}
        ]
    })];

    let ctx = build_fork_context(&messages, "Continue");
    assert_eq!(ctx.messages.len(), 1);
    let content = ctx.messages[0]["content"].as_array().unwrap();
    assert_eq!(content.len(), 2);
}

#[test]
fn test_build_fork_child_message_has_xml_tags() {
    let msg = build_fork_child_message("Find all TODO comments");
    assert!(msg.contains(&format!("<{FORK_BOILERPLATE_TAG}>")));
    assert!(msg.contains(&format!("</{FORK_BOILERPLATE_TAG}>")));
    assert!(msg.contains(FORK_DIRECTIVE_PREFIX));
    assert!(msg.contains("Find all TODO comments"));
    assert!(msg.contains("Non-negotiable"));
}

#[test]
fn test_build_worktree_notice() {
    let notice = build_worktree_notice("/parent/dir", "/worktree/dir");
    assert!(notice.contains("/parent/dir"));
    assert!(notice.contains("/worktree/dir"));
    assert!(notice.contains("isolated git worktree"));
}

#[test]
fn test_is_in_fork_child_detects_tag() {
    let messages = vec![serde_json::json!({
        "role": "user",
        "content": [
            {"type": "text", "text": format!("<{FORK_BOILERPLATE_TAG}>\nrules\n</{FORK_BOILERPLATE_TAG}>")}
        ]
    })];
    assert!(is_in_fork_child(&messages));
}

#[test]
fn test_is_in_fork_child_no_tag() {
    let messages = vec![serde_json::json!({
        "role": "user",
        "content": [{"type": "text", "text": "normal message"}]
    })];
    assert!(!is_in_fork_child(&messages));
}

#[test]
fn test_is_in_fork_child_assistant_messages_ignored() {
    let messages = vec![serde_json::json!({
        "role": "assistant",
        "content": [{"type": "text", "text": format!("<{FORK_BOILERPLATE_TAG}>rules</{FORK_BOILERPLATE_TAG}>")}]
    })];
    assert!(!is_in_fork_child(&messages));
}

#[test]
fn test_is_fork_allowed_guards() {
    // Fork disabled by default (env var not set)
    assert!(!is_fork_allowed(0, None, &[]));
    assert!(!is_fork_allowed(1, None, &[]));
    assert!(!is_fork_allowed(0, Some("Explore"), &[]));
}

#[test]
fn test_build_fork_context_empty_messages() {
    let ctx = build_fork_context(&[], "directive");
    assert!(ctx.messages.is_empty());
}

#[test]
fn test_build_fork_context_string_content() {
    let messages = vec![serde_json::json!({"role": "user", "content": "plain text"})];
    let ctx = build_fork_context(&messages, "test");
    assert_eq!(ctx.messages.len(), 1);
    assert_eq!(ctx.messages[0]["content"].as_str().unwrap(), "plain text");
}
