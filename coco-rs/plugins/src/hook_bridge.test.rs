use std::collections::HashMap;

use coco_hooks::HookRegistry;
use coco_types::HookEventType;
use coco_types::HookScope;
use pretty_assertions::assert_eq;

use crate::LoadedPlugin;
use crate::PluginManifest;
use crate::PluginSource;

use super::*;

fn test_plugin(name: &str, path: &std::path::Path) -> LoadedPlugin {
    LoadedPlugin {
        name: name.to_string(),
        manifest: PluginManifest {
            name: name.to_string(),
            version: None,
            description: "Test plugin".to_string(),
            skills: vec![],
            hooks: HashMap::new(),
            mcp_servers: HashMap::new(),
        },
        path: path.to_path_buf(),
        source: PluginSource::User,
        enabled: true,
    }
}

#[test]
fn test_load_hooks_from_hooks_dir() {
    let dir = tempfile::tempdir().unwrap();
    let hooks_dir = dir.path().join("hooks");
    std::fs::create_dir(&hooks_dir).unwrap();
    std::fs::write(
        hooks_dir.join("hooks.json"),
        r#"{
            "pre_tool_use": [
                { "type": "command", "command": "echo pre", "matcher": "Bash" }
            ]
        }"#,
    )
    .unwrap();

    let plugin = test_plugin("my-plugin", dir.path());
    let hooks = load_plugin_hooks(&plugin);

    assert_eq!(hooks.len(), 1);
    assert_eq!(hooks[0].event, HookEventType::PreToolUse);
    assert_eq!(hooks[0].matcher.as_deref(), Some("Bash"));
    assert_eq!(hooks[0].scope, HookScope::Builtin);
    assert_eq!(
        hooks[0].status_message.as_deref(),
        Some("[my-plugin] running hook")
    );
}

#[test]
fn test_load_hooks_from_manifest_inline() {
    let dir = tempfile::tempdir().unwrap();
    let mut manifest_hooks = HashMap::new();
    manifest_hooks.insert(
        "session_start".to_string(),
        serde_json::json!([{ "type": "prompt", "prompt": "hello from plugin" }]),
    );

    let plugin = LoadedPlugin {
        name: "inline-plugin".to_string(),
        manifest: PluginManifest {
            name: "inline-plugin".to_string(),
            version: None,
            description: "Test".to_string(),
            skills: vec![],
            hooks: manifest_hooks,
            mcp_servers: HashMap::new(),
        },
        path: dir.path().to_path_buf(),
        source: PluginSource::User,
        enabled: true,
    };

    let hooks = load_plugin_hooks(&plugin);

    assert_eq!(hooks.len(), 1);
    assert_eq!(hooks[0].event, HookEventType::SessionStart);
    assert_eq!(hooks[0].scope, HookScope::Builtin);
    assert!(
        hooks[0]
            .status_message
            .as_deref()
            .unwrap()
            .starts_with("[inline-plugin]")
    );
}

#[test]
fn test_load_hooks_deduplication() {
    let dir = tempfile::tempdir().unwrap();

    // hooks/hooks.json has the same hook as the manifest
    let hooks_dir = dir.path().join("hooks");
    std::fs::create_dir(&hooks_dir).unwrap();
    std::fs::write(
        hooks_dir.join("hooks.json"),
        r#"{ "pre_tool_use": [{ "type": "command", "command": "echo dup" }] }"#,
    )
    .unwrap();

    let mut manifest_hooks = HashMap::new();
    manifest_hooks.insert(
        "pre_tool_use".to_string(),
        serde_json::json!([{ "type": "command", "command": "echo dup" }]),
    );

    let plugin = LoadedPlugin {
        name: "dup-plugin".to_string(),
        manifest: PluginManifest {
            name: "dup-plugin".to_string(),
            version: None,
            description: "Test".to_string(),
            skills: vec![],
            hooks: manifest_hooks,
            mcp_servers: HashMap::new(),
        },
        path: dir.path().to_path_buf(),
        source: PluginSource::User,
        enabled: true,
    };

    // load_plugin_hooks returns both (no dedup at load time)
    let hooks = load_plugin_hooks(&plugin);
    assert_eq!(hooks.len(), 2);

    // register_plugin_hooks deduplicates via register_deduped
    let mut registry = HookRegistry::new();
    register_plugin_hooks(&mut registry, &[&plugin]);
    // The two hooks have the same command, so one is deduplicated
    assert_eq!(registry.len(), 1);
}

#[test]
fn test_load_hooks_empty_plugin() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = test_plugin("empty-plugin", dir.path());
    let hooks = load_plugin_hooks(&plugin);
    assert!(hooks.is_empty());
}

#[test]
fn test_load_all_plugin_hooks_multiple_plugins() {
    let dir1 = tempfile::tempdir().unwrap();
    let dir2 = tempfile::tempdir().unwrap();

    let hooks_dir1 = dir1.path().join("hooks");
    std::fs::create_dir(&hooks_dir1).unwrap();
    std::fs::write(
        hooks_dir1.join("hooks.json"),
        r#"{ "pre_tool_use": [{ "type": "command", "command": "echo p1" }] }"#,
    )
    .unwrap();

    let hooks_dir2 = dir2.path().join("hooks");
    std::fs::create_dir(&hooks_dir2).unwrap();
    std::fs::write(
        hooks_dir2.join("hooks.json"),
        r#"{ "session_start": [{ "type": "prompt", "prompt": "hi" }] }"#,
    )
    .unwrap();

    let p1 = test_plugin("plugin-a", dir1.path());
    let p2 = test_plugin("plugin-b", dir2.path());

    let hooks = load_all_plugin_hooks(&[&p1, &p2]);
    assert_eq!(hooks.len(), 2);
    assert_eq!(hooks[0].event, HookEventType::PreToolUse);
    assert_eq!(hooks[1].event, HookEventType::SessionStart);
}

#[test]
fn test_status_message_preserves_existing() {
    let dir = tempfile::tempdir().unwrap();
    let hooks_dir = dir.path().join("hooks");
    std::fs::create_dir(&hooks_dir).unwrap();
    std::fs::write(
        hooks_dir.join("hooks.json"),
        r#"{
            "pre_tool_use": [
                { "type": "command", "command": "echo x", "status_message": "linting code" }
            ]
        }"#,
    )
    .unwrap();

    let plugin = test_plugin("lint-plugin", dir.path());
    let hooks = load_plugin_hooks(&plugin);

    assert_eq!(hooks.len(), 1);
    assert_eq!(
        hooks[0].status_message.as_deref(),
        Some("[lint-plugin] linting code")
    );
}
