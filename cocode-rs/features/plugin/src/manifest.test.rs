use super::*;
use std::path::Path;

#[test]
fn test_parse_manifest() {
    let toml = r#"
[plugin]
name = "test-plugin"
version = "1.0.0"
description = "A test plugin"
author = "Test Author"

[contributions]
skills = ["skills/"]
hooks = ["hooks.toml"]
"#;

    let manifest = PluginManifest::from_str(toml, Path::new("test")).unwrap();
    assert_eq!(manifest.plugin.name, "test-plugin");
    assert_eq!(manifest.plugin.version, "1.0.0");
    assert_eq!(manifest.plugin.author, Some("Test Author".to_string()));
    assert_eq!(manifest.contributions.skills, vec!["skills/"]);
    assert_eq!(manifest.contributions.hooks, vec!["hooks.toml"]);
}

#[test]
fn test_parse_minimal_manifest() {
    let toml = r#"
[plugin]
name = "minimal"
version = "0.1.0"
description = "Minimal plugin"
"#;

    let manifest = PluginManifest::from_str(toml, Path::new("test")).unwrap();
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
    };

    let errors = manifest.validate().unwrap_err();
    assert!(errors.iter().any(|e| e.contains("alphanumeric")));
}
