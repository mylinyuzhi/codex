use super::*;

#[test]
fn diff_missing() {
    let mut declared = HashMap::new();
    declared.insert(
        "m1".to_string(),
        DeclaredMarketplace {
            name: "m1".into(),
            source: MarketplaceSourceRef::Url {
                url: "https://x".into(),
            },
            source_is_fallback: false,
        },
    );
    let materialized: KnownMarketplacesFile = HashMap::new();
    let diff = diff_marketplaces(&declared, &materialized);
    assert_eq!(diff.missing, vec!["m1".to_string()]);
}

#[test]
fn diff_up_to_date() {
    let source = MarketplaceSourceRef::Url {
        url: "https://x".into(),
    };
    let mut declared = HashMap::new();
    declared.insert(
        "m1".to_string(),
        DeclaredMarketplace {
            name: "m1".into(),
            source: source.clone(),
            source_is_fallback: false,
        },
    );
    let mut materialized = HashMap::new();
    materialized.insert(
        "m1".to_string(),
        MaterializedMarketplace {
            source,
            install_location: PathBuf::from("/cache/m1"),
            last_synced: None,
        },
    );
    let diff = diff_marketplaces(&declared, &materialized);
    assert_eq!(diff.up_to_date, vec!["m1".to_string()]);
}

#[test]
fn diff_source_changed() {
    let mut declared = HashMap::new();
    declared.insert(
        "m1".to_string(),
        DeclaredMarketplace {
            name: "m1".into(),
            source: MarketplaceSourceRef::Url {
                url: "https://new".into(),
            },
            source_is_fallback: false,
        },
    );
    let mut materialized = HashMap::new();
    materialized.insert(
        "m1".to_string(),
        MaterializedMarketplace {
            source: MarketplaceSourceRef::Url {
                url: "https://old".into(),
            },
            install_location: PathBuf::from("/cache/m1"),
            last_synced: None,
        },
    );
    let diff = diff_marketplaces(&declared, &materialized);
    assert_eq!(diff.source_changed.len(), 1);
}

#[test]
fn diff_fallback_source_skips_compare() {
    // Fallback intent: even if material source differs, mark up-to-date.
    let mut declared = HashMap::new();
    declared.insert(
        "m1".to_string(),
        DeclaredMarketplace {
            name: "m1".into(),
            source: MarketplaceSourceRef::Url {
                url: "https://default".into(),
            },
            source_is_fallback: true,
        },
    );
    let mut materialized = HashMap::new();
    materialized.insert(
        "m1".to_string(),
        MaterializedMarketplace {
            source: MarketplaceSourceRef::Git {
                url: "git://other".into(),
                r#ref: None,
            },
            install_location: PathBuf::from("/cache/m1"),
            last_synced: None,
        },
    );
    let diff = diff_marketplaces(&declared, &materialized);
    assert_eq!(diff.up_to_date, vec!["m1".to_string()]);
}

#[tokio::test]
async fn reconcile_runs_for_missing() {
    let mut declared = HashMap::new();
    declared.insert(
        "m1".to_string(),
        DeclaredMarketplace {
            name: "m1".into(),
            source: MarketplaceSourceRef::Url {
                url: "https://x".into(),
            },
            source_is_fallback: false,
        },
    );
    let materialized: KnownMarketplacesFile = HashMap::new();
    let calls = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::<String>::new()));
    let calls_clone = calls.clone();
    let result = reconcile_marketplaces(&declared, &materialized, move |name, _src| {
        let calls = calls_clone.clone();
        async move {
            calls.lock().await.push(name);
            Ok(PathBuf::from("/cache/m1"))
        }
    })
    .await;
    assert_eq!(result.installed, vec!["m1".to_string()]);
    assert!(calls.lock().await.contains(&"m1".to_string()));
}
