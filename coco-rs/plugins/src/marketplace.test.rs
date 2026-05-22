use pretty_assertions::assert_eq;

use super::*;
use crate::schemas::PluginAuthor;
use crate::schemas::PluginSource;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_marketplace(name: &str, plugins: Vec<PluginMarketplaceEntry>) -> PluginMarketplace {
    PluginMarketplace {
        name: name.to_string(),
        owner: PluginAuthor {
            name: "Test Owner".to_string(),
            email: None,
            url: None,
        },
        plugins,
        force_remove_deleted_plugins: None,
        metadata: None,
        allow_cross_marketplace_dependencies_on: None,
    }
}

fn make_entry(name: &str, desc: Option<&str>, tags: Option<Vec<&str>>) -> PluginMarketplaceEntry {
    PluginMarketplaceEntry {
        name: name.to_string(),
        source: PluginSource::RelativePath(format!("./plugins/{name}")),
        version: Some("1.0.0".to_string()),
        description: desc.map(String::from),
        author: None,
        category: None,
        tags: tags.map(|t| t.into_iter().map(String::from).collect()),
        strict: true,
        homepage: None,
        license: None,
        keywords: None,
        dependencies: None,
    }
}

// ---------------------------------------------------------------------------
// search_plugins
// ---------------------------------------------------------------------------

#[test]
fn test_search_plugins_by_name() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut mgr = MarketplaceManager::new(tmp.path().to_path_buf());

    let marketplace = make_marketplace(
        "test-mkt",
        vec![
            make_entry("code-formatter", Some("Formats code"), None),
            make_entry("linter", Some("Lints code"), None),
            make_entry("deployer", Some("Deploys apps"), None),
        ],
    );
    mgr.marketplace_cache
        .insert("test-mkt".to_string(), marketplace);

    let results = mgr.search_plugins("formatter");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "code-formatter");
    assert_eq!(results[0].marketplace, "test-mkt");
}

#[test]
fn test_search_plugins_by_description() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut mgr = MarketplaceManager::new(tmp.path().to_path_buf());

    let marketplace = make_marketplace(
        "mkt",
        vec![
            make_entry("alpha", Some("Kubernetes deployment tool"), None),
            make_entry("beta", Some("Database migration helper"), None),
        ],
    );
    mgr.marketplace_cache.insert("mkt".to_string(), marketplace);

    let results = mgr.search_plugins("kubernetes");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "alpha");
}

#[test]
fn test_search_plugins_by_tag() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut mgr = MarketplaceManager::new(tmp.path().to_path_buf());

    let marketplace = make_marketplace(
        "mkt",
        vec![
            make_entry("tagged-plugin", None, Some(vec!["cloud", "aws"])),
            make_entry("other", None, Some(vec!["local"])),
        ],
    );
    mgr.marketplace_cache.insert("mkt".to_string(), marketplace);

    let results = mgr.search_plugins("aws");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "tagged-plugin");
}

#[test]
fn test_search_plugins_empty_query_returns_all() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut mgr = MarketplaceManager::new(tmp.path().to_path_buf());

    let marketplace = make_marketplace(
        "mkt",
        vec![make_entry("a", None, None), make_entry("b", None, None)],
    );
    mgr.marketplace_cache.insert("mkt".to_string(), marketplace);

    let results = mgr.search_plugins("");
    assert_eq!(results.len(), 2);
}

// ---------------------------------------------------------------------------
// MarketplacePlugin::from_entry
// ---------------------------------------------------------------------------

#[test]
fn test_marketplace_plugin_from_entry() {
    let entry = make_entry("my-plugin", Some("Does things"), Some(vec!["tool"]));
    let plugin = MarketplacePlugin::from_entry(&entry, "my-mkt");

    assert_eq!(plugin.name, "my-plugin");
    assert_eq!(plugin.version.as_deref(), Some("1.0.0"));
    assert_eq!(plugin.description.as_deref(), Some("Does things"));
    assert_eq!(plugin.marketplace, "my-mkt");
    assert_eq!(plugin.tags, vec!["tool".to_string()]);
    assert_eq!(plugin.downloads, 0);
}

