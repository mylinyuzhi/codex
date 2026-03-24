use pretty_assertions::assert_eq;

use super::*;

#[test]
fn test_deserialize_empty_file() {
    let json = r#"{"bindings": []}"#;
    let file: KeybindingsFile = serde_json::from_str(json).unwrap();
    assert!(file.bindings.is_empty());
}

#[test]
fn test_deserialize_with_bindings() {
    let json = r#"{
        "bindings": [
            {
                "context": "Chat",
                "bindings": {
                    "ctrl+k ctrl+c": "ext:clearScreen",
                    "meta+p": "chat:modelPicker"
                }
            }
        ]
    }"#;
    let file: KeybindingsFile = serde_json::from_str(json).unwrap();
    assert_eq!(file.bindings.len(), 1);
    assert_eq!(file.bindings[0].context, "Chat");
    assert_eq!(file.bindings[0].bindings.len(), 2);
}

#[test]
fn test_deserialize_with_null_unbind() {
    let json = r#"{
        "bindings": [
            {
                "context": "Chat",
                "bindings": {
                    "ctrl+t": null
                }
            }
        ]
    }"#;
    let file: KeybindingsFile = serde_json::from_str(json).unwrap();
    assert_eq!(file.bindings[0].bindings.get("ctrl+t"), Some(&None));
}

#[test]
fn test_deserialize_missing_bindings_field() {
    let json = "{}";
    let file: KeybindingsFile = serde_json::from_str(json).unwrap();
    assert!(file.bindings.is_empty());
}

#[test]
fn test_serialize_roundtrip() {
    let file = KeybindingsFile {
        bindings: vec![ContextBindings {
            context: "Chat".to_string(),
            bindings: {
                let mut m = std::collections::BTreeMap::new();
                m.insert(
                    "ctrl+t".to_string(),
                    Some("ext:cycleThinkingLevel".to_string()),
                );
                m.insert("ctrl+m".to_string(), None);
                m
            },
        }],
    };
    let json = serde_json::to_string_pretty(&file).unwrap();
    let parsed: KeybindingsFile = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.bindings.len(), 1);
    assert_eq!(parsed.bindings[0].bindings.len(), 2);
}
