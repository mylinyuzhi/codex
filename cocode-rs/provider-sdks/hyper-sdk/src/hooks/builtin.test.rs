use super::*;
use crate::messages::ContentBlock;
use crate::messages::Message;
use crate::response::FinishReason;

#[tokio::test]
async fn test_response_id_hook_openai() {
    let hook = ResponseIdHook::new();
    let mut request = GenerateRequest::new(vec![Message::user("Hello")]);
    let mut context =
        HookContext::with_provider("openai", "gpt-4o").previous_response_id("resp_prev_123");

    hook.on_request(&mut request, &mut context).await.unwrap();

    // Check that provider options were set
    let options = request
        .provider_options
        .as_ref()
        .and_then(|opts| downcast_options::<OpenAIOptions>(opts))
        .unwrap();
    assert_eq!(
        options.previous_response_id,
        Some("resp_prev_123".to_string())
    );
}

#[tokio::test]
async fn test_response_id_hook_other_provider() {
    let hook = ResponseIdHook::new();
    let mut request = GenerateRequest::new(vec![Message::user("Hello")]);
    let mut context = HookContext::with_provider("anthropic", "claude-3")
        .previous_response_id("resp_prev_123");

    hook.on_request(&mut request, &mut context).await.unwrap();

    // For non-supported providers, options should not be set
    assert!(request.provider_options.is_none());
}

#[tokio::test]
async fn test_response_id_hook_volcengine() {
    let hook = ResponseIdHook::new();
    let mut request = GenerateRequest::new(vec![Message::user("Hello")]);
    let mut context = HookContext::with_provider("volcengine", "doubao-pro-32k")
        .previous_response_id("resp_prev_456");

    hook.on_request(&mut request, &mut context).await.unwrap();

    // Check that provider options were set
    let options = request
        .provider_options
        .as_ref()
        .and_then(|opts| downcast_options::<VolcengineOptions>(opts))
        .unwrap();
    assert_eq!(
        options.previous_response_id,
        Some("resp_prev_456".to_string())
    );
}

#[tokio::test]
async fn test_usage_tracking_hook() {
    let hook = UsageTrackingHook::new();

    // First response
    let mut response1 = GenerateResponse::new("resp_1", "gpt-4o")
        .with_content(vec![ContentBlock::text("Hello")])
        .with_usage(TokenUsage::new(100, 50))
        .with_finish_reason(FinishReason::Stop);

    let context = HookContext::with_provider("openai", "gpt-4o");
    hook.on_response(&mut response1, &context).await.unwrap();

    // Second response
    let mut response2 = GenerateResponse::new("resp_2", "gpt-4o")
        .with_content(vec![ContentBlock::text("World")])
        .with_usage(TokenUsage::new(80, 40))
        .with_finish_reason(FinishReason::Stop);

    hook.on_response(&mut response2, &context).await.unwrap();

    // Check accumulated usage
    let usage = hook.get_usage();
    assert_eq!(usage.prompt_tokens, 180);
    assert_eq!(usage.completion_tokens, 90);
    assert_eq!(usage.total_tokens, 270);

    // Test reset
    hook.reset();
    let usage = hook.get_usage();
    assert_eq!(usage.total_tokens, 0);
}

#[test]
fn test_usage_tracking_shared_counter() {
    let shared = Arc::new(Mutex::new(TokenUsage::default()));
    let hook1 = UsageTrackingHook::with_shared_usage(shared.clone());
    let hook2 = UsageTrackingHook::with_shared_usage(shared);

    // Modifying through one hook affects the other
    {
        let mut usage = hook1.usage.lock().unwrap();
        usage.prompt_tokens = 100;
    }

    assert_eq!(hook2.get_usage().prompt_tokens, 100);
}

// ============================================================
// CrossProviderSanitizationHook Tests
// ============================================================

#[tokio::test]
async fn test_cross_provider_sanitization_hook_strips_signatures() {
    let hook = CrossProviderSanitizationHook::new();

    // Create request with messages from different providers
    let anthropic_msg = Message::new(
        crate::messages::Role::Assistant,
        vec![
            ContentBlock::Thinking {
                content: "Thinking content".to_string(),
                signature: Some("anthropic-signature".to_string()),
            },
            ContentBlock::text("Response"),
        ],
    )
    .with_source("anthropic", "claude-sonnet-4-20250514");

    let mut request = GenerateRequest::new(vec![
        Message::user("Hello"),
        anthropic_msg,
        Message::user("Follow up"),
    ]);

    // Target provider is OpenAI
    let mut context = HookContext::with_provider("openai", "gpt-4o");

    hook.on_request(&mut request, &mut context).await.unwrap();

    // Anthropic signature should be stripped
    if let ContentBlock::Thinking { signature, .. } = &request.messages[1].content[0] {
        assert!(
            signature.is_none(),
            "Signature should be stripped when switching to different provider"
        );
    } else {
        panic!("Expected Thinking block");
    }
}

#[tokio::test]
async fn test_cross_provider_sanitization_hook_preserves_same_provider() {
    let hook = CrossProviderSanitizationHook::new();

    // Create request with messages from the same provider
    let anthropic_msg = Message::new(
        crate::messages::Role::Assistant,
        vec![ContentBlock::Thinking {
            content: "Thinking content".to_string(),
            signature: Some("anthropic-signature".to_string()),
        }],
    )
    .with_source("anthropic", "claude-sonnet-4-20250514");

    let mut request = GenerateRequest::new(vec![Message::user("Hello"), anthropic_msg]);

    // Target provider is the same (Anthropic) - hook should NOT modify
    // Note: Same-provider, different-model sanitization is handled by Message::sanitize_for_target
    let mut context = HookContext::with_provider("anthropic", "claude-opus-4-20250514");

    hook.on_request(&mut request, &mut context).await.unwrap();

    // Signature should be preserved - hook only handles cross-PROVIDER sanitization
    if let ContentBlock::Thinking { signature, .. } = &request.messages[1].content[0] {
        assert!(
            signature.is_some(),
            "Signature should be preserved for same provider (cross-model sanitization is separate)"
        );
    }
}

#[tokio::test]
async fn test_cross_provider_sanitization_hook_skips_no_source() {
    let hook = CrossProviderSanitizationHook::new();

    // Message without source info (e.g., user messages)
    let mut request = GenerateRequest::new(vec![Message::user("Hello")]);

    let mut context = HookContext::with_provider("openai", "gpt-4o");

    // Should not panic or modify user messages
    hook.on_request(&mut request, &mut context).await.unwrap();

    assert_eq!(request.messages.len(), 1);
    assert_eq!(request.messages[0].text(), "Hello");
}

#[test]
fn test_cross_provider_sanitization_hook_priority() {
    let hook = CrossProviderSanitizationHook::new();
    assert_eq!(hook.priority(), 5);
    assert_eq!(hook.name(), "cross_provider_sanitization");
}
