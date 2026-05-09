use super::*;
use coco_config::Settings;
use coco_config::SettingsWithSource;
use coco_types::PermissionBehavior;
use coco_types::PermissionRuleSource;
use std::collections::HashMap;

fn settings_with(per_source: HashMap<SettingSource, serde_json::Value>) -> SettingsWithSource {
    SettingsWithSource {
        merged: Settings::default(),
        per_source,
    }
}

#[test]
fn maps_settings_sources_to_permission_sources_per_behavior() {
    let user_raw = serde_json::json!({
        "permissions": {
            "allow": ["WebFetch(domain:example.com)"],
            "deny":  ["Read(/etc/secret)"],
            "ask":   ["Bash(rm:*)"]
        }
    });
    let policy_raw = serde_json::json!({
        "permissions": {
            "allow": ["WebFetch(domain:enterprise.com)"]
        }
    });
    let mut per = HashMap::new();
    per.insert(SettingSource::User, user_raw);
    per.insert(SettingSource::Policy, policy_raw);
    let s = settings_with(per);

    let (allow, deny, ask) = typed_permission_rules(&s);

    let user_allow = allow.get(&PermissionRuleSource::UserSettings).unwrap();
    assert_eq!(user_allow.len(), 1);
    assert_eq!(user_allow[0].behavior, PermissionBehavior::Allow);
    assert_eq!(user_allow[0].value.tool_pattern, "WebFetch");
    assert_eq!(
        user_allow[0].value.rule_content.as_deref(),
        Some("domain:example.com")
    );

    let user_deny = deny.get(&PermissionRuleSource::UserSettings).unwrap();
    assert_eq!(user_deny.len(), 1);
    assert_eq!(user_deny[0].behavior, PermissionBehavior::Deny);

    let user_ask = ask.get(&PermissionRuleSource::UserSettings).unwrap();
    assert_eq!(user_ask.len(), 1);
    assert_eq!(user_ask[0].behavior, PermissionBehavior::Ask);
    assert_eq!(user_ask[0].value.tool_pattern, "Bash");

    let policy_allow = allow.get(&PermissionRuleSource::PolicySettings).unwrap();
    assert_eq!(policy_allow.len(), 1);
}

#[test]
fn drops_plugin_sourced_rules() {
    let plugin_raw = serde_json::json!({
        "permissions": { "allow": ["WebFetch(domain:plugin.com)"] }
    });
    let mut per = HashMap::new();
    per.insert(SettingSource::Plugin, plugin_raw);
    let s = settings_with(per);

    let (allow, deny, ask) = typed_permission_rules(&s);
    assert!(allow.is_empty(), "plugin allow rules dropped");
    assert!(deny.is_empty());
    assert!(ask.is_empty());
}

#[test]
fn handles_missing_permissions_block_gracefully() {
    let raw = serde_json::json!({ "model": "claude-haiku-4-5-20251001" });
    let mut per = HashMap::new();
    per.insert(SettingSource::User, raw);
    let s = settings_with(per);

    let (allow, deny, ask) = typed_permission_rules(&s);
    assert!(allow.is_empty());
    assert!(deny.is_empty());
    assert!(ask.is_empty());
}
