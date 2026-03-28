use super::*;

#[tokio::test]
async fn test_persist_rule_creates_new_file() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let home = dir.path();

    persist_rule(home, "Read", "").await.expect("persist");

    let content = tokio::fs::read_to_string(home.join("settings.local.json"))
        .await
        .expect("read");
    let config: serde_json::Value = serde_json::from_str(&content).expect("parse");

    let allow = config["permissions"]["allow"].as_array().expect("array");
    assert_eq!(allow.len(), 1);
    assert_eq!(allow[0], "Read");
}

#[tokio::test]
async fn test_persist_rule_with_pattern() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let home = dir.path();

    persist_rule(home, "Bash", "git *").await.expect("persist");

    let content = tokio::fs::read_to_string(home.join("settings.local.json"))
        .await
        .expect("read");
    let config: serde_json::Value = serde_json::from_str(&content).expect("parse");

    let allow = config["permissions"]["allow"].as_array().expect("array");
    assert_eq!(allow[0], "Bash(git *)");
}

#[tokio::test]
async fn test_persist_rule_appends_to_existing() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let home = dir.path();

    persist_rule(home, "Read", "").await.expect("persist 1");
    persist_rule(home, "Edit", "").await.expect("persist 2");

    let content = tokio::fs::read_to_string(home.join("settings.local.json"))
        .await
        .expect("read");
    let config: serde_json::Value = serde_json::from_str(&content).expect("parse");

    let allow = config["permissions"]["allow"].as_array().expect("array");
    assert_eq!(allow.len(), 2);
    assert_eq!(allow[0], "Read");
    assert_eq!(allow[1], "Edit");
}

#[tokio::test]
async fn test_persist_rule_deduplicates() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let home = dir.path();

    persist_rule(home, "Read", "").await.expect("persist 1");
    persist_rule(home, "Read", "").await.expect("persist 2");

    let content = tokio::fs::read_to_string(home.join("settings.local.json"))
        .await
        .expect("read");
    let config: serde_json::Value = serde_json::from_str(&content).expect("parse");

    let allow = config["permissions"]["allow"].as_array().expect("array");
    assert_eq!(allow.len(), 1);
}

#[tokio::test]
async fn test_persist_rule_preserves_existing_config() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let home = dir.path();
    let settings_path = home.join("settings.local.json");

    let existing = serde_json::json!({
        "other_key": "preserved",
        "permissions": {
            "deny": ["Write"]
        }
    });
    tokio::fs::write(
        &settings_path,
        serde_json::to_string_pretty(&existing).expect("ser"),
    )
    .await
    .expect("write");

    persist_rule(home, "Read", "").await.expect("persist");

    let content = tokio::fs::read_to_string(&settings_path)
        .await
        .expect("read");
    let config: serde_json::Value = serde_json::from_str(&content).expect("parse");

    assert_eq!(config["other_key"], "preserved");
    assert_eq!(config["permissions"]["deny"][0], "Write");
    assert_eq!(config["permissions"]["allow"][0], "Read");
}

#[tokio::test]
async fn test_remove_rule() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let home = dir.path();

    persist_rule(home, "Read", "").await.expect("persist 1");
    persist_rule(home, "Edit", "").await.expect("persist 2");
    remove_rule(home, "Read", "").await.expect("remove");

    let content = tokio::fs::read_to_string(home.join("settings.local.json"))
        .await
        .expect("read");
    let config: serde_json::Value = serde_json::from_str(&content).expect("parse");

    let allow = config["permissions"]["allow"].as_array().expect("array");
    assert_eq!(allow.len(), 1);
    assert_eq!(allow[0], "Edit");
}

#[tokio::test]
async fn test_remove_rule_nonexistent_file() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let home = dir.path();

    // Should not error when file doesn't exist
    remove_rule(home, "Read", "").await.expect("remove");
}

#[tokio::test]
async fn test_remove_rule_with_pattern() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let home = dir.path();

    persist_rule(home, "Bash", "git *").await.expect("persist");
    remove_rule(home, "Bash", "git *").await.expect("remove");

    let content = tokio::fs::read_to_string(home.join("settings.local.json"))
        .await
        .expect("read");
    let config: serde_json::Value = serde_json::from_str(&content).expect("parse");

    let allow = config["permissions"]["allow"].as_array().expect("array");
    assert!(allow.is_empty());
}

// New tests for RuleAction and RuleDestination

#[tokio::test]
async fn test_persist_deny_rule() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let home = dir.path();

    persist_rule_with_options(
        home,
        "Bash",
        "rm -rf *",
        RuleAction::Deny,
        RuleDestination::Local,
    )
    .await
    .expect("persist deny");

    let content = tokio::fs::read_to_string(home.join("settings.local.json"))
        .await
        .expect("read");
    let config: serde_json::Value = serde_json::from_str(&content).expect("parse");

    let deny = config["permissions"]["deny"].as_array().expect("array");
    assert_eq!(deny.len(), 1);
    assert_eq!(deny[0], "Bash(rm -rf *)");
}

#[tokio::test]
async fn test_persist_ask_rule() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let home = dir.path();

    persist_rule_with_options(home, "Write", "", RuleAction::Ask, RuleDestination::Local)
        .await
        .expect("persist ask");

    let content = tokio::fs::read_to_string(home.join("settings.local.json"))
        .await
        .expect("read");
    let config: serde_json::Value = serde_json::from_str(&content).expect("parse");

    let ask = config["permissions"]["ask"].as_array().expect("array");
    assert_eq!(ask.len(), 1);
    assert_eq!(ask[0], "Write");
}

#[tokio::test]
async fn test_persist_user_destination() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let home = dir.path();

    persist_rule_with_options(home, "Read", "", RuleAction::Allow, RuleDestination::User)
        .await
        .expect("persist user");

    // User destination writes to settings.json (not settings.local.json)
    assert!(home.join("settings.json").exists());
    assert!(!home.join("settings.local.json").exists());
}

#[tokio::test]
async fn test_remove_deny_rule() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let home = dir.path();

    persist_rule_with_options(
        home,
        "Bash",
        "rm *",
        RuleAction::Deny,
        RuleDestination::Local,
    )
    .await
    .expect("persist");
    remove_rule_with_options(
        home,
        "Bash",
        "rm *",
        RuleAction::Deny,
        RuleDestination::Local,
    )
    .await
    .expect("remove");

    let content = tokio::fs::read_to_string(home.join("settings.local.json"))
        .await
        .expect("read");
    let config: serde_json::Value = serde_json::from_str(&content).expect("parse");

    let deny = config["permissions"]["deny"].as_array().expect("array");
    assert!(deny.is_empty());
}

#[tokio::test]
async fn test_mixed_actions_coexist() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let home = dir.path();

    persist_rule_with_options(home, "Read", "", RuleAction::Allow, RuleDestination::Local)
        .await
        .expect("allow");
    persist_rule_with_options(home, "Write", "", RuleAction::Ask, RuleDestination::Local)
        .await
        .expect("ask");
    persist_rule_with_options(
        home,
        "Bash",
        "rm -rf *",
        RuleAction::Deny,
        RuleDestination::Local,
    )
    .await
    .expect("deny");

    let content = tokio::fs::read_to_string(home.join("settings.local.json"))
        .await
        .expect("read");
    let config: serde_json::Value = serde_json::from_str(&content).expect("parse");

    assert_eq!(config["permissions"]["allow"][0], "Read");
    assert_eq!(config["permissions"]["ask"][0], "Write");
    assert_eq!(config["permissions"]["deny"][0], "Bash(rm -rf *)");
}