// ---------------------------------------------------------------------------
// Known marketplaces I/O
// ---------------------------------------------------------------------------

#[test]
fn test_register_and_load_marketplace() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut mgr = MarketplaceManager::new(tmp.path().to_path_buf());

    mgr.register_marketplace(
        "my-marketplace",
        MarketplaceSource::Url {
            url: "https://example.com/marketplace.json".to_string(),
            headers: None,
        },
        "/cache/my-marketplace.json",
    )
    .expect("register");

    let known = mgr.load_known_marketplaces();
    assert!(known.contains_key("my-marketplace"));
    let entry = &known["my-marketplace"];
    assert_eq!(entry.install_location, "/cache/my-marketplace.json");
}

#[test]
fn test_register_marketplace_rejects_reserved_name() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut mgr = MarketplaceManager::new(tmp.path().to_path_buf());

    let result = mgr.register_marketplace(
        "inline",
        MarketplaceSource::Url {
            url: "https://example.com".to_string(),
            headers: None,
        },
        "/cache/inline.json",
    );

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("reserved"));
}

// ---------------------------------------------------------------------------
// install_plugin (local source)
// ---------------------------------------------------------------------------

#[test]
fn test_install_plugin_local_source() {
    let tmp = tempfile::tempdir().expect("tempdir");

    // Create a "marketplace" directory with a plugin source.
    let mkt_dir = tmp.path().join("marketplaces").join("test-mkt");
    let plugin_src = mkt_dir.join("plugins").join("my-plugin");
    std::fs::create_dir_all(&plugin_src).expect("mkdir");
    std::fs::write(
        plugin_src.join("PLUGIN.toml"),
        "name = \"my-plugin\"\nversion = \"1.0.0\"\n",
    )
    .expect("write");

    // Register the marketplace.
    let mut mgr = MarketplaceManager::new(tmp.path().to_path_buf());
    mgr.register_marketplace(
        "test-mkt",
        MarketplaceSource::Directory {
            path: mkt_dir.display().to_string(),
        },
        &mkt_dir.display().to_string(),
    )
    .expect("register");

    let entry = make_entry("my-plugin", Some("A plugin"), None);
    let install_path = mgr
        .install_plugin("test-mkt", &entry, PluginScope::User)
        .expect("install");

    assert!(install_path.exists());
    assert!(install_path.join("PLUGIN.toml").exists());
}

// ---------------------------------------------------------------------------
// InstallCountsCache
// ---------------------------------------------------------------------------

#[test]
fn test_install_counts_cache_roundtrip() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("counts.json");

    let cache = InstallCountsCache {
        version: 1,
        fetched_at: "2024-01-15T10:00:00Z".to_string(),
        counts: vec![
            InstallCountEntry {
                plugin: "foo@mkt".to_string(),
                unique_installs: 42,
            },
            InstallCountEntry {
                plugin: "bar@mkt".to_string(),
                unique_installs: 100,
            },
        ],
    };

    cache.save(&path).expect("save");
    let loaded = InstallCountsCache::load(&path).expect("load");

    assert_eq!(loaded.counts.len(), 2);
    assert_eq!(loaded.get_count("foo@mkt"), Some(42));
    assert_eq!(loaded.get_count("bar@mkt"), Some(100));
    assert_eq!(loaded.get_count("missing@mkt"), None);
}

// ---------------------------------------------------------------------------
// PluginRecommendation
// ---------------------------------------------------------------------------

#[test]
fn test_plugin_recommendation_serde() {
    let rec = PluginRecommendation {
        plugin_id: "my-plugin@official".to_string(),
        plugin_name: "my-plugin".to_string(),
        marketplace_name: "official".to_string(),
        plugin_description: Some("A great plugin".to_string()),
        source_command: "docker build".to_string(),
    };

    let json = serde_json::to_string(&rec).expect("serialize");
    let deserialized: PluginRecommendation = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(deserialized, rec);
}

