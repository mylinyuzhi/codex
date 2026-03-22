use super::*;

#[test]
fn test_plugins_dir() {
    let dir = plugins_dir(Path::new("/home/user/.cocode"));
    assert_eq!(dir, PathBuf::from("/home/user/.cocode/plugins"));
}

#[test]
fn test_cache_dir() {
    let dir = cache_dir(Path::new("/home/user/.cocode/plugins"));
    assert_eq!(dir, PathBuf::from("/home/user/.cocode/plugins/cache"));
}

#[test]
fn test_versioned_cache_path() {
    let path = versioned_cache_path(
        Path::new("/home/user/.cocode/plugins"),
        "my-marketplace",
        "hello-plugin",
        "1.0.0",
    );
    assert_eq!(
        path,
        PathBuf::from("/home/user/.cocode/plugins/cache/my-marketplace/hello-plugin/1.0.0")
    );
}

#[test]
fn test_sanitize_path_component() {
    assert_eq!(sanitize_path_component("hello-world"), "hello-world");
    assert_eq!(sanitize_path_component("hello_world"), "hello_world");
    assert_eq!(sanitize_path_component("owner/repo"), "owner-repo");
    assert_eq!(sanitize_path_component("my@plugin!v2"), "my-plugin-v2");
    assert_eq!(sanitize_path_component("simple123"), "simple123");
}

#[test]
fn test_resolve_version_manifest() {
    assert_eq!(resolve_version(Some("1.2.3"), Some("0.1.0"), None), "1.2.3");
}

#[test]
fn test_resolve_version_marketplace_fallback() {
    assert_eq!(resolve_version(None, Some("0.1.0"), None), "0.1.0");
    assert_eq!(resolve_version(Some(""), Some("0.1.0"), None), "0.1.0");
}

#[test]
fn test_resolve_version_default() {
    assert_eq!(resolve_version(None, None, None), "0.0.0");
}

#[test]
fn test_copy_to_versioned_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source");
    let target = tmp.path().join("target");

    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("plugin.json"), r#"{"plugin":{"name":"test"}}"#).unwrap();

    let sub = source.join("skills");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("hello.md"), "Hello").unwrap();

    copy_to_versioned_cache(&source, &target).unwrap();

    assert!(target.join("plugin.json").exists());
    assert!(target.join("skills").join("hello.md").exists());
}

#[test]
fn test_copy_to_versioned_cache_overwrites() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source");
    let target = tmp.path().join("target");

    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("file.txt"), "v1").unwrap();

    copy_to_versioned_cache(&source, &target).unwrap();
    assert_eq!(
        std::fs::read_to_string(target.join("file.txt")).unwrap(),
        "v1"
    );

    std::fs::write(source.join("file.txt"), "v2").unwrap();
    copy_to_versioned_cache(&source, &target).unwrap();
    assert_eq!(
        std::fs::read_to_string(target.join("file.txt")).unwrap(),
        "v2"
    );
}

#[test]
fn test_delete_plugin_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = tmp.path().join("cache");
    let plugin_dir = cache_root.join("market").join("plugin").join("1.0.0");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(plugin_dir.join("plugin.json"), "test").unwrap();

    delete_plugin_cache(&plugin_dir, &cache_root).unwrap();

    assert!(!plugin_dir.exists());
    // Empty parent dirs should be cleaned up
    assert!(!cache_root.join("market").join("plugin").exists());
    assert!(!cache_root.join("market").exists());
}

#[test]
fn test_copy_excludes_dot_git() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source");
    let target = tmp.path().join("target");

    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("plugin.json"), r#"{"plugin":{"name":"test"}}"#).unwrap();

    // Create a .git directory with content
    let git_dir = source.join(".git");
    std::fs::create_dir_all(git_dir.join("objects")).unwrap();
    std::fs::write(git_dir.join("HEAD"), "ref: refs/heads/main").unwrap();
    std::fs::write(git_dir.join("config"), "[core]").unwrap();

    // Create a nested .git directory
    let nested = source.join("subdir");
    std::fs::create_dir_all(nested.join(".git")).unwrap();
    std::fs::write(nested.join("file.txt"), "hello").unwrap();

    copy_to_versioned_cache(&source, &target).unwrap();

    assert!(target.join("plugin.json").exists());
    assert!(!target.join(".git").exists());
    assert!(target.join("subdir").join("file.txt").exists());
    assert!(!target.join("subdir").join(".git").exists());
}

#[test]
fn test_delete_plugin_cache_nonexistent() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = tmp.path().join("cache");
    let plugin_dir = cache_root.join("nonexistent");
    // Should not error on nonexistent path
    delete_plugin_cache(&plugin_dir, &cache_root).unwrap();
}
