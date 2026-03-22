use super::*;

#[test]
fn test_json_to_request() {
    let json = serde_json::json!({
        "messages": [
            {"role": "user", "content": "Hello!"}
        ],
        "temperature": 0.7,
        "max_tokens": 1000
    });

    let request = json_to_request(&json).unwrap();
    assert_eq!(request.messages.len(), 1);
    assert_eq!(request.temperature, Some(0.7));
    assert_eq!(request.max_tokens, Some(1000));
}

#[test]
fn test_json_to_message_simple() {
    let json = serde_json::json!({
        "role": "user",
        "content": "Hello!"
    });

    let message = json_to_message(&json).unwrap();
    assert_eq!(message.role, Role::User);
    assert_eq!(message.text(), "Hello!");
}

#[test]
fn test_json_to_message_blocks() {
    let json = serde_json::json!({
        "role": "user",
        "content": [
            {"type": "text", "text": "What's in this image?"},
            {"type": "image_url", "image_url": {"url": "https://example.com/image.png"}}
        ]
    });

    let message = json_to_message(&json).unwrap();
    assert_eq!(message.role, Role::User);
    assert_eq!(message.content.len(), 2);
}

#[test]
fn test_response_to_json() {
    let response = GenerateResponse::new("resp_1", "gpt-4o")
        .with_content(vec![ContentBlock::text("Hello!")])
        .with_usage(TokenUsage::new(10, 5));

    let json = response_to_json(&response);
    assert_eq!(json["id"], "resp_1");
    assert_eq!(json["model"], "gpt-4o");
    assert!(json["content"].is_array());
}
