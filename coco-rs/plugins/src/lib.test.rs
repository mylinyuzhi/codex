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

// ---------------------------------------------------------------------------
// Integration coverage — plugin lifecycle through to contribution
// aggregation. The tests above exercise individual surfaces (manifest
// parsing, manager register/enable, directory discovery); these tie
// the pieces together as the real loader/host does on startup.
// ---------------------------------------------------------------------------

#[test]
fn test_collect_all_contributions_aggregates_enabled_plugins_only() {
    // Two registered plugins, each contributing distinct skills + hooks.
    // Disable one; verify `collect_all_contributions` only sees the
    // enabled plugin's contributions. This is the load-bearing rule
    // for the host's command/skill/hook bridges.
    let mut mgr = PluginManager::new();
    mgr.register(LoadedPlugin {
        name: "alpha".into(),
        manifest: PluginManifest {
            name: "alpha".into(),
            version: Some("1.0.0".into()),
            description: "alpha plugin".into(),
            skills: vec!["alpha-skill-1".into(), "alpha-skill-2".into()],
            hooks: {
                let mut h = HashMap::new();
                h.insert(
                    "before_tool".into(),
                    serde_json::json!({"command": "alpha-pre"}),
                );
                h
            },
            mcp_servers: {
                let mut m = HashMap::new();
                m.insert(
                    "alpha-server".into(),
                    serde_json::json!({"url": "http://alpha"}),
                );
                m
            },
        },
        path: "/tmp/alpha".into(),
        source: PluginSource::User,
        enabled: true,
    });
    mgr.register(LoadedPlugin {
        name: "beta".into(),
        manifest: PluginManifest {
            name: "beta".into(),
            version: None,
            description: "beta plugin".into(),
            skills: vec!["beta-skill".into()],
            hooks: {
                let mut h = HashMap::new();
                h.insert(
                    "after_tool".into(),
                    serde_json::json!({"command": "beta-post"}),
                );
                h
            },
            mcp_servers: HashMap::new(),
        },
        path: "/tmp/beta".into(),
        source: PluginSource::Project,
        enabled: true,
    });

    // Both enabled — union view.
    let c_all = collect_all_contributions(&mgr);
    assert_eq!(c_all.skills.len(), 3, "expected alpha×2 + beta×1 skills");
    assert!(c_all.skills.iter().any(|s| s == "alpha-skill-1"));
    assert!(c_all.skills.iter().any(|s| s == "beta-skill"));
    assert_eq!(c_all.hooks.len(), 2, "alpha + beta hooks");
    assert_eq!(c_all.mcp_servers.len(), 1);
    assert!(c_all.mcp_servers.contains_key("alpha-server"));

    // Disable beta — its contributions drop out of the union.
    assert!(mgr.disable("beta"));
    let c_alpha_only = collect_all_contributions(&mgr);
    assert_eq!(c_alpha_only.skills.len(), 2);
    assert!(
        c_alpha_only.skills.iter().all(|s| s.starts_with("alpha-")),
        "disabled plugin's skills must not surface, got {:?}",
        c_alpha_only.skills,
    );
    assert_eq!(c_alpha_only.hooks.len(), 1);
    // Re-enable — union view returns.
    assert!(mgr.enable("beta"));
    let c_restored = collect_all_contributions(&mgr);
    assert_eq!(c_restored.skills.len(), 3);
}

#[test]
fn test_get_plugin_dirs_includes_both_config_and_project() {
    // The host calls `get_plugin_dirs(config_dir, project_dir)` at
    // startup — the result is the loader's input. Verifies both
    // user-level (`<config_dir>/plugins/*/`) and project-level
    // (`<project_dir>/.claude/plugins/*/`) directories are surfaced.
    let tmp = tempfile::tempdir().expect("tempdir");
    let config_dir = tmp.path().join("config");
    let project_dir = tmp.path().join("project");
    let user_plugin = config_dir.join("plugins").join("user-plug");
    let proj_plugin = project_dir
        .join(".claude")
        .join("plugins")
        .join("proj-plug");
    std::fs::create_dir_all(&user_plugin).expect("mkdir user");
    std::fs::create_dir_all(&proj_plugin).expect("mkdir proj");

    let dirs = get_plugin_dirs(&config_dir, &project_dir);
    assert!(
        dirs.iter().any(|p| p == &user_plugin),
        "user plugin dir missing — got {dirs:?}",
    );
    assert!(
        dirs.iter().any(|p| p == &proj_plugin),
        "project plugin dir missing — got {dirs:?}",
    );
}

