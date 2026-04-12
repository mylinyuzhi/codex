use std::io::Write;

use coco_types::PermissionRuleValue;

use super::*;

/// Create a temp dir with a settings file containing permission rules.
fn setup_temp_settings(rules_json: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let claude_dir = dir.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).expect("create .claude dir");
    let settings_path = claude_dir.join("settings.json");
    let mut f = std::fs::File::create(&settings_path).expect("create settings file");
    f.write_all(rules_json.as_bytes()).expect("write settings");
    (dir, settings_path)
}

#[test]
fn test_load_rules_from_project_settings() {
    let (dir, _) = setup_temp_settings(
        r#"{ "permissions": { "allow": ["Read", "Bash(git *)"], "deny": ["Write"] } }"#,
    );
    let store = SettingsPermissionStore::new(dir.path());
    let rules = store.load_all_rules();

    assert_eq!(rules.allow.len(), 2, "expected 2 allow rules");
    assert_eq!(rules.deny.len(), 1, "expected 1 deny rule");
    assert_eq!(rules.ask.len(), 0);

    // Verify parsed rule content
    assert_eq!(rules.allow[0].value.tool_pattern, "Read");
    assert!(rules.allow[0].value.rule_content.is_none());
    assert_eq!(rules.allow[1].value.tool_pattern, "Bash");
    assert_eq!(rules.allow[1].value.rule_content.as_deref(), Some("git *"));
    assert_eq!(rules.deny[0].value.tool_pattern, "Write");
}

#[test]
fn test_persist_add_rules() {
    let (dir, settings_path) =
        setup_temp_settings(r#"{ "permissions": { "allow": ["Read"] }, "model": "claude" }"#);
    let store = SettingsPermissionStore::new(dir.path());

    // Add a new allow rule
    let update = PermissionUpdate::AddRules {
        rules: vec![PermissionRule {
            source: PermissionRuleSource::ProjectSettings,
            behavior: PermissionBehavior::Allow,
            value: PermissionRuleValue {
                tool_pattern: "Bash".into(),
                rule_content: Some("git *".into()),
            },
        }],
        destination: PermissionUpdateDestination::ProjectSettings,
    };
    store.persist_update(&update).expect("persist update");

    // Re-read the file
    let contents = std::fs::read_to_string(&settings_path).expect("read settings");
    let value: serde_json::Value = serde_json::from_str(&contents).expect("parse JSON");

    // Verify existing fields preserved
    assert_eq!(value["model"], "claude");

    // Verify rules
    let allow = value["permissions"]["allow"]
        .as_array()
        .expect("allow array");
    assert_eq!(allow.len(), 2);
    assert_eq!(allow[0], "Read");
    assert_eq!(allow[1], "Bash(git *)");
}

#[test]
fn test_persist_no_duplicates() {
    let (dir, settings_path) = setup_temp_settings(r#"{ "permissions": { "allow": ["Read"] } }"#);
    let store = SettingsPermissionStore::new(dir.path());

    let update = PermissionUpdate::AddRules {
        rules: vec![PermissionRule {
            source: PermissionRuleSource::ProjectSettings,
            behavior: PermissionBehavior::Allow,
            value: PermissionRuleValue {
                tool_pattern: "Read".into(),
                rule_content: None,
            },
        }],
        destination: PermissionUpdateDestination::ProjectSettings,
    };
    store.persist_update(&update).expect("persist");

    let contents = std::fs::read_to_string(&settings_path).expect("read");
    let value: serde_json::Value = serde_json::from_str(&contents).expect("parse");
    let allow = value["permissions"]["allow"].as_array().expect("array");
    assert_eq!(allow.len(), 1, "should not duplicate existing rule");
}

#[test]
fn test_session_updates_not_persisted() {
    let (dir, settings_path) = setup_temp_settings(r#"{ "permissions": { "allow": [] } }"#);
    let store = SettingsPermissionStore::new(dir.path());

    let update = PermissionUpdate::AddRules {
        rules: vec![PermissionRule {
            source: PermissionRuleSource::Session,
            behavior: PermissionBehavior::Allow,
            value: PermissionRuleValue {
                tool_pattern: "Bash".into(),
                rule_content: None,
            },
        }],
        destination: PermissionUpdateDestination::Session,
    };
    store.persist_update(&update).expect("persist");

    let contents = std::fs::read_to_string(&settings_path).expect("read");
    let value: serde_json::Value = serde_json::from_str(&contents).expect("parse");
    let allow = value["permissions"]["allow"].as_array().expect("array");
    assert_eq!(allow.len(), 0, "session rules should not touch disk");
}

#[test]
fn test_persist_remove_rules() {
    let (dir, settings_path) =
        setup_temp_settings(r#"{ "permissions": { "allow": ["Read", "Bash(git *)", "Write"] } }"#);
    let store = SettingsPermissionStore::new(dir.path());

    let update = PermissionUpdate::RemoveRules {
        rules: vec![PermissionRule {
            source: PermissionRuleSource::ProjectSettings,
            behavior: PermissionBehavior::Allow,
            value: PermissionRuleValue {
                tool_pattern: "Bash".into(),
                rule_content: Some("git *".into()),
            },
        }],
        destination: PermissionUpdateDestination::ProjectSettings,
    };
    store.persist_update(&update).expect("persist remove");

    let contents = std::fs::read_to_string(&settings_path).expect("read");
    let value: serde_json::Value = serde_json::from_str(&contents).expect("parse");
    let allow = value["permissions"]["allow"].as_array().expect("array");
    assert_eq!(allow.len(), 2, "should have removed Bash(git *)");
    assert_eq!(allow[0], "Read");
    assert_eq!(allow[1], "Write");
}

#[test]
fn test_show_always_allow_options_default_true() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let store = SettingsPermissionStore::new(dir.path());
    assert!(store.show_always_allow_options());
}
