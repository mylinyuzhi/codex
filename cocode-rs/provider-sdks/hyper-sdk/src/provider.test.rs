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

#[test]
fn test_extra_api_keys() {
    let config = ProviderConfig::new("primary-key")
        .with_extra_api_keys(vec!["extra-1".into(), "extra-2".into()]);

    assert_eq!(config.extra_api_keys.len(), 2);
    assert_eq!(config.extra_api_keys[0], "extra-1");
    assert_eq!(config.extra_api_keys[1], "extra-2");
}

#[test]
fn test_all_api_keys() {
    let config = ProviderConfig::new("primary")
        .with_extra_api_keys(vec!["extra-1".into(), "extra-2".into()]);

    let all = config.all_api_keys();
    assert_eq!(all, vec!["primary", "extra-1", "extra-2"]);
}

#[test]
fn test_all_api_keys_no_primary() {
    let config = ProviderConfig::default().with_extra_api_keys(vec!["extra-1".into()]);

    let all = config.all_api_keys();
    assert_eq!(all, vec!["extra-1"]);
}

#[test]
fn test_all_api_keys_no_extras() {
    let config = ProviderConfig::new("primary");
    let all = config.all_api_keys();
    assert_eq!(all, vec!["primary"]);
}

#[test]
fn test_extra_api_keys_default_empty() {
    let config = ProviderConfig::default();
    assert!(config.extra_api_keys.is_empty());
    assert!(config.all_api_keys().is_empty());
}
