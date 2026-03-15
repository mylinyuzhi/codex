use super::*;

fn make_text_block(text: &str) -> ContentBlock {
    ContentBlock::text(text)
}

fn make_tool_use_block(id: &str, name: &str) -> ContentBlock {
    ContentBlock::tool_use(id, name, serde_json::json!({}))
}

fn make_thinking_block(content: &str) -> ContentBlock {
    ContentBlock::Thinking {
        content: content.to_string(),
        signature: None,
    }
}

#[test]
fn test_is_text_block() {
    assert!(is_text_block(&make_text_block("hello")));
    assert!(!is_text_block(&make_tool_use_block("id", "name")));
}

#[test]
fn test_is_tool_use_block() {
    assert!(is_tool_use_block(&make_tool_use_block("id", "name")));
    assert!(!is_tool_use_block(&make_text_block("hello")));
}

#[test]
fn test_is_thinking_block() {
    assert!(is_thinking_block(&make_thinking_block("thinking...")));
    assert!(!is_thinking_block(&make_text_block("hello")));
}

#[test]
fn test_extract_text() {
    assert_eq!(extract_text(&make_text_block("hello")), Some("hello"));
    assert_eq!(extract_text(&make_tool_use_block("id", "name")), None);
}

#[test]
fn test_extract_tool_use() {
    let block = make_tool_use_block("call_1", "get_weather");
    let (id, name, _input) = extract_tool_use(&block).unwrap();
    assert_eq!(id, "call_1");
    assert_eq!(name, "get_weather");
}

#[test]
fn test_has_tool_use() {
    let msg_with_tool = Message::new(
        Role::Assistant,
        vec![
            make_text_block("Let me help"),
            make_tool_use_block("call_1", "get_weather"),
        ],
    );
    assert!(has_tool_use(&msg_with_tool));

    let msg_without_tool = Message::assistant("Just text");
    assert!(!has_tool_use(&msg_without_tool));
}

#[test]
fn test_get_text_content() {
    let msg = Message::new(
        Role::Assistant,
        vec![
            make_text_block("Hello "),
            make_tool_use_block("call_1", "test"),
            make_text_block("world"),
        ],
    );
    assert_eq!(get_text_content(&msg), "Hello world");
}

#[test]
fn test_get_tool_calls() {
    let msg = Message::new(
        Role::Assistant,
        vec![
            make_text_block("Let me check"),
            make_tool_use_block("call_1", "get_weather"),
            make_tool_use_block("call_2", "get_time"),
        ],
    );
    let calls = get_tool_calls(&msg);
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].name, "get_weather");
    assert_eq!(calls[1].name, "get_time");
}

#[test]
fn test_message_role_checks() {
    assert!(is_user_message(&Message::user("hello")));
    assert!(is_assistant_message(&Message::assistant("hi")));
    assert!(is_system_message(&Message::system("instructions")));
}

#[test]
fn test_count_tool_uses() {
    let msg = Message::new(
        Role::Assistant,
        vec![
            make_tool_use_block("call_1", "tool1"),
            make_text_block("text"),
            make_tool_use_block("call_2", "tool2"),
        ],
    );
    assert_eq!(count_tool_uses(&msg), 2);
}
