use super::*;
use crate::tracked::MessageSource;

fn make_tracked_user(content: &str, turn_id: &str) -> TrackedMessage {
    TrackedMessage::new(
        LanguageModelMessage::user_text(content),
        turn_id,
        MessageSource::User,
    )
}

fn make_tracked_assistant(content: &str, turn_id: &str) -> TrackedMessage {
    TrackedMessage::new(
        LanguageModelMessage::assistant_text(content),
        turn_id,
        MessageSource::assistant(None),
    )
}

#[test]
fn test_basic_normalization() {
    let messages = vec![
        make_tracked_user("Hello", "turn-1"),
        make_tracked_assistant("Hi there!", "turn-1"),
    ];

    let normalized = normalize_messages_for_api(&messages, &NormalizationOptions::for_api());
    assert_eq!(normalized.len(), 2);
}

#[test]
fn test_skip_tombstoned() {
    let mut messages = vec![
        make_tracked_user("Hello", "turn-1"),
        make_tracked_assistant("Hi there!", "turn-1"),
    ];
    messages[1].tombstone();

    let options = NormalizationOptions::for_api();
    let normalized = normalize_messages_for_api(&messages, &options);
    assert_eq!(normalized.len(), 1);
}

#[test]
fn test_merge_consecutive() {
    let messages = vec![
        make_tracked_user("Hello", "turn-1"),
        make_tracked_user(" world", "turn-1"),
    ];

    let options = NormalizationOptions {
        merge_consecutive: true,
        ..Default::default()
    };
    let normalized = normalize_messages_for_api(&messages, &options);
    assert_eq!(normalized.len(), 1);
    if let LanguageModelMessage::User { content, .. } = &normalized[0] {
        assert_eq!(content.len(), 2);
    } else {
        panic!("Expected user message");
    }
}

#[test]
fn test_strip_thinking_signatures() {
    let mut tracked = make_tracked_assistant("", "turn-1");
    tracked.inner = LanguageModelMessage::assistant(vec![AssistantContentPart::Reasoning(
        ReasoningPart::new("Let me think...")
            .with_metadata(cocode_inference::ProviderMetadata::new()),
    )]);

    let options = NormalizationOptions {
        strip_thinking_signatures: true,
        ..Default::default()
    };
    let normalized = normalize_messages_for_api(&[tracked], &options);

    if let LanguageModelMessage::Assistant { content, .. } = &normalized[0] {
        if let AssistantContentPart::Reasoning(rp) = &content[0] {
            assert!(rp.provider_metadata.is_none());
        } else {
            panic!("Expected reasoning block");
        }
    } else {
        panic!("Expected assistant message");
    }
}

#[test]
fn test_validation_empty() {
    let result = validate_messages(&[]);
    assert!(matches!(result, Err(ValidationError::EmptyMessages)));
}

#[test]
fn test_validation_system_not_first() {
    let messages = vec![
        LanguageModelMessage::user_text("Hello"),
        LanguageModelMessage::system("Instructions"),
    ];

    let result = validate_messages(&messages);
    assert!(matches!(
        result,
        Err(ValidationError::SystemNotFirst { .. })
    ));
}

#[test]
fn test_estimate_tokens() {
    let messages = vec![
        LanguageModelMessage::user_text("Hello world"), // ~3 tokens
        LanguageModelMessage::assistant_text("Hi there!"), // ~2 tokens
    ];

    let tokens = estimate_tokens(&messages);
    assert!(tokens > 0);
}
