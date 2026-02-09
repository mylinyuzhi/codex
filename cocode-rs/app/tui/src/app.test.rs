use super::*;

#[test]
fn test_app_config_default() {
    let config = AppConfig::default();
    assert!(!config.model.is_empty());
    assert!(!config.available_models.is_empty());
}

#[test]
fn test_has_line_range_suffix() {
    // Should detect line range suffixes
    assert!(has_line_range_suffix("file.rs:10"));
    assert!(has_line_range_suffix("file.rs:10-20"));
    assert!(has_line_range_suffix("src/main.rs:1"));
    assert!(has_line_range_suffix("src/main.rs:100-200"));

    // Should NOT detect non-line-range patterns
    assert!(!has_line_range_suffix("file.rs"));
    assert!(!has_line_range_suffix("file.rs:"));
    assert!(!has_line_range_suffix("file.rs:abc"));
    assert!(!has_line_range_suffix("file.rs:10-"));
    assert!(!has_line_range_suffix("file.rs:-20"));
    assert!(!has_line_range_suffix("file:name.rs"));
}

#[test]
fn test_create_channels() {
    let (agent_tx, _agent_rx, command_tx, _command_rx) = create_channels(16);

    // Channels should be usable
    assert!(agent_tx.try_send(LoopEvent::StreamRequestStart).is_ok());
    assert!(
        command_tx
            .try_send(UserCommand::SubmitInput {
                content: vec![hyper_sdk::ContentBlock::text("test")],
                display_text: "test".to_string()
            })
            .is_ok()
    );
}
