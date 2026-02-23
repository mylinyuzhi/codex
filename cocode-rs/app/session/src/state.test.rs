use super::*;
use cocode_config::json_config::ExtraMarketplaceConfig;
use cocode_config::json_config::MarketplaceSourceConfig;
use hyper_sdk::ContentBlock;

#[test]
fn test_turn_result_from_loop_result() {
    let loop_result = LoopResult::completed(
        3,
        1000,
        500,
        "Hello!".to_string(),
        vec![ContentBlock::text("Hello!")],
    );

    let turn = TurnResult::from_loop_result(&loop_result);
    assert_eq!(turn.final_text, "Hello!");
    assert_eq!(turn.turns_completed, 3);
    assert_eq!(turn.usage.input_tokens, 1000);
    assert_eq!(turn.usage.output_tokens, 500);
    assert!(turn.is_complete);
}

#[test]
fn test_turn_result_serde() {
    let turn = TurnResult {
        final_text: "test".to_string(),
        turns_completed: 5,
        usage: TokenUsage::new(100, 50),
        has_pending_tools: false,
        is_complete: true,
        stop_reason: StopReason::ModelStopSignal,
    };

    let json = serde_json::to_string(&turn).expect("serialize");
    let parsed: TurnResult = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(parsed.final_text, turn.final_text);
    assert_eq!(parsed.turns_completed, turn.turns_completed);
    assert_eq!(parsed.usage.input_tokens, turn.usage.input_tokens);
}

#[test]
fn test_convert_extra_marketplaces_all_variants() {
    let extras = vec![
        ExtraMarketplaceConfig {
            name: "gh-market".to_string(),
            source: MarketplaceSourceConfig::Github {
                repo: "owner/repo".to_string(),
                git_ref: Some("main".to_string()),
            },
            auto_update: true,
        },
        ExtraMarketplaceConfig {
            name: "git-market".to_string(),
            source: MarketplaceSourceConfig::Git {
                url: "https://example.com/repo.git".to_string(),
                git_ref: None,
            },
            auto_update: false,
        },
        ExtraMarketplaceConfig {
            name: "dir-market".to_string(),
            source: MarketplaceSourceConfig::Directory {
                path: "/tmp/plugins".to_string(),
            },
            auto_update: false,
        },
        ExtraMarketplaceConfig {
            name: "url-market".to_string(),
            source: MarketplaceSourceConfig::Url {
                url: "https://example.com/marketplace.json".to_string(),
            },
            auto_update: true,
        },
    ];

    let result = convert_extra_marketplaces(&extras);
    assert_eq!(result.len(), 4);

    assert_eq!(result[0].name, "gh-market");
    assert!(result[0].auto_update);
    assert!(matches!(
        &result[0].source,
        cocode_plugin::MarketplaceSource::Github { repo, git_ref }
        if repo == "owner/repo" && *git_ref == Some("main".to_string())
    ));

    assert_eq!(result[1].name, "git-market");
    assert!(!result[1].auto_update);
    assert!(matches!(
        &result[1].source,
        cocode_plugin::MarketplaceSource::Git { url, .. }
        if url == "https://example.com/repo.git"
    ));

    assert_eq!(result[2].name, "dir-market");
    assert!(matches!(
        &result[2].source,
        cocode_plugin::MarketplaceSource::Directory { path }
        if path == std::path::Path::new("/tmp/plugins")
    ));

    assert_eq!(result[3].name, "url-market");
    assert!(result[3].auto_update);
    assert!(matches!(
        &result[3].source,
        cocode_plugin::MarketplaceSource::Url { url }
        if url == "https://example.com/marketplace.json"
    ));
}

#[test]
fn test_convert_extra_marketplaces_empty() {
    let result = convert_extra_marketplaces(&[]);
    assert!(result.is_empty());
}

#[test]
fn test_marketplace_source_display() {
    assert_eq!(
        marketplace_source_display(&cocode_plugin::MarketplaceSource::Github {
            repo: "owner/repo".to_string(),
            git_ref: None,
        }),
        ("github".to_string(), "owner/repo".to_string())
    );

    assert_eq!(
        marketplace_source_display(&cocode_plugin::MarketplaceSource::Git {
            url: "https://git.example.com/repo".to_string(),
            git_ref: Some("v1".to_string()),
        }),
        (
            "git".to_string(),
            "https://git.example.com/repo".to_string()
        )
    );

    assert_eq!(
        marketplace_source_display(&cocode_plugin::MarketplaceSource::Directory {
            path: std::path::PathBuf::from("/tmp/dir"),
        }),
        ("directory".to_string(), "/tmp/dir".to_string())
    );

    assert_eq!(
        marketplace_source_display(&cocode_plugin::MarketplaceSource::Url {
            url: "https://example.com/m.json".to_string(),
        }),
        ("url".to_string(), "https://example.com/m.json".to_string())
    );
}