// ---------------------------------------------------------------------------
// get_plugin_by_id
// ---------------------------------------------------------------------------

#[test]
fn test_get_plugin_by_id() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut mgr = MarketplaceManager::new(tmp.path().to_path_buf());

    let marketplace = make_marketplace(
        "mkt",
        vec![
            make_entry("alpha", Some("Plugin A"), None),
            make_entry("beta", Some("Plugin B"), None),
        ],
    );
    mgr.marketplace_cache.insert("mkt".to_string(), marketplace);

    let found = mgr.get_plugin_by_id("alpha@mkt");
    assert!(found.is_some());
    let (plugin, entry) = found.unwrap();
    assert_eq!(plugin.name, "alpha");
    assert_eq!(entry.name, "alpha");

    assert!(mgr.get_plugin_by_id("missing@mkt").is_none());
    assert!(mgr.get_plugin_by_id("alpha@other").is_none());
}

// ---------------------------------------------------------------------------
// detect_delisted_plugins
// ---------------------------------------------------------------------------

fn make_installed_manager(
    dir: &Path,
    plugin_ids: &[(&str, Option<&str>)],
) -> crate::loader::InstalledPluginsManager {
    let path = dir.join("installed_plugins.json");
    let mut mgr = crate::loader::InstalledPluginsManager::load(path).expect("load");
    for (id, version) in plugin_ids {
        mgr.record_installation(
            id,
            crate::schemas::PluginInstallationEntry {
                scope: crate::schemas::PluginScope::User,
                project_path: None,
                install_path: format!("/cache/{id}"),
                version: version.map(String::from),
                installed_at: Some("2024-01-01T00:00:00Z".to_string()),
                last_updated: None,
                git_commit_sha: None,
            },
        );
    }
    mgr.save().expect("save");
    mgr
}

#[test]
fn test_detect_delisted_plugins_finds_removed() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let installed = make_installed_manager(
        tmp.path(),
        &[
            ("alpha@mkt", Some("1.0.0")),
            ("beta@mkt", Some("1.0.0")),
            ("gamma@mkt", Some("1.0.0")),
        ],
    );
    // Marketplace only lists alpha and gamma -- beta is delisted.
    let marketplace = make_marketplace(
        "mkt",
        vec![
            make_entry("alpha", None, None),
            make_entry("gamma", None, None),
        ],
    );

    let delisted = detect_delisted_plugins(&installed, &marketplace, "mkt");
    assert_eq!(delisted, vec!["beta@mkt"]);
}

#[test]
fn test_detect_delisted_plugins_ignores_other_marketplaces() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let installed = make_installed_manager(
        tmp.path(),
        &[("alpha@mkt", Some("1.0.0")), ("beta@other", Some("1.0.0"))],
    );
    let marketplace = make_marketplace("mkt", vec![make_entry("alpha", None, None)]);

    let delisted = detect_delisted_plugins(&installed, &marketplace, "mkt");
    assert!(delisted.is_empty());
}

#[test]
fn test_detect_delisted_plugins_empty_when_all_listed() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let installed = make_installed_manager(tmp.path(), &[("alpha@mkt", Some("1.0.0"))]);
    let marketplace = make_marketplace("mkt", vec![make_entry("alpha", None, None)]);

    let delisted = detect_delisted_plugins(&installed, &marketplace, "mkt");
    assert!(delisted.is_empty());
}

// ---------------------------------------------------------------------------
// flagged plugins I/O
// ---------------------------------------------------------------------------

#[test]
fn test_flagged_plugins_roundtrip() {
    let tmp = tempfile::tempdir().expect("tempdir");

    let flagged = vec![FlaggedPlugin {
        plugin_id: "removed@mkt".to_string(),
        flagged_at: "2024-06-01T12:00:00Z".to_string(),
        marketplace: "mkt".to_string(),
    }];
    save_flagged_plugins(tmp.path(), &flagged).expect("save");

    let loaded = load_flagged_plugins(tmp.path());
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].plugin_id, "removed@mkt");
    assert_eq!(loaded[0].marketplace, "mkt");
}

