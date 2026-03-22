use super::*;
use std::collections::HashMap;
use std::path::Path;

#[test]
fn test_parse_manifest() {
    let json = r#"{
  "plugin": {
    "name": "test-plugin",
    "version": "1.0.0",
    "description": "A test plugin",
    "author": "Test Author"
  },
  "contributions": {
    "skills": ["skills/"],
    "hooks": ["hooks.json"]
  }
}"#;

    let manifest = PluginManifest::from_str(json, Path::new("test")).unwrap();
    assert_eq!(manifest.plugin.name, "test-plugin");
    assert_eq!(manifest.plugin.version, "1.0.0");
    assert_eq!(manifest.plugin.author, Some("Test Author".to_string()));
    assert_eq!(manifest.contributions.skills, vec!["skills/"]);
    assert_eq!(manifest.contributions.hooks, vec!["hooks.json"]);
}

#[test]
fn test_parse_minimal_manifest() {
    let json = r#"{
  "plugin": {
    "name": "minimal",
    "version": "0.1.0",
    "description": "Minimal plugin"
  }
}"#;

    let manifest = PluginManifest::from_str(json, Path::new("test")).unwrap();
    assert_eq!(manifest.plugin.name, "minimal");
    assert!(manifest.contributions.skills.is_empty());
    assert!(manifest.contributions.hooks.is_empty());
}

#[test]
fn test_validate_manifest() {
    let manifest = PluginManifest {
        plugin: PluginMetadata {
            name: "valid-name".to_string(),
            version: "1.0.0".to_string(),
            description: "Valid description".to_string(),
            author: None,
            repository: None,
            license: None,
            min_cocode_version: None,
        },
        contributions: PluginContributions::default(),
        user_config: HashMap::new(),
    };

    assert!(manifest.validate().is_ok());
}

#[test]
fn test_validate_empty_name() {
    let manifest = PluginManifest {
        plugin: PluginMetadata {
            name: "".to_string(),
            version: "1.0.0".to_string(),
            description: "Description".to_string(),
            author: None,
            repository: None,
            license: None,
            min_cocode_version: None,
        },
        contributions: PluginContributions::default(),
        user_config: HashMap::new(),
    };

    let errors = manifest.validate().unwrap_err();
    assert!(errors.iter().any(|e| e.contains("name")));
}

#[test]
fn test_validate_invalid_name_chars() {
    let manifest = PluginManifest {
        plugin: PluginMetadata {
            name: "invalid name!".to_string(),
            version: "1.0.0".to_string(),
            description: "Description".to_string(),
            author: None,
            repository: None,
            license: None,
            min_cocode_version: None,
        },
        contributions: PluginContributions::default(),
        user_config: HashMap::new(),
    };

    let errors = manifest.validate().unwrap_err();
    assert!(errors.iter().any(|e| e.contains("lowercase")));
}

#[test]
fn test_from_dir_hidden_plugin_dir() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let plugin_dir = tmp.path().join("my-plugin");
    let hidden_dir = plugin_dir.join(PLUGIN_DIR);
    std::fs::create_dir_all(&hidden_dir).expect("mkdir");

    std::fs::write(
        hidden_dir.join(PLUGIN_JSON),
        r#"{
  "plugin": {
    "name": "hidden-plugin",
    "version": "1.0.0",
    "description": "Plugin in .cocode-plugin/"
  }
}"#,
    )
    .expect("write");

    let manifest = PluginManifest::from_dir(&plugin_dir).unwrap();
    assert_eq!(manifest.plugin.name, "hidden-plugin");
}

#[test]
fn test_from_dir_prefers_hidden_over_root() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let plugin_dir = tmp.path().join("my-plugin");
    let hidden_dir = plugin_dir.join(PLUGIN_DIR);
    std::fs::create_dir_all(&hidden_dir).expect("mkdir");

    // Write both root and hidden manifest
    std::fs::write(
        plugin_dir.join(PLUGIN_JSON),
        r#"{"plugin": {"name": "root-plugin", "version": "1.0.0", "description": "Root"}}"#,
    )
    .expect("write root");
    std::fs::write(
        hidden_dir.join(PLUGIN_JSON),
        r#"{"plugin": {"name": "hidden-plugin", "version": "1.0.0", "description": "Hidden"}}"#,
    )
    .expect("write hidden");

    let manifest = PluginManifest::from_dir(&plugin_dir).unwrap();
    // .cocode-plugin/ should be preferred
    assert_eq!(manifest.plugin.name, "hidden-plugin");
}
