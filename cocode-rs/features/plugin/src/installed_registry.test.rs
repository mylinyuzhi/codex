use super::*;

fn make_entry(scope: &str, version: &str) -> InstalledPluginEntry {
    InstalledPluginEntry {
        scope: scope.to_string(),
        version: version.to_string(),
        install_path: PathBuf::from(format!("/cache/{scope}/plugin/{version}")),
        installed_at: "2025-01-01T00:00:00Z".to_string(),
        last_updated: "2025-01-01T00:00:00Z".to_string(),
        git_commit_sha: None,
        project_path: None,
    }
}

#[test]
fn test_empty_registry() {
    let reg = InstalledPluginsRegistry::empty();
    assert_eq!(reg.version, 2);
    assert!(reg.is_empty());
    assert!(reg.all_plugin_ids().is_empty());
}

#[test]
fn test_add_and_get() {
    let mut reg = InstalledPluginsRegistry::empty();
    reg.add("hello", make_entry("user", "1.0.0"));

    let entries = reg.get("hello").unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].version, "1.0.0");
    assert_eq!(entries[0].scope, "user");
}

#[test]
fn test_add_multiple_scopes() {
    let mut reg = InstalledPluginsRegistry::empty();
    reg.add("hello", make_entry("user", "1.0.0"));
    reg.add("hello", make_entry("project", "1.1.0"));

    let entries = reg.get("hello").unwrap();
    assert_eq!(entries.len(), 2);
}

#[test]
fn test_add_replaces_same_scope() {
    let mut reg = InstalledPluginsRegistry::empty();
    reg.add("hello", make_entry("user", "1.0.0"));
    reg.add("hello", make_entry("user", "2.0.0"));

    let entries = reg.get("hello").unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].version, "2.0.0");
}

#[test]
fn test_remove() {
    let mut reg = InstalledPluginsRegistry::empty();
    reg.add("hello", make_entry("user", "1.0.0"));
    reg.add("hello", make_entry("project", "1.1.0"));

    let removed = reg.remove("hello", "user").unwrap();
    assert_eq!(removed.version, "1.0.0");

    let entries = reg.get("hello").unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].scope, "project");
}

#[test]
fn test_remove_last_entry_removes_key() {
    let mut reg = InstalledPluginsRegistry::empty();
    reg.add("hello", make_entry("user", "1.0.0"));
    reg.remove("hello", "user");

    assert!(reg.get("hello").is_none());
    assert!(reg.is_empty());
}

#[test]
fn test_remove_nonexistent() {
    let mut reg = InstalledPluginsRegistry::empty();
    assert!(reg.remove("nonexistent", "user").is_none());
}

#[test]
fn test_save_and_load_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("installed_plugins.json");

    let mut reg = InstalledPluginsRegistry::empty();
    reg.add("hello", make_entry("user", "1.0.0"));
    reg.add("world", make_entry("project", "2.0.0"));
    reg.save(&path).unwrap();

    let loaded = InstalledPluginsRegistry::load(&path);
    assert_eq!(loaded.version, 2);
    assert_eq!(loaded.plugins.len(), 2);
    assert_eq!(loaded.get("hello").unwrap()[0].version, "1.0.0");
}

#[test]
fn test_load_missing_file() {
    let reg = InstalledPluginsRegistry::load(Path::new("/nonexistent/path.json"));
    assert!(reg.is_empty());
}

#[test]
fn test_load_corrupt_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("installed_plugins.json");
    std::fs::write(&path, "not valid json").unwrap();

    let reg = InstalledPluginsRegistry::load(&path);
    assert!(reg.is_empty());
}

#[test]
fn test_all_plugin_ids() {
    let mut reg = InstalledPluginsRegistry::empty();
    reg.add("alpha", make_entry("user", "1.0.0"));
    reg.add("beta", make_entry("user", "1.0.0"));

    let mut ids = reg.all_plugin_ids();
    ids.sort();
    assert_eq!(ids, vec!["alpha", "beta"]);
}
