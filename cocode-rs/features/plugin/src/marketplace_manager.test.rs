use super::*;

fn setup_marketplace_dir(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    let manifest = MarketplaceManifest {
        name: "test-market".to_string(),
        description: Some("Test marketplace".to_string()),
        plugins: vec![MarketplacePluginEntry {
            name: "hello".to_string(),
            description: Some("Hello plugin".to_string()),
            version: Some("0.1.0".to_string()),
            source: crate::marketplace_types::MarketplacePluginSource::RelativePath(
                "./plugins/hello".to_string(),
            ),
        }],
    };
    let json = serde_json::to_string_pretty(&manifest).unwrap();
    std::fs::write(dir.join("marketplace.json"), json).unwrap();
}

#[tokio::test]
async fn test_add_directory_source() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins_dir = tmp.path().join("plugins");
    let market_dir = tmp.path().join("my-marketplace");

    setup_marketplace_dir(&market_dir);

    let manager = MarketplaceManager::new(plugins_dir);
    let name = manager
        .add_source(MarketplaceSource::Directory {
            path: market_dir.clone(),
        })
        .await
        .unwrap();

    assert_eq!(name, "my-marketplace");

    let list = manager.list();
    assert_eq!(list.len(), 1);
    assert!(list.contains_key("my-marketplace"));
}

#[tokio::test]
async fn test_add_duplicate_source() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins_dir = tmp.path().join("plugins");
    let market_dir = tmp.path().join("my-marketplace");

    setup_marketplace_dir(&market_dir);

    let manager = MarketplaceManager::new(plugins_dir);
    manager
        .add_source(MarketplaceSource::Directory {
            path: market_dir.clone(),
        })
        .await
        .unwrap();

    let result = manager
        .add_source(MarketplaceSource::Directory {
            path: market_dir.clone(),
        })
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_remove_source() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins_dir = tmp.path().join("plugins");
    let market_dir = tmp.path().join("test-market");

    setup_marketplace_dir(&market_dir);

    let manager = MarketplaceManager::new(plugins_dir);
    manager
        .add_source(MarketplaceSource::Directory { path: market_dir })
        .await
        .unwrap();

    manager.remove_source("test-market").await.unwrap();
    assert!(manager.list().is_empty());
}

#[tokio::test]
async fn test_remove_nonexistent_source() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins_dir = tmp.path().join("plugins");
    let manager = MarketplaceManager::new(plugins_dir);

    let result = manager.remove_source("nonexistent").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_find_plugin() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins_dir = tmp.path().join("plugins");
    let market_dir = tmp.path().join("test-market");

    setup_marketplace_dir(&market_dir);

    let manager = MarketplaceManager::new(plugins_dir);
    manager
        .add_source(MarketplaceSource::Directory { path: market_dir })
        .await
        .unwrap();

    // Find by name
    let found = manager.find_plugin("hello").unwrap();
    assert_eq!(found.entry.name, "hello");
    assert_eq!(found.marketplace_name, "test-market");

    // Find by name@marketplace
    let found = manager.find_plugin("hello@test-market").unwrap();
    assert_eq!(found.entry.name, "hello");

    // Not found
    assert!(manager.find_plugin("nonexistent").is_none());
}

#[tokio::test]
async fn test_find_plugin_in_marketplace() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins_dir = tmp.path().join("plugins");
    let market_dir = tmp.path().join("test-market");

    setup_marketplace_dir(&market_dir);

    let manager = MarketplaceManager::new(plugins_dir);
    manager
        .add_source(MarketplaceSource::Directory { path: market_dir })
        .await
        .unwrap();

    let found = manager
        .find_plugin_in_marketplace("hello", "test-market")
        .unwrap();
    assert_eq!(found.entry.name, "hello");

    assert!(
        manager
            .find_plugin_in_marketplace("nonexistent", "test-market")
            .is_none()
    );
}

#[test]
fn test_empty_list() {
    let tmp = tempfile::tempdir().unwrap();
    let manager = MarketplaceManager::new(tmp.path().to_path_buf());
    assert!(manager.list().is_empty());
}

#[test]
fn test_should_refresh_disabled() {
    let km = KnownMarketplace {
        source: MarketplaceSource::Directory {
            path: std::path::PathBuf::from("/tmp/test"),
        },
        install_location: std::path::PathBuf::from("/tmp/install"),
        last_updated: Some(chrono::Utc::now().to_rfc3339()),
        auto_update: false,
    };
    assert!(!should_refresh(&km));
}

#[test]
fn test_should_refresh_recent() {
    let km = KnownMarketplace {
        source: MarketplaceSource::Directory {
            path: std::path::PathBuf::from("/tmp/test"),
        },
        install_location: std::path::PathBuf::from("/tmp/install"),
        last_updated: Some(chrono::Utc::now().to_rfc3339()),
        auto_update: true,
    };
    // Updated just now — should not refresh
    assert!(!should_refresh(&km));
}

#[test]
fn test_should_refresh_stale() {
    let old = chrono::Utc::now() - chrono::Duration::hours(25);
    let km = KnownMarketplace {
        source: MarketplaceSource::Directory {
            path: std::path::PathBuf::from("/tmp/test"),
        },
        install_location: std::path::PathBuf::from("/tmp/install"),
        last_updated: Some(old.to_rfc3339()),
        auto_update: true,
    };
    assert!(should_refresh(&km));
}

#[test]
fn test_should_refresh_no_timestamp() {
    let km = KnownMarketplace {
        source: MarketplaceSource::Directory {
            path: std::path::PathBuf::from("/tmp/test"),
        },
        install_location: std::path::PathBuf::from("/tmp/install"),
        last_updated: None,
        auto_update: true,
    };
    assert!(should_refresh(&km));
}

#[tokio::test]
async fn test_auto_refresh_stale_no_stale() {
    let tmp = tempfile::tempdir().unwrap();
    let plugins_dir = tmp.path().join("plugins");
    let market_dir = tmp.path().join("fresh-market");

    setup_marketplace_dir(&market_dir);

    let manager = MarketplaceManager::new(plugins_dir);
    manager
        .add_source(MarketplaceSource::Directory {
            path: market_dir.clone(),
        })
        .await
        .unwrap();

    // auto_update is false by default, so nothing should be stale
    let refreshed = manager.auto_refresh_stale().await.unwrap();
    assert!(refreshed.is_empty());
}