#[test]
fn test_load_flagged_plugins_missing_file() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let loaded = load_flagged_plugins(tmp.path());
    assert!(loaded.is_empty());
}

#[test]
fn test_flag_delisted_plugin_idempotent() {
    let tmp = tempfile::tempdir().expect("tempdir");

    flag_delisted_plugin(tmp.path(), "removed@mkt", "mkt").expect("first flag");
    flag_delisted_plugin(tmp.path(), "removed@mkt", "mkt").expect("second flag (no-op)");

    let loaded = load_flagged_plugins(tmp.path());
    assert_eq!(loaded.len(), 1);
}

// ---------------------------------------------------------------------------
// is_marketplace_auto_update
// ---------------------------------------------------------------------------

#[test]
fn test_auto_update_explicit_setting_overrides() {
    assert!(is_marketplace_auto_update(
        "claude-plugins-official",
        Some(true)
    ));
    assert!(!is_marketplace_auto_update(
        "claude-plugins-official",
        Some(false)
    ));
    assert!(is_marketplace_auto_update("random-mkt", Some(true)));
}

#[test]
fn test_auto_update_default_official_enabled() {
    // Official marketplace (not in NO_AUTO_UPDATE_OFFICIAL) defaults to true.
    assert!(is_marketplace_auto_update("claude-plugins-official", None));
    assert!(is_marketplace_auto_update("anthropic-marketplace", None));
}

#[test]
fn test_auto_update_default_knowledge_work_disabled() {
    assert!(!is_marketplace_auto_update("knowledge-work-plugins", None));
}

#[test]
fn test_auto_update_default_non_official_disabled() {
    assert!(!is_marketplace_auto_update("my-custom-mkt", None));
}

// ---------------------------------------------------------------------------
// check_plugin_updates
// ---------------------------------------------------------------------------

#[test]
fn test_check_plugin_updates_detects_version_mismatch() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let installed = make_installed_manager(tmp.path(), &[("alpha@mkt", Some("1.0.0"))]);
    // Marketplace has a newer version.
    let mut entry = make_entry("alpha", None, None);
    entry.version = Some("2.0.0".to_string());
    let marketplace = make_marketplace("mkt", vec![entry]);

    let checks = check_plugin_updates(&installed, &marketplace, "mkt");
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].plugin_id, "alpha@mkt");
    assert_eq!(checks[0].current_version.as_deref(), Some("1.0.0"));
    assert_eq!(checks[0].available_version.as_deref(), Some("2.0.0"));
    assert!(checks[0].needs_update);
}

#[test]
fn test_check_plugin_updates_no_update_when_same_version() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let installed = make_installed_manager(tmp.path(), &[("alpha@mkt", Some("1.0.0"))]);
    let marketplace = make_marketplace("mkt", vec![make_entry("alpha", None, None)]);

    let checks = check_plugin_updates(&installed, &marketplace, "mkt");
    assert_eq!(checks.len(), 1);
    assert!(!checks[0].needs_update);
}

#[test]
fn test_check_plugin_updates_skips_uninstalled() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let installed = make_installed_manager(tmp.path(), &[]);
    let marketplace = make_marketplace("mkt", vec![make_entry("alpha", None, None)]);

    let checks = check_plugin_updates(&installed, &marketplace, "mkt");
    assert!(checks.is_empty());
}

#[test]
fn test_check_plugin_updates_needs_update_when_no_local_version() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let installed = make_installed_manager(tmp.path(), &[("alpha@mkt", None)]);
    let marketplace = make_marketplace("mkt", vec![make_entry("alpha", None, None)]);

    let checks = check_plugin_updates(&installed, &marketplace, "mkt");
    assert_eq!(checks.len(), 1);
    assert!(checks[0].needs_update);
}
