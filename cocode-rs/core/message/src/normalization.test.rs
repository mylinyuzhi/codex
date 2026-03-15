use super::*;
use crate::tracked::MessageSource;

fn make_tracked(role: Role, content: &str, turn_id: &str) -> TrackedMessage {
    TrackedMessage::new(
        match role {
            Role::User => Message::user(content),
            Role::Assistant => Message::assistant(content),
            Role::System => Message::system(content),
            Role::Tool => panic!("Use specific tool message constructors"),
        },
        turn_id,
        match role {
            Role::User => MessageSource::User,
            Role::Assistant => MessageSource::assistant(None),
            Role::System => MessageSource::System,
            Role::Tool => panic!("Use specific tool message constructors"),
        },
    )
}

#[test]
fn test_basic_normalization() {
    let messages = vec![
        make_tracked(Role::User, "Hello", "turn-1"),
        make_tracked(Role::Assistant, "Hi there!", "turn-1"),
    ];

    let normalized = normalize_messages_for_api(&messages, &NormalizationOptions::for_api());
    assert_eq!(normalized.len(), 2);
}

#[test]
fn test_skip_tombstoned() {
    let mut messages = vec![
        make_tracked(Role::User, "Hello", "turn-1"),
        make_tracked(Role::Assistant, "Hi there!", "turn-1"),
    ];
    messages[1].tombstone();

    let options = NormalizationOptions::for_api();
    let normalized = normalize_messages_for_api(&messages, &options);
    assert_eq!(normalized.len(), 1);
}

#[test]
fn test_merge_consecutive() {
    let messages = vec![
        make_tracked(Role::User, "Hello", "turn-1"),
        make_tracked(Role::User, " world", "turn-1"),
    ];

    let options = NormalizationOptions {
        merge_consecutive: true,
        ..Default::default()
    };
    let normalized = normalize_messages_for_api(&messages, &options);
    assert_eq!(normalized.len(), 1);
    assert_eq!(normalized[0].content.len(), 2);
}

#[test]
fn test_strip_thinking_signatures() {
    let mut tracked = make_tracked(Role::Assistant, "", "turn-1");
    tracked.inner.content = vec![ContentBlock::Thinking {
        content: "Let me think...".to_string(),
        signature: Some("sig123".to_string()),
    }];

    let options = NormalizationOptions {
        strip_thinking_signatures: true,
        ..Default::default()
    };
    let normalized = normalize_messages_for_api(&[tracked], &options);

    if let ContentBlock::Thinking { signature, .. } = &normalized[0].content[0] {
        assert!(signature.is_none());
    } else {
        panic!("Expected thinking block");
    }
}

#[test]
fn test_validation_empty() {
    let result = validate_messages(&[]);
    assert!(matches!(result, Err(ValidationError::EmptyMessages)));
}

#[test]
fn test_validation_system_not_first() {
    let messages = vec![Message::user("Hello"), Message::system("Instructions")];

    let result = validate_messages(&messages);
    assert!(matches!(
        result,
        Err(ValidationError::SystemNotFirst { .. })
    ));
}

#[test]
fn test_estimate_tokens() {
    let messages = vec![
        Message::user("Hello world"),    // ~3 tokens
        Message::assistant("Hi there!"), // ~2 tokens
    ];

    let tokens = estimate_tokens(&messages);
    assert!(tokens > 0);
}
