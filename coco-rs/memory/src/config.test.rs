use super::*;

#[test]
fn test_default_config_is_enabled() {
    let config = MemoryConfig::default();
    assert!(config.enabled);
    assert!(config.extraction_enabled);
    assert!(!config.team_memory_enabled);
    assert!(!config.skip_index);
    assert_eq!(config.extraction_throttle, 1);
}

#[test]
fn test_disabled_config() {
    let config = MemoryConfig::disabled();
    assert!(!config.enabled);
    assert!(!config.extraction_enabled);
}

#[test]
fn test_resolve_memory_dir_default() {
    let config = MemoryConfig::default();
    let project = std::path::Path::new("/home/user/my-project");
    let dir = config.resolve_memory_dir(project);
    assert!(dir.to_string_lossy().contains("memory"));
    assert!(dir.to_string_lossy().contains(".claude"));
}

#[test]
fn test_resolve_memory_dir_custom() {
    let config = MemoryConfig {
        custom_directory: Some(std::path::PathBuf::from("/custom/mem")),
        ..MemoryConfig::default()
    };
    let project = std::path::Path::new("/home/user/project");
    let dir = config.resolve_memory_dir(project);
    assert_eq!(dir, std::path::PathBuf::from("/custom/mem"));
}

#[test]
fn test_sanitize_project_path() {
    assert_eq!(
        sanitize_project_path(std::path::Path::new("/home/user/proj")),
        "home-user-proj"
    );
    assert_eq!(
        sanitize_project_path(std::path::Path::new("/a/b/c")),
        "a-b-c"
    );
}
