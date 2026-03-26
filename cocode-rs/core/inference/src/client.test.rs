use pretty_assertions::assert_eq;

use super::*;

#[test]
fn test_client_config_defaults() {
    let config = ApiClientConfig::default();
    assert!(config.stall_detection_enabled);
    assert_eq!(config.stall_timeout, Duration::from_secs(30));
}

#[test]
fn test_client_config_builder() {
    let config = ApiClientConfig::default()
        .with_stall_timeout(Duration::from_secs(60))
        .with_stall_detection(false);

    assert_eq!(config.stall_timeout, Duration::from_secs(60));
    assert!(!config.stall_detection_enabled);
}

#[test]
fn test_stream_options() {
    let opts = StreamOptions::streaming();
    assert!(opts.streaming);

    let opts = StreamOptions::non_streaming();
    assert!(!opts.streaming);
}

#[test]
fn test_config_with_stall_settings() {
    let config = ApiClientConfig::default()
        .with_stall_timeout(Duration::from_secs(45))
        .with_stall_detection(false);

    assert_eq!(config.stall_timeout, Duration::from_secs(45));
    assert!(!config.stall_detection_enabled);
}

#[test]
fn test_config_with_fallback_disabled() {
    let config = ApiClientConfig::default().with_fallback(ApiFallbackConfig::disabled());

    assert!(!config.fallback.enable_stream_fallback);
    assert!(!config.fallback.enable_overflow_recovery);
}

#[test]
fn test_fallback_config_defaults() {
    assert_eq!(
        ApiFallbackConfig::default(),
        ApiFallbackConfig {
            enable_stream_fallback: true,
            enable_overflow_recovery: true,
            fallback_max_tokens: Some(21333),
            min_output_tokens: 3000,
            max_overflow_attempts: 3,
            floor_output_tokens: 3000,
            buffer_tokens: 1000,
        }
    );
}

#[test]
fn test_fallback_config_disabled() {
    let config = ApiFallbackConfig::disabled();
    assert!(!config.enable_stream_fallback);
    assert!(!config.enable_overflow_recovery);
    assert_eq!(config.fallback_max_tokens, None);
    assert_eq!(config.max_overflow_attempts, 0);
}

#[test]
fn test_fallback_config_builder() {
    let config = ApiFallbackConfig::default()
        .with_stream_fallback(false)
        .with_fallback_max_tokens(Some(10000))
        .with_overflow_recovery(false)
        .with_min_output_tokens(1000)
        .with_max_overflow_attempts(5);

    assert!(!config.enable_stream_fallback);
    assert_eq!(config.fallback_max_tokens, Some(10000));
    assert!(!config.enable_overflow_recovery);
    assert_eq!(config.min_output_tokens, 1000);
    assert_eq!(config.max_overflow_attempts, 5);
}

#[test]
fn test_api_client_config_with_fallback() {
    let config = ApiClientConfig::default().with_fallback(ApiFallbackConfig::disabled());

    assert!(!config.fallback.enable_stream_fallback);
    assert!(!config.fallback.enable_overflow_recovery);
}

#[test]
fn test_from_provider_info() {
    use cocode_protocol::ProviderApi;
    use cocode_protocol::ProviderInfo;

    let info = ProviderInfo::new("Test", ProviderApi::Openai, "https://api.openai.com/v1")
        .with_api_key("test-key");

    let result = ApiClient::from_provider_info(&info, "gpt-4o", ApiClientConfig::default());
    assert!(result.is_ok());

    let (client, model) = result.unwrap();
    assert_eq!(model.model_id(), "gpt-4o");
    assert_eq!(model.provider(), "openai.responses");
    assert!(client.config().fallback.enable_stream_fallback);
}

// =========================================================================
// M3: Provider-level failover tests
// =========================================================================

#[tokio::test]
async fn test_stream_request_with_fallback_empty_models() {
    let client = ApiClient::new();
    let request =
        crate::LanguageModelCallOptions::new(vec![crate::LanguageModelMessage::user_text("test")]);
    let result = client
        .stream_request_with_fallback(&[], request, StreamOptions::streaming())
        .await;
    assert!(result.is_err());
}

// =========================================================================
// Overflow recovery tests
// =========================================================================

#[test]
fn test_overflow_recovery_smart_with_parsed_info() {
    let client = ApiClient::new();
    let request = crate::LanguageModelCallOptions::new(vec![]);
    // Anthropic pattern: input=80000, max=8192, limit=200000
    let error: crate::error::ApiError = crate::error::api_error::ContextOverflowSnafu {
        message: "context limit: 80000 + 8192 > 200000",
    }
    .build();
    let result = client.try_overflow_recovery(&request, &error);
    // available = 200000 - 80000 - 1000(buffer) = 119000
    assert_eq!(result, Some(119000));
}

#[test]
fn test_overflow_recovery_blind_fallback() {
    let client = ApiClient::new();
    let mut request = crate::LanguageModelCallOptions::new(vec![]);
    request.max_output_tokens = Some(8192);
    // Unparseable error message → blind 3/4 reduction
    let error: crate::error::ApiError = crate::error::api_error::ContextOverflowSnafu {
        message: "context length exceeded",
    }
    .build();
    let result = client.try_overflow_recovery(&request, &error);
    // 8192 * 3/4 = 6144
    assert_eq!(result, Some(6144));
}

