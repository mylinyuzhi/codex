use super::*;

#[test]
fn test_client_config_defaults() {
    let config = ApiClientConfig::default();
    assert!(config.cache.enabled);
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
fn test_builder() {
    let builder = ApiClientBuilder::new()
        .stall_timeout(Duration::from_secs(45))
        .stall_detection(false);

    assert_eq!(builder.config.stall_timeout, Duration::from_secs(45));
    assert!(!builder.config.stall_detection_enabled);
}

#[test]
fn test_builder_with_fallback() {
    let builder = ApiClientBuilder::new().fallback(FallbackConfig::disabled());

    assert!(!builder.config.fallback.enable_stream_fallback);
    assert!(!builder.config.fallback.enable_overflow_recovery);
}

#[test]
fn test_fallback_config_defaults() {
    let config = FallbackConfig::default();
    assert!(config.enable_stream_fallback);
    assert!(config.enable_overflow_recovery);
    assert_eq!(config.fallback_max_tokens, Some(21333));
    assert_eq!(config.min_output_tokens, 3000);
    assert_eq!(config.max_overflow_attempts, 3);
}

#[test]
fn test_fallback_config_disabled() {
    let config = FallbackConfig::disabled();
    assert!(!config.enable_stream_fallback);
    assert!(!config.enable_overflow_recovery);
    assert_eq!(config.fallback_max_tokens, None);
    assert_eq!(config.max_overflow_attempts, 0);
}

#[test]
fn test_fallback_config_builder() {
    let config = FallbackConfig::default()
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
    let config = ApiClientConfig::default().with_fallback(FallbackConfig::disabled());

    assert!(!config.fallback.enable_stream_fallback);
    assert!(!config.fallback.enable_overflow_recovery);
}

#[test]
fn test_from_provider_info() {
    use cocode_protocol::ProviderInfo;
    use cocode_protocol::ProviderType;

    let info = ProviderInfo::new("Test", ProviderType::Openai, "https://api.openai.com/v1")
        .with_api_key("test-key");

    let result = ApiClient::from_provider_info(&info, "gpt-4o", ApiClientConfig::default());
    assert!(result.is_ok());

    let (client, model) = result.unwrap();
    assert_eq!(model.model_name(), "gpt-4o");
    assert_eq!(model.provider(), "openai");
    assert!(client.config().fallback.enable_stream_fallback);
}