#[test]
fn test_get_plugin_dirs_handles_missing_dirs() {
    // No plugin dirs on disk — function must not error, just return empty.
    let tmp = tempfile::tempdir().expect("tempdir");
    let dirs = get_plugin_dirs(
        &tmp.path().join("nope-config"),
        &tmp.path().join("nope-project"),
    );
    assert!(
        dirs.is_empty(),
        "expected empty list when neither plugin dir exists, got {dirs:?}",
    );
}

#[test]
fn test_load_from_dirs_skips_malformed_manifest() {
    // A directory with a syntactically broken PLUGIN.toml must be
    // skipped, not abort the whole discovery. Sibling valid plugins
    // still load.
    let tmp = tempfile::tempdir().expect("tempdir");
    let good_dir = tmp.path().join("good-plug");
    let bad_dir = tmp.path().join("bad-plug");
    std::fs::create_dir_all(&good_dir).expect("mkdir good");
    std::fs::create_dir_all(&bad_dir).expect("mkdir bad");
    std::fs::write(
        good_dir.join("PLUGIN.toml"),
        "name = \"good\"\ndescription = \"loads cleanly\"\n",
    )
    .expect("write good");
    std::fs::write(
        bad_dir.join("PLUGIN.toml"),
        "this is not valid TOML at all === [[",
    )
    .expect("write bad");

    let mut mgr = PluginManager::new();
    mgr.load_from_dirs(&[good_dir, bad_dir]);
    assert_eq!(
        mgr.len(),
        1,
        "expected only the well-formed plugin to load, got {} plugins",
        mgr.len(),
    );
    assert!(mgr.get("good").is_some());
}

#[test]
fn test_contributions_includes_directory_discovered_skills() {
    // Skills can be contributed via two routes: listed in the manifest
    // *or* discovered by walking the plugin dir's `skills/` for `*.md`
    // files. The latter is the conventional route — tests it here so
    // a regression in `discover_dir_contributions` is caught.
    let tmp = tempfile::tempdir().expect("tempdir");
    let plugin_root = tmp.path().join("dir-skills-plug");
    let skills_dir = plugin_root.join("skills");
    std::fs::create_dir_all(&skills_dir).expect("mkdir skills");
    std::fs::write(skills_dir.join("review.md"), "# Review skill\n").expect("write skill");
    std::fs::write(skills_dir.join("plan.md"), "# Plan skill\n").expect("write skill");
    // A non-markdown file in the same dir must be ignored.
    std::fs::write(skills_dir.join("readme.txt"), "ignore me").expect("write ignore");

    let plugin = LoadedPlugin {
        name: "dir-skills".into(),
        manifest: PluginManifest {
            name: "dir-skills".into(),
            version: None,
            description: "skills via dir scan".into(),
            skills: Vec::new(), // none in manifest — dir scan fills these in
            hooks: HashMap::new(),
            mcp_servers: HashMap::new(),
        },
        path: plugin_root,
        source: PluginSource::User,
        enabled: true,
    };

    let c = plugin.contributions();
    assert_eq!(
        c.skills.len(),
        2,
        "expected 2 *.md skills picked up, got {:?}",
        c.skills,
    );
    assert!(c.skills.iter().any(|s| s == "review"));
    assert!(c.skills.iter().any(|s| s == "plan"));
    assert!(
        !c.skills.iter().any(|s| s == "readme"),
        "non-markdown files must not contribute as skills",
    );
}
