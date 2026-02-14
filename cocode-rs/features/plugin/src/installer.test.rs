use super::*;

use crate::marketplace_types::MarketplaceManifest;
use crate::marketplace_types::MarketplacePluginEntry;
use crate::marketplace_types::MarketplacePluginSource;

fn setup_marketplace_with_plugin(base: &std::path::Path) -> PathBuf {
    let plugins_dir = base.join("plugins-home");

    // Create marketplace directory
    let market_dir = base.join("my-market");
    std::fs::create_dir_all(&market_dir).unwrap();

    // Create marketplace.json
    let manifest = MarketplaceManifest {
        name: "my-market".to_string(),
        description: None,
        plugins: vec![MarketplacePluginEntry {
            name: "test-plugin".to_string(),
            description: Some("Test plugin".to_string()),
            version: Some("1.0.0".to_string()),
            source: MarketplacePluginSource::RelativePath("./plugins/test-plugin".to_string()),
        }],
    };
    std::fs::write(
        market_dir.join("marketplace.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    // Create plugin directory
    let plugin_dir = market_dir.join("plugins").join("test-plugin");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(
        plugin_dir.join("plugin.json"),
        r#"{
  "plugin": {
    "name": "test-plugin",
    "version": "1.0.0",
    "description": "A test plugin"
  },
  "contributions": {
    "skills": ["skills/"]
  }
}"#,
    )
    .unwrap();

    // Create a skill
    let skill_dir = plugin_dir.join("skills").join("greet");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: greet\ndescription: Greet the user\n---\nSay hello!\n",
    )
    .unwrap();

    plugins_dir
}

#[tokio::test]
async fn test_install_and_uninstall() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins_dir = setup_marketplace_with_plugin(tmp.path());

    // Add marketplace
    let marketplace = MarketplaceManager::new(plugins_dir.clone());
    let market_dir = tmp.path().join("my-market");
    marketplace
        .add_source(MarketplaceSource::Directory { path: market_dir })
        .await
        .unwrap();

    // Install
    let installer = PluginInstaller::new(plugins_dir.clone());
    let result = installer
        .install("test-plugin", PluginScope::User)
        .await
        .unwrap();

    assert_eq!(result.plugin_id, "test-plugin");
    assert_eq!(result.version, "1.0.0");
    assert!(result.install_path.exists());

    // Verify registry
    let registry = InstalledPluginsRegistry::load(&plugins_dir.join("installed_plugins.json"));
    assert!(registry.get("test-plugin").is_some());

    // Verify settings
    let settings = PluginSettings::load(&plugins_dir.join("settings.json"));
    assert!(settings.is_enabled("test-plugin"));

    // List installed
    let installed = installer.list_installed();
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0].id, "test-plugin");

    // Uninstall
    installer
        .uninstall("test-plugin", PluginScope::User)
        .await
        .unwrap();

    let registry = InstalledPluginsRegistry::load(&plugins_dir.join("installed_plugins.json"));
    assert!(registry.is_empty());

    let installed = installer.list_installed();
    assert!(installed.is_empty());
}

#[tokio::test]
async fn test_install_not_found() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins_dir = tmp.path().join("plugins");

    let installer = PluginInstaller::new(plugins_dir);
    let result = installer.install("nonexistent", PluginScope::User).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_uninstall_not_installed() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins_dir = tmp.path().join("plugins");

    let installer = PluginInstaller::new(plugins_dir);
    let result = installer.uninstall("nonexistent", PluginScope::User).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_clone_source_url_invalid() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("url-plugin");

    // A non-existent URL should fail with a download error, not
    // the old "cannot be directly cloned" message.
    let result = clone_source(
        &MarketplaceSource::Url {
            url: "file:///nonexistent/plugin.tar.gz".to_string(),
        },
        &target,
    )
    .await;

    assert!(result.is_err());
    let err_msg = format!("{}", result.unwrap_err());
    // Should show a download failure, not the old "cannot be directly cloned" message
    assert!(
        err_msg.contains("download") || err_msg.contains("fetch") || err_msg.contains("curl"),
        "Expected download-related error, got: {err_msg}"
    );
}

#[test]
fn test_list_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let installer = PluginInstaller::new(tmp.path().to_path_buf());
    assert!(installer.list_installed().is_empty());
}
