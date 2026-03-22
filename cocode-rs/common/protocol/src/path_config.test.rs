use super::*;

#[test]
fn test_path_config_default() {
    let config = PathConfig::default();
    assert!(config.project_dir.is_none());
    assert!(config.plugin_root.is_none());
    assert!(config.env_file.is_none());
    assert!(config.is_empty());
}

#[test]
fn test_path_config_new() {
    let config = PathConfig::new(
        Some(PathBuf::from("/project")),
        Some(PathBuf::from("/plugins")),
        Some(PathBuf::from("/.env")),
    );
    assert_eq!(config.project_dir, Some(PathBuf::from("/project")));
    assert_eq!(config.plugin_root, Some(PathBuf::from("/plugins")));
    assert_eq!(config.env_file, Some(PathBuf::from("/.env")));
    assert!(!config.is_empty());
}

#[test]
fn test_path_config_serde() {
    let json = r#"{
        "project_dir": "/project",
        "plugin_root": "/plugins",
        "env_file": "/.env"
    }"#;
    let config: PathConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.project_dir, Some(PathBuf::from("/project")));
    assert_eq!(config.plugin_root, Some(PathBuf::from("/plugins")));
    assert_eq!(config.env_file, Some(PathBuf::from("/.env")));
}

#[test]
fn test_path_config_serde_defaults() {
    let json = r#"{}"#;
    let config: PathConfig = serde_json::from_str(json).unwrap();
    assert!(config.project_dir.is_none());
    assert!(config.plugin_root.is_none());
    assert!(config.env_file.is_none());
}

#[test]
fn test_merge() {
    let mut base = PathConfig {
        project_dir: Some(PathBuf::from("/base")),
        plugin_root: Some(PathBuf::from("/base-plugins")),
        env_file: None,
    };

    let override_config = PathConfig {
        project_dir: None,
        plugin_root: Some(PathBuf::from("/override-plugins")),
        env_file: Some(PathBuf::from("/.env")),
    };

    base.merge(&override_config);

    // project_dir unchanged (override is None)
    assert_eq!(base.project_dir, Some(PathBuf::from("/base")));
    // plugin_root overridden
    assert_eq!(base.plugin_root, Some(PathBuf::from("/override-plugins")));
    // env_file added
    assert_eq!(base.env_file, Some(PathBuf::from("/.env")));
}

#[test]
fn test_is_empty() {
    let config = PathConfig::default();
    assert!(config.is_empty());

    let config = PathConfig {
        project_dir: Some(PathBuf::from("/project")),
        ..Default::default()
    };
    assert!(!config.is_empty());
}
