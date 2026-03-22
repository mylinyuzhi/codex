use super::*;

#[test]
fn test_attachment_config_default() {
    let config = AttachmentConfig::default();
    assert!(!config.disable_attachments);
    assert!(!config.enable_token_usage_attachment);
}

#[test]
fn test_attachment_config_serde() {
    let json = r#"{"disable_attachments": true, "enable_token_usage_attachment": true}"#;
    let config: AttachmentConfig = serde_json::from_str(json).unwrap();
    assert!(config.disable_attachments);
    assert!(config.enable_token_usage_attachment);
}

#[test]
fn test_attachment_config_serde_defaults() {
    let json = r#"{}"#;
    let config: AttachmentConfig = serde_json::from_str(json).unwrap();
    assert!(!config.disable_attachments);
    assert!(!config.enable_token_usage_attachment);
}

#[test]
fn test_are_attachments_enabled() {
    let mut config = AttachmentConfig::default();
    assert!(config.are_attachments_enabled());

    config.disable_attachments = true;
    assert!(!config.are_attachments_enabled());
}

#[test]
fn test_should_include_token_usage() {
    let mut config = AttachmentConfig::default();
    assert!(!config.should_include_token_usage());

    config.enable_token_usage_attachment = true;
    assert!(config.should_include_token_usage());

    config.disable_attachments = true;
    assert!(!config.should_include_token_usage());
}
