use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::*;

#[test]
fn test_load_missing_file() {
    let dir = TempDir::new().unwrap();
    let (bindings, warnings) = load_user_bindings(dir.path());
    assert!(bindings.is_empty());
    assert!(warnings.is_empty());
}

#[test]
fn test_load_empty_file() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("keybindings.json");
    std::fs::write(&path, r#"{"bindings": []}"#).unwrap();

    let (bindings, warnings) = load_user_bindings(dir.path());
    assert!(bindings.is_empty());
    assert!(warnings.is_empty());
}

#[test]
fn test_load_valid_bindings() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("keybindings.json");
    let content = r#"{
        "bindings": [
            {
                "context": "Chat",
                "bindings": { "ctrl+t": "app:toggleTodos" }
            }
        ]
    }"#;
    std::fs::write(&path, content).unwrap();

    let (bindings, warnings) = load_user_bindings(dir.path());
    assert_eq!(bindings.len(), 1);
    assert!(warnings.is_empty(), "expected no warnings");
}

#[test]
fn test_load_invalid_json() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("keybindings.json");
    std::fs::write(&path, "not json").unwrap();

    let (bindings, warnings) = load_user_bindings(dir.path());
    assert!(bindings.is_empty());
    assert_eq!(warnings.len(), 1);
}

#[test]
fn test_load_invalid_context() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("keybindings.json");
    let content = r#"{
        "bindings": [
            {
                "context": "NonExistent",
                "bindings": { "ctrl+t": "app:toggleTodos" }
            }
        ]
    }"#;
    std::fs::write(&path, content).unwrap();

    let (bindings, warnings) = load_user_bindings(dir.path());
    assert!(bindings.is_empty());
    assert_eq!(warnings.len(), 1);
}

#[test]
fn test_load_invalid_action() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("keybindings.json");
    let content = r#"{
        "bindings": [
            {
                "context": "Chat",
                "bindings": { "ctrl+t": "unknown:action" }
            }
        ]
    }"#;
    std::fs::write(&path, content).unwrap();

    let (bindings, warnings) = load_user_bindings(dir.path());
    assert!(bindings.is_empty());
    assert_eq!(warnings.len(), 1);
}

#[test]
fn test_load_null_unbind_skipped() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("keybindings.json");
    let content = r#"{
        "bindings": [
            {
                "context": "Chat",
                "bindings": { "ctrl+t": null }
            }
        ]
    }"#;
    std::fs::write(&path, content).unwrap();

    let (bindings, warnings) = load_user_bindings(dir.path());
    assert!(bindings.is_empty());
    assert!(warnings.is_empty());
}

#[test]
fn test_load_command_binding() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("keybindings.json");
    let content = r#"{
        "bindings": [
            {
                "context": "Chat",
                "bindings": { "ctrl+d": "command:doctor" }
            }
        ]
    }"#;
    std::fs::write(&path, content).unwrap();

    let (bindings, warnings) = load_user_bindings(dir.path());
    assert_eq!(bindings.len(), 1);
    assert!(
        warnings.is_empty(),
        "expected no warnings for command binding"
    );
}

#[test]
fn test_load_chord_binding_from_json() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("keybindings.json");
    let content = r#"{
        "bindings": [
            {
                "context": "Chat",
                "bindings": { "ctrl+k ctrl+c": "ext:clearScreen" }
            }
        ]
    }"#;
    std::fs::write(&path, content).unwrap();

    let (bindings, warnings) = load_user_bindings(dir.path());
    assert_eq!(bindings.len(), 1);
    assert!(bindings[0].sequence.is_chord());
    assert!(
        warnings.is_empty(),
        "expected no warnings for chord binding"
    );
}
