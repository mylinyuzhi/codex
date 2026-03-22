use super::*;

#[test]
fn test_github_source_serde() {
    let source = MarketplaceSource::Github {
        repo: "owner/repo".to_string(),
        git_ref: Some("main".to_string()),
    };
    let json = serde_json::to_string(&source).unwrap();
    assert!(json.contains("\"source\":\"github\""));
    assert!(json.contains("\"repo\":\"owner/repo\""));
    let deserialized: MarketplaceSource = serde_json::from_str(&json).unwrap();
    if let MarketplaceSource::Github { repo, git_ref } = deserialized {
        assert_eq!(repo, "owner/repo");
        assert_eq!(git_ref.unwrap(), "main");
    } else {
        panic!("Expected Github variant");
    }
}

#[test]
fn test_git_source_serde() {
    let source = MarketplaceSource::Git {
        url: "https://example.com/repo.git".to_string(),
        git_ref: None,
    };
    let json = serde_json::to_string(&source).unwrap();
    let deserialized: MarketplaceSource = serde_json::from_str(&json).unwrap();
    if let MarketplaceSource::Git { url, git_ref } = deserialized {
        assert_eq!(url, "https://example.com/repo.git");
        assert!(git_ref.is_none());
    } else {
        panic!("Expected Git variant");
    }
}

#[test]
fn test_directory_source_serde() {
    let source = MarketplaceSource::Directory {
        path: PathBuf::from("/tmp/marketplace"),
    };
    let json = serde_json::to_string(&source).unwrap();
    let deserialized: MarketplaceSource = serde_json::from_str(&json).unwrap();
    if let MarketplaceSource::Directory { path } = deserialized {
        assert_eq!(path, PathBuf::from("/tmp/marketplace"));
    } else {
        panic!("Expected Directory variant");
    }
}

#[test]
fn test_marketplace_manifest_serde() {
    let manifest = MarketplaceManifest {
        name: "test-market".to_string(),
        description: Some("A test marketplace".to_string()),
        plugins: vec![
            MarketplacePluginEntry {
                name: "hello".to_string(),
                description: Some("Hello plugin".to_string()),
                version: Some("0.1.0".to_string()),
                source: MarketplacePluginSource::RelativePath("./plugins/hello".to_string()),
            },
            MarketplacePluginEntry {
                name: "remote-plugin".to_string(),
                description: None,
                version: None,
                source: MarketplacePluginSource::Remote(MarketplaceSource::Github {
                    repo: "user/plugin".to_string(),
                    git_ref: None,
                }),
            },
        ],
    };

    let json = serde_json::to_string_pretty(&manifest).unwrap();
    let deserialized: MarketplaceManifest = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.name, "test-market");
    assert_eq!(deserialized.plugins.len(), 2);

    if let MarketplacePluginSource::RelativePath(path) = &deserialized.plugins[0].source {
        assert_eq!(path, "./plugins/hello");
    } else {
        panic!("Expected RelativePath");
    }
}

#[test]
fn test_known_marketplace_serde() {
    let km = KnownMarketplace {
        source: MarketplaceSource::Github {
            repo: "org/marketplace".to_string(),
            git_ref: Some("v1".to_string()),
        },
        install_location: PathBuf::from("/home/user/.cocode/plugins/marketplaces/org-marketplace"),
        last_updated: Some("2025-01-01T00:00:00Z".to_string()),
        auto_update: true,
    };

    let json = serde_json::to_string(&km).unwrap();
    let deserialized: KnownMarketplace = serde_json::from_str(&json).unwrap();
    assert!(deserialized.auto_update);
    assert!(deserialized.last_updated.is_some());
}

#[test]
fn test_derive_name_github() {
    let source = MarketplaceSource::Github {
        repo: "owner/my-plugins".to_string(),
        git_ref: None,
    };
    assert_eq!(source.derive_name(), "owner-my-plugins");
}

#[test]
fn test_derive_name_git() {
    let source = MarketplaceSource::Git {
        url: "https://example.com/plugins.git".to_string(),
        git_ref: None,
    };
    assert_eq!(source.derive_name(), "plugins");
}

#[test]
fn test_derive_name_directory() {
    let source = MarketplaceSource::Directory {
        path: PathBuf::from("/tmp/my-marketplace"),
    };
    assert_eq!(source.derive_name(), "my-marketplace");
}

#[test]
fn test_auto_update_default_false() {
    let json = r#"{
        "source": {"source": "directory", "path": "/tmp"},
        "install_location": "/tmp/install"
    }"#;
    let km: KnownMarketplace = serde_json::from_str(json).unwrap();
    assert!(!km.auto_update);
}
