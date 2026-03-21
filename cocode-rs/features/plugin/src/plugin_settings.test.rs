use super::*;

#[test]
fn test_default_settings() {
    let settings = PluginSettings::default();
    assert!(settings.enabled_plugins.is_empty());
}

#[test]
fn test_is_enabled_default_true() {
    let settings = PluginSettings::default();
    assert!(settings.is_enabled("any-plugin"));
}

#[test]
fn test_set_enabled() {
    let mut settings = PluginSettings::default();
    settings.set_enabled("my-plugin", false);
    assert!(!settings.is_enabled("my-plugin"));

    settings.set_enabled("my-plugin", true);
    assert!(settings.is_enabled("my-plugin"));
}

#[test]
fn test_remove() {
    let mut settings = PluginSettings::default();
    settings.set_enabled("my-plugin", false);
    assert!(!settings.is_enabled("my-plugin"));

    settings.remove("my-plugin");
    // After removal, defaults back to true
    assert!(settings.is_enabled("my-plugin"));
}

#[test]
fn test_save_and_load_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");

    let mut settings = PluginSettings::default();
    settings.set_enabled("plugin-a", true);
    settings.set_enabled("plugin-b", false);
    settings.save(&path).unwrap();

    let loaded = PluginSettings::load(&path);
    assert!(loaded.is_enabled("plugin-a"));
    assert!(!loaded.is_enabled("plugin-b"));
}

#[test]
fn test_load_missing_file() {
    let settings = PluginSettings::load(Path::new("/nonexistent/settings.json"));
    assert!(settings.enabled_plugins.is_empty());
}

#[test]
fn test_load_corrupt_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");
    std::fs::write(&path, "invalid json").unwrap();

    let settings = PluginSettings::load(&path);
    assert!(settings.enabled_plugins.is_empty());
}

#[test]
fn test_get_set_config() {
    let mut settings = PluginSettings::default();

    // No config initially
    assert!(settings.get_config("my-plugin", "api_key").is_none());

    // Set config
    settings.set_config(
        "my-plugin",
        "api_key",
        serde_json::Value::String("sk-123".to_string()),
    );
    settings.set_config(
        "my-plugin",
        "region",
        serde_json::Value::String("us-east-1".to_string()),
    );

    assert_eq!(
        settings.get_config("my-plugin", "api_key"),
        Some(&serde_json::Value::String("sk-123".to_string()))
    );
    assert_eq!(
        settings.get_config("my-plugin", "region"),
        Some(&serde_json::Value::String("us-east-1".to_string()))
    );

    // Different plugin
    assert!(settings.get_config("other-plugin", "api_key").is_none());
}

#[test]
fn test_get_plugin_config() {
    let mut settings = PluginSettings::default();
    settings.set_config("my-plugin", "key1", serde_json::json!("value1"));
    settings.set_config("my-plugin", "key2", serde_json::json!(42));

    let config = settings.get_plugin_config("my-plugin").unwrap();
    assert_eq!(config.len(), 2);
    assert_eq!(config.get("key1"), Some(&serde_json::json!("value1")));
    assert_eq!(config.get("key2"), Some(&serde_json::json!(42)));

    assert!(settings.get_plugin_config("missing").is_none());
}

#[test]
fn test_remove_clears_config() {
    let mut settings = PluginSettings::default();
    settings.set_enabled("my-plugin", true);
    settings.set_config("my-plugin", "key", serde_json::json!("val"));

    settings.remove("my-plugin");

    assert!(settings.is_enabled("my-plugin")); // defaults to true
    assert!(settings.get_config("my-plugin", "key").is_none());
    assert!(settings.get_plugin_config("my-plugin").is_none());
}

#[test]
fn test_config_save_load_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");

    let mut settings = PluginSettings::default();
    settings.set_config("my-plugin", "api_key", serde_json::json!("secret"));
    settings.set_config("my-plugin", "debug", serde_json::json!(true));
    settings.save(&path).unwrap();

    let loaded = PluginSettings::load(&path);
    assert_eq!(
        loaded.get_config("my-plugin", "api_key"),
        Some(&serde_json::json!("secret"))
    );
    assert_eq!(
        loaded.get_config("my-plugin", "debug"),
        Some(&serde_json::json!(true))
    );
}

#[test]
fn test_serde_roundtrip() {
    let mut settings = PluginSettings::default();
    settings.set_enabled("hello@market", true);
    settings.set_enabled("world@market", false);

    let json = serde_json::to_string(&settings).unwrap();
    let deserialized: PluginSettings = serde_json::from_str(&json).unwrap();

    assert!(deserialized.is_enabled("hello@market"));
    assert!(!deserialized.is_enabled("world@market"));
}