#[test]
fn test_overflow_recovery_below_min_returns_none() {
    let mut config = ApiClientConfig::default();
    config.fallback.min_output_tokens = 5000;
    let client = ApiClient::with_config(config);
    let mut request = crate::LanguageModelCallOptions::new(vec![]);
    request.max_output_tokens = Some(4000);
    let error: crate::error::ApiError = crate::error::api_error::ContextOverflowSnafu {
        message: "context length exceeded",
    }
    .build();
    let result = client.try_overflow_recovery(&request, &error);
    // 4000 * 3/4 = 3000, but min_output_tokens = 5000 → None
    assert!(result.is_none());
}

#[test]
fn test_overflow_recovery_floor_enforced() {
    let mut config = ApiClientConfig::default();
    config.fallback.min_output_tokens = 4000;
    let client = ApiClient::with_config(config);
    let request = crate::LanguageModelCallOptions::new(vec![]);
    // Very tight space: input=199000, limit=200000
    let error: crate::error::ApiError = crate::error::api_error::ContextOverflowSnafu {
        message: "context limit: 199000 + 4096 > 200000",
    }
    .build();
    let result = client.try_overflow_recovery(&request, &error);
    // available = 200000 - 199000 - 1000 = 0, floor = 3000
    // max(0, 3000) = 3000, but 3000 < min_output_tokens(4000) → None
    assert!(result.is_none());
}

#[test]
fn test_extract_thinking_budget_none() {
    let request = crate::LanguageModelCallOptions::new(vec![]);
    assert!(extract_thinking_budget(&request).is_none());
}

#[test]
fn test_extract_thinking_budget_anthropic() {
    use serde_json::json;
    use std::collections::HashMap;
    let mut opts = HashMap::new();
    let mut anthropic_opts = HashMap::new();
    anthropic_opts.insert(
        "thinking".to_string(),
        json!({"type": "enabled", "budgetTokens": 32000}),
    );
    opts.insert("anthropic".to_string(), anthropic_opts);
    let provider_opts = crate::ProviderOptions::from_map(opts);

    let mut request = crate::LanguageModelCallOptions::new(vec![]);
    request.provider_options = Some(provider_opts);
    assert_eq!(extract_thinking_budget(&request), Some(32000));
}

// =========================================================================
// Overflow recovery edge cases
// =========================================================================

#[test]
fn test_overflow_recovery_thinking_budget_larger_than_available() {
    use serde_json::json;
    use std::collections::HashMap;

    let client = ApiClient::new();
    let mut request = crate::LanguageModelCallOptions::new(vec![]);

    // Set a large thinking budget
    let mut opts = HashMap::new();
    let mut anthropic_opts = HashMap::new();
    anthropic_opts.insert(
        "thinking".to_string(),
        json!({"type": "enabled", "budgetTokens": 50000}),
    );
    opts.insert("anthropic".to_string(), anthropic_opts);
    request.provider_options = Some(crate::ProviderOptions::from_map(opts));

    // available = 200000 - 140000 - 1000 = 59000
    // thinking budget = 50000, so new_max = max(59000, 50001) = 59000
    let error: crate::error::ApiError = crate::error::api_error::ContextOverflowSnafu {
        message: "context limit: 140000 + 8192 > 200000",
    }
    .build();
    let result = client.try_overflow_recovery(&request, &error);
    assert_eq!(result, Some(59000));
}

#[test]
fn test_overflow_recovery_thinking_budget_dominates() {
    use serde_json::json;
    use std::collections::HashMap;

    let client = ApiClient::new();
    let mut request = crate::LanguageModelCallOptions::new(vec![]);

    // Set thinking budget larger than available space
    let mut opts = HashMap::new();
    let mut anthropic_opts = HashMap::new();
    anthropic_opts.insert(
        "thinking".to_string(),
        json!({"type": "enabled", "budgetTokens": 10000}),
    );
    opts.insert("anthropic".to_string(), anthropic_opts);
    request.provider_options = Some(crate::ProviderOptions::from_map(opts));

    // available = 200000 - 195000 - 1000 = 4000, floor = 3000
    // max(4000, 3000) = 4000, then max(4000, 10001) = 10001
    // 10001 >= min_output_tokens(3000) → Some(10001)
    let error: crate::error::ApiError = crate::error::api_error::ContextOverflowSnafu {
        message: "context limit: 195000 + 8192 > 200000",
    }
    .build();
    let result = client.try_overflow_recovery(&request, &error);
    assert_eq!(result, Some(10001));
}

#[test]
fn test_overflow_recovery_negative_available_space() {
    let client = ApiClient::new();
    let request = crate::LanguageModelCallOptions::new(vec![]);

    // Input exceeds context limit: available = 200000 - 210000 - 1000 = -11000
    // max(-11000, 3000) = 3000, 3000 >= min_output_tokens(3000) → Some(3000)
    let error: crate::error::ApiError = crate::error::api_error::ContextOverflowSnafu {
        message: "context limit: 210000 + 4096 > 200000",
    }
    .build();
    let result = client.try_overflow_recovery(&request, &error);
    assert_eq!(result, Some(3000));
}

#[test]
fn test_overflow_recovery_blind_between_floor_and_min() {
    let mut config = ApiClientConfig::default();
    config.fallback.min_output_tokens = 5000;
    let client = ApiClient::with_config(config);
    let mut request = crate::LanguageModelCallOptions::new(vec![]);
    request.max_output_tokens = Some(5500);

    // 5500 * 3/4 = 4125, floor = 3000, max(4125, 3000) = 4125
    // 4125 < min_output_tokens(5000) → None
    let error: crate::error::ApiError = crate::error::api_error::ContextOverflowSnafu {
        message: "context length exceeded",
    }
    .build();
    let result = client.try_overflow_recovery(&request, &error);
    assert!(result.is_none());
}
