use super::*;

#[test]
fn test_default() {
    assert_eq!(
        ConfigShellToolType::default(),
        ConfigShellToolType::ShellCommand
    );
}

#[test]
fn test_serde() {
    let shell_type = ConfigShellToolType::Shell;
    let json = serde_json::to_string(&shell_type).expect("serialize");
    assert_eq!(json, "\"shell\"");

    let parsed: ConfigShellToolType = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed, ConfigShellToolType::Shell);
}
