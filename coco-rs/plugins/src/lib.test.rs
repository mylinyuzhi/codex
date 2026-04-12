use super::*;

#[test]
fn test_plugin_manager() {
    let mut mgr = PluginManager::new();
    mgr.register(LoadedPlugin {
        name: "test-plugin".into(),
        manifest: PluginManifest {
            name: "test-plugin".into(),
            version: Some("1.0.0".into()),
            description: "A test plugin".into(),
            skills: vec!["my-skill".into()],
            hooks: Default::default(),
            mcp_servers: Default::default(),
        },
        path: "/plugins/test".into(),
        source: PluginSource::User,
        enabled: true,
    });

    assert_eq!(mgr.len(), 1);
    assert_eq!(mgr.enabled().len(), 1);
    assert!(mgr.get("test-plugin").is_some());
}

#[test]
fn test_load_manifest_from_toml() {
    let toml_str = r#"
name = "my-plugin"
version = "0.1.0"
description = "Does things"
skills = ["skill1", "skill2"]

[hooks]
before_tool = { command = "echo hi" }

[mcp_servers]
my_server = { url = "http://localhost:3000" }
"#;
    let manifest: PluginManifest = toml::from_str(toml_str).expect("should parse TOML");
    assert_eq!(manifest.name, "my-plugin");
    assert_eq!(manifest.version.as_deref(), Some("0.1.0"));
    assert_eq!(manifest.description, "Does things");
    assert_eq!(manifest.skills.len(), 2);
    assert_eq!(manifest.hooks.len(), 1);
    assert!(manifest.hooks.contains_key("before_tool"));
    assert_eq!(manifest.mcp_servers.len(), 1);
}

#[test]
fn test_load_manifest_minimal() {
    let toml_str = r#"
name = "minimal"
description = "Bare minimum"
"#;
    let manifest: PluginManifest = toml::from_str(toml_str).expect("should parse TOML");
    assert_eq!(manifest.name, "minimal");
    assert!(manifest.version.is_none());
    assert!(manifest.skills.is_empty());
    assert!(manifest.hooks.is_empty());
    assert!(manifest.mcp_servers.is_empty());
}

#[test]
fn test_discover_plugins() {
    let tmp = tempfile::tempdir().expect("should create tempdir");

    // Create two plugin directories
    let plugin_a = tmp.path().join("plugin-a");
    let plugin_b = tmp.path().join("plugin-b");
    let empty_dir = tmp.path().join("no-plugin");
    std::fs::create_dir_all(&plugin_a).expect("mkdir");
    std::fs::create_dir_all(&plugin_b).expect("mkdir");
    std::fs::create_dir_all(&empty_dir).expect("mkdir");

    std::fs::write(
        plugin_a.join("PLUGIN.toml"),
        "name = \"alpha\"\ndescription = \"Plugin A\"\n",
    )
    .expect("write");

    std::fs::write(
        plugin_b.join("PLUGIN.toml"),
        "name = \"beta\"\ndescription = \"Plugin B\"\nskills = [\"s1\"]\n",
    )
    .expect("write");

    let dirs = vec![plugin_a, plugin_b, empty_dir];
    let plugins = discover_plugins(&dirs);

    assert_eq!(plugins.len(), 2);
    let names: Vec<&str> = plugins.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"alpha"));
    assert!(names.contains(&"beta"));
}

#[test]
fn test_load_from_dirs() {
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let plugin_dir = tmp.path().join("my-plugin");
    std::fs::create_dir_all(&plugin_dir).expect("mkdir");
    std::fs::write(
        plugin_dir.join("PLUGIN.toml"),
        "name = \"from-dir\"\ndescription = \"Loaded from dir\"\n",
    )
    .expect("write");

    let mut mgr = PluginManager::new();
    mgr.load_from_dirs(&[plugin_dir]);

    assert_eq!(mgr.len(), 1);
    let plugin = mgr.get("from-dir").expect("plugin should exist");
    assert_eq!(plugin.manifest.description, "Loaded from dir");
    assert!(plugin.enabled);
}

#[test]
fn test_enable_disable() {
    let mut mgr = PluginManager::new();
    mgr.register(LoadedPlugin {
        name: "toggle-me".into(),
        manifest: PluginManifest {
            name: "toggle-me".into(),
            version: None,
            description: "Togglable".into(),
            skills: vec![],
            hooks: Default::default(),
            mcp_servers: Default::default(),
        },
        path: "/tmp".into(),
        source: PluginSource::User,
        enabled: true,
    });

    assert_eq!(mgr.enabled().len(), 1);

    assert!(mgr.disable("toggle-me"));
    assert_eq!(mgr.enabled().len(), 0);
    assert!(!mgr.get("toggle-me").expect("exists").enabled);

    assert!(mgr.enable("toggle-me"));
    assert_eq!(mgr.enabled().len(), 1);
    assert!(mgr.get("toggle-me").expect("exists").enabled);

    // Non-existent plugin returns false
    assert!(!mgr.enable("nonexistent"));
    assert!(!mgr.disable("nonexistent"));
}

#[test]
fn test_load_plugin_manifest_from_file() {
    let tmp = tempfile::tempdir().expect("should create tempdir");
    let manifest_path = tmp.path().join("PLUGIN.toml");
    std::fs::write(
        &manifest_path,
        "name = \"file-loaded\"\ndescription = \"From file\"\nversion = \"2.0.0\"\n",
    )
    .expect("write");

    let manifest = load_plugin_manifest(&manifest_path).expect("should load manifest");
    assert_eq!(manifest.name, "file-loaded");
    assert_eq!(manifest.version.as_deref(), Some("2.0.0"));
}
