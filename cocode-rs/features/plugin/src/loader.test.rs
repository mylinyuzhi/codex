use super::*;
use std::fs;

#[test]
fn test_scan_empty_dir() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let loader = PluginLoader::new();
    let results = loader.scan(tmp.path());
    assert!(results.is_empty());
}

#[test]
fn test_scan_finds_plugin() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let plugin_dir = tmp.path().join("my-plugin");
    fs::create_dir_all(&plugin_dir).expect("mkdir");
    fs::write(
        plugin_dir.join("plugin.json"),
        r#"{
  "plugin": {
    "name": "my-plugin",
    "version": "1.0.0",
    "description": "Test plugin"
  }
}"#,
    )
    .expect("write");

    let loader = PluginLoader::new();
    let results = loader.scan(tmp.path());
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], plugin_dir);
}

#[test]
fn test_load_plugin() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let plugin_dir = tmp.path().join("test-plugin");
    fs::create_dir_all(&plugin_dir).expect("mkdir");
    fs::write(
        plugin_dir.join("plugin.json"),
        r#"{
  "plugin": {
    "name": "test-plugin",
    "version": "0.1.0",
    "description": "A test plugin"
  }
}"#,
    )
    .expect("write");

    let loader = PluginLoader::new();
    let plugin = loader
        .load(&plugin_dir, PluginScope::Project)
        .expect("load");

    assert_eq!(plugin.name(), "test-plugin");
    assert_eq!(plugin.version(), "0.1.0");
    assert_eq!(plugin.scope, PluginScope::Project);
}

#[test]
fn test_load_plugin_with_skills() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let plugin_dir = tmp.path().join("skill-plugin");
    let skills_dir = plugin_dir.join("skills").join("my-skill");
    fs::create_dir_all(&skills_dir).expect("mkdir");

    fs::write(
        plugin_dir.join("plugin.json"),
        r#"{
  "plugin": {
    "name": "skill-plugin",
    "version": "0.1.0",
    "description": "Plugin with skills"
  },
  "contributions": {
    "skills": ["skills/"]
  }
}"#,
    )
    .expect("write");

    fs::write(
        skills_dir.join("SKILL.md"),
        "---\nname: my-skill\ndescription: A skill from a plugin\n---\nDo something\n",
    )
    .expect("write skill");

    let loader = PluginLoader::new();
    let plugin = loader.load(&plugin_dir, PluginScope::User).expect("load");

    assert_eq!(plugin.name(), "skill-plugin");
    assert_eq!(plugin.contributions.len(), 1);
    assert!(plugin.contributions[0].is_skill());
    assert_eq!(plugin.contributions[0].name(), "my-skill");
}

#[test]
fn test_load_nonexistent_plugin() {
    let loader = PluginLoader::new();
    let result = loader.load(Path::new("/nonexistent"), PluginScope::Project);
    assert!(result.is_err());
}

#[test]
fn test_auto_discover_skills_dir() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let plugin_dir = tmp.path().join("auto-plugin");
    let skills_dir = plugin_dir.join("skills").join("my-skill");
    fs::create_dir_all(&skills_dir).expect("mkdir");

    // Manifest without explicit skills declaration
    fs::write(
        plugin_dir.join("plugin.json"),
        r#"{
  "plugin": {
    "name": "auto-plugin",
    "version": "0.1.0",
    "description": "Plugin with auto-discovered skills"
  }
}"#,
    )
    .expect("write manifest");

    fs::write(
        skills_dir.join("SKILL.md"),
        "---\nname: auto-skill\ndescription: Auto-discovered skill\n---\nDo something\n",
    )
    .expect("write skill");

    let loader = PluginLoader::new();
    let plugin = loader.load(&plugin_dir, PluginScope::User).expect("load");

    assert_eq!(plugin.name(), "auto-plugin");
    assert_eq!(plugin.contributions.len(), 1);
    assert!(plugin.contributions[0].is_skill());
    assert_eq!(plugin.contributions[0].name(), "auto-skill");
}

#[test]
fn test_scan_finds_hidden_plugin_dir() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let plugin_dir = tmp.path().join("hidden-plugin");
    let hidden_dir = plugin_dir.join(".cocode-plugin");
    fs::create_dir_all(&hidden_dir).expect("mkdir");

    fs::write(
        hidden_dir.join("plugin.json"),
        r#"{
  "plugin": {
    "name": "hidden-plugin",
    "version": "1.0.0",
    "description": "Plugin using .cocode-plugin/"
  }
}"#,
    )
    .expect("write");

    let loader = PluginLoader::new();
    let results = loader.scan(tmp.path());
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], plugin_dir);
}
