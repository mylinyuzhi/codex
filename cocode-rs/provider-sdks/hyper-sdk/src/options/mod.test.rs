use super::*;

#[test]
fn test_try_downcast_options_success() {
    let opts: ProviderOptions = OpenAIOptions::new().boxed();
    let result = try_downcast_options::<OpenAIOptions>(&opts);
    assert!(result.is_ok());
}

#[test]
fn test_try_downcast_options_failure() {
    let opts: ProviderOptions = OpenAIOptions::new().boxed();
    let result = try_downcast_options::<AnthropicOptions>(&opts);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, HyperError::ConfigError(_)));
    // Verify error message contains type name
    let msg = err.to_string();
    assert!(msg.contains("type mismatch"));
    assert!(msg.contains("AnthropicOptions"));
}

#[test]
fn test_downcast_options_success() {
    let opts: ProviderOptions = AnthropicOptions::new().boxed();
    let result = downcast_options::<AnthropicOptions>(&opts);
    assert!(result.is_some());
}

#[test]
fn test_downcast_options_failure() {
    let opts: ProviderOptions = AnthropicOptions::new().boxed();
    let result = downcast_options::<OpenAIOptions>(&opts);
    assert!(result.is_none());
}

// ============================================================
// ProviderMarker Tests
// ============================================================

#[test]
fn test_provider_marker_openai() {
    assert_eq!(OpenAIOptions::PROVIDER_NAME, "openai");
}

#[test]
fn test_provider_marker_anthropic() {
    assert_eq!(AnthropicOptions::PROVIDER_NAME, "anthropic");
}

#[test]
fn test_provider_marker_gemini() {
    assert_eq!(GeminiOptions::PROVIDER_NAME, "gemini");
}

#[test]
fn test_provider_marker_volcengine() {
    assert_eq!(VolcengineOptions::PROVIDER_NAME, "volcengine");
}

#[test]
fn test_provider_marker_zai() {
    assert_eq!(ZaiOptions::PROVIDER_NAME, "zhipuai");
}

#[test]
fn test_provider_name_method() {
    let openai_opts: ProviderOptions = OpenAIOptions::new().boxed();
    assert_eq!(openai_opts.provider_name(), Some("openai"));

    let anthropic_opts: ProviderOptions = AnthropicOptions::new().boxed();
    assert_eq!(anthropic_opts.provider_name(), Some("anthropic"));
}

// ============================================================
// validate_options_for_provider Tests
// ============================================================

#[test]
fn test_validate_options_none() {
    // No options is always valid
    assert!(validate_options_for_provider(None, "openai").unwrap());
    assert!(validate_options_for_provider(None, "anthropic").unwrap());
}

#[test]
fn test_validate_options_correct_provider() {
    let openai_opts: ProviderOptions = OpenAIOptions::new().boxed();
    assert!(validate_options_for_provider(Some(&openai_opts), "openai").unwrap());

    let anthropic_opts: ProviderOptions = AnthropicOptions::new().boxed();
    assert!(validate_options_for_provider(Some(&anthropic_opts), "anthropic").unwrap());

    let gemini_opts: ProviderOptions = GeminiOptions::new().boxed();
    assert!(validate_options_for_provider(Some(&gemini_opts), "gemini").unwrap());

    let volcengine_opts: ProviderOptions = VolcengineOptions::new().boxed();
    assert!(validate_options_for_provider(Some(&volcengine_opts), "volcengine").unwrap());

    let zai_opts: ProviderOptions = ZaiOptions::new().boxed();
    assert!(validate_options_for_provider(Some(&zai_opts), "zhipuai").unwrap());
}

#[test]
fn test_validate_options_wrong_provider() {
    // OpenAI options with Anthropic provider
    let openai_opts: ProviderOptions = OpenAIOptions::new().boxed();
    assert!(!validate_options_for_provider(Some(&openai_opts), "anthropic").unwrap());

    // Anthropic options with OpenAI provider
    let anthropic_opts: ProviderOptions = AnthropicOptions::new().boxed();
    assert!(!validate_options_for_provider(Some(&anthropic_opts), "openai").unwrap());

    // Gemini options with Volcengine provider
    let gemini_opts: ProviderOptions = GeminiOptions::new().boxed();
    assert!(!validate_options_for_provider(Some(&gemini_opts), "volcengine").unwrap());
}
