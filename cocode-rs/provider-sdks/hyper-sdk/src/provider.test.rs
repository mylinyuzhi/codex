use super::*;

#[test]
fn test_provider_config_builder() {
    let config = ProviderConfig::new("sk-test-key")
        .with_base_url("https://api.example.com")
        .with_timeout(30);

    assert_eq!(config.api_key, Some("sk-test-key".to_string()));
    assert_eq!(config.base_url, Some("https://api.example.com".to_string()));
    assert_eq!(config.timeout_secs, Some(30));
}

#[test]
fn test_require_api_key() {
    let config = ProviderConfig::new("sk-test");
    assert!(config.require_api_key().is_ok());

    let config = ProviderConfig::default();
    assert!(config.require_api_key().is_err());
}
