use super::*;
use crate::types::ProviderModelConfig;
use cocode_protocol::Capability;
use cocode_protocol::ProviderApi;
use cocode_protocol::WireApi;

fn create_test_resolver() -> ConfigResolver {
    let mut models = HashMap::new();
    models.insert(
        "test-model".to_string(),
        ModelInfo {
            slug: "test-model".to_string(),
            display_name: Some("Test Model".to_string()),
            context_window: Some(8192),
            max_output_tokens: Some(2048),
            capabilities: Some(vec![Capability::TextGeneration, Capability::Streaming]),
            ..Default::default()
        },
    );
    models.insert(
        "deepseek-r1".to_string(),
        ModelInfo {
            slug: "deepseek-r1".to_string(),
            display_name: Some("DeepSeek R1".to_string()),
            context_window: Some(64000),
            max_output_tokens: Some(8192),
            ..Default::default()
        },
    );
    models.insert(
        "ep-12345".to_string(),
        ModelInfo {
            slug: "ep-12345".to_string(),
            context_window: Some(32000),
            max_output_tokens: Some(4096),
            ..Default::default()
        },
    );

    let mut providers = HashMap::new();
    providers.insert(
        "test-provider".to_string(),
        ProviderConfig {
            name: "Test Provider".to_string(),
            api: ProviderApi::Openai,
            base_url: "https://api.test.com".to_string(),
            timeout_secs: 300,
            env_key: Some("TEST_API_KEY".to_string()),
            api_key: Some("fallback-key".to_string()),
            streaming: true,
            wire_api: WireApi::Responses,
            models: vec![
                ProviderModelConfig::new("test-model"),
                ProviderModelConfig::with_api_model_name("ep-12345", "deepseek-r1"),
            ],
            options: None,
            interceptors: Vec::new(),
        },
    );

    ConfigResolver {
        models,
        providers,
        config_dir: None,
    }
}

#[test]
fn test_resolve_model_info_basic() {
    let resolver = create_test_resolver();
    let info = resolver
        .resolve_model_info("test-provider", "test-model")
        .unwrap();

    assert_eq!(info.slug, "test-model");
    assert_eq!(info.display_name, Some("Test Model".to_string()));
    assert_eq!(info.context_window, Some(8192));
    // Resolved from models.json (no per-provider overrides)
    assert_eq!(info.max_output_tokens, Some(2048));
}

#[test]
fn test_resolve_api_model_name() {
    let resolver = create_test_resolver();

    // Direct api_model_name resolution
    let canonical = resolver.resolve_api_model_name("test-provider", "ep-12345");
    assert_eq!(canonical, "deepseek-r1");

    // Non-aliased model returns itself
    let canonical = resolver.resolve_api_model_name("test-provider", "test-model");
    assert_eq!(canonical, "test-model");
}

#[test]
fn test_resolve_model_with_alias() {
    let resolver = create_test_resolver();
    let info = resolver
        .resolve_model_info("test-provider", "ep-12345")
        .unwrap();

    assert_eq!(info.slug, "ep-12345");
    // Resolved from models.json
    assert_eq!(info.context_window, Some(32000));
}

#[test]
fn test_resolve_provider_with_env_key() {
    let resolver = create_test_resolver();

    // Set env var
    // SAFETY: This is a test, and we're using a unique env var name
    unsafe {
        env::set_var("TEST_API_KEY", "env-api-key");
    }

    let config = resolver.resolve_provider("test-provider").unwrap();
    assert_eq!(config.api_key, "env-api-key");
    assert!(config.streaming);
    assert_eq!(config.wire_api, WireApi::Responses);

    // Clean up
    // SAFETY: This is a test cleanup
    unsafe {
        env::remove_var("TEST_API_KEY");
    }
}

#[test]
fn test_resolve_provider_fallback_to_config() {
    let resolver = create_test_resolver();

    // Ensure env var is not set
    // SAFETY: This is a test cleanup
    unsafe {
        env::remove_var("TEST_API_KEY");
    }

    let config = resolver.resolve_provider("test-provider").unwrap();
    assert_eq!(config.api_key, "fallback-key");
}

#[test]
fn test_resolve_provider_not_found() {
    use crate::error::NotFoundKind;
    let resolver = create_test_resolver();
    let result = resolver.resolve_provider("nonexistent");
    assert!(matches!(
        result,
        Err(ConfigError::NotFound {
            kind: NotFoundKind::Provider,
            ..
        })
    ));
}

#[test]
fn test_list_providers() {
    let resolver = create_test_resolver();
    let providers = resolver.list_providers();
    assert!(providers.contains(&"test-provider"));
}

#[test]
fn test_list_models() {
    let resolver = create_test_resolver();
    let models = resolver.list_models("test-provider");
    assert!(models.contains(&"test-model"));
    assert!(models.contains(&"ep-12345"));
}

#[test]
fn test_empty_resolver() {
    let resolver = ConfigResolver::empty();
    assert!(resolver.list_providers().is_empty());
}

#[test]
fn test_unknown_model_missing_required_fields() {
    // Unknown model without context_window/max_output_tokens should fail validation
    let resolver = create_test_resolver();
    let result = resolver.resolve_model_info("test-provider", "unknown-model");
    assert!(result.is_err());
}

#[test]
fn test_base_instructions_from_file() {
    use tempfile::tempdir;

    let temp_dir = tempdir().unwrap();
    let instructions_content = "You are a helpful assistant.";
    std::fs::write(
        temp_dir.path().join("instructions.md"),
        instructions_content,
    )
    .unwrap();

    let mut models = HashMap::new();
    models.insert(
        "test-model".to_string(),
        ModelInfo {
            slug: "test-model".to_string(),
            display_name: Some("Test Model".to_string()),
            context_window: Some(4096),
            max_output_tokens: Some(1024),
            base_instructions_file: Some("instructions.md".to_string()),
            ..Default::default()
        },
    );

    let resolver = ConfigResolver {
        models,
        providers: HashMap::new(),
        config_dir: Some(temp_dir.path().to_path_buf()),
    };

    let info = resolver
        .resolve_model_info("test-provider", "test-model")
        .unwrap();

    assert_eq!(
        info.base_instructions,
        Some(instructions_content.to_string())
    );
}

#[test]
fn test_base_instructions_file_overrides_inline() {
    use tempfile::tempdir;

    let temp_dir = tempdir().unwrap();
    let file_content = "Instructions from file";
    std::fs::write(temp_dir.path().join("instructions.md"), file_content).unwrap();

    let mut models = HashMap::new();
    models.insert(
        "test-model".to_string(),
        ModelInfo {
            slug: "test-model".to_string(),
            display_name: Some("Test Model".to_string()),
            context_window: Some(4096),
            max_output_tokens: Some(1024),
            base_instructions: Some("Inline instructions".to_string()),
            base_instructions_file: Some("instructions.md".to_string()),
            ..Default::default()
        },
    );

    let resolver = ConfigResolver {
        models,
        providers: HashMap::new(),
        config_dir: Some(temp_dir.path().to_path_buf()),
    };

    let info = resolver
        .resolve_model_info("test-provider", "test-model")
        .unwrap();

    // File should take precedence over inline
    assert_eq!(info.base_instructions, Some(file_content.to_string()));
}

#[test]
fn test_base_instructions_fallback_to_inline() {
    let mut models = HashMap::new();
    models.insert(
        "test-model".to_string(),
        ModelInfo {
            slug: "test-model".to_string(),
            display_name: Some("Test Model".to_string()),
            context_window: Some(4096),
            max_output_tokens: Some(1024),
            base_instructions: Some("Inline instructions".to_string()),
            base_instructions_file: Some("nonexistent.md".to_string()),
            ..Default::default()
        },
    );

    let resolver = ConfigResolver {
        models,
        providers: HashMap::new(),
        config_dir: Some(PathBuf::from("/tmp")),
    };

    let info = resolver
        .resolve_model_info("test-provider", "test-model")
        .unwrap();

    // Should fall back to inline when file doesn't exist
    assert_eq!(
        info.base_instructions,
        Some("Inline instructions".to_string())
    );
}

#[test]
fn test_model_config_options_not_merged_into_model_info() {
    // model_options on ProviderModelConfig are NOT merged into ModelInfo.options;
    // they are carried on ProviderModel separately.
    let mut models = HashMap::new();
    models.insert(
        "test-model".to_string(),
        ModelInfo {
            slug: "test-model".to_string(),
            context_window: Some(4096),
            max_output_tokens: Some(1024),
            ..Default::default()
        },
    );

    let mut providers = HashMap::new();
    let mut model_opts = HashMap::new();
    model_opts.insert("temperature".to_string(), serde_json::json!(0.9));
    model_opts.insert("seed".to_string(), serde_json::json!(42));

    providers.insert(
        "test-provider".to_string(),
        ProviderConfig {
            name: "Test Provider".to_string(),
            api: ProviderApi::Openai,
            base_url: "https://api.test.com".to_string(),
            timeout_secs: 300,
            env_key: None,
            api_key: Some("test-key".to_string()),
            streaming: true,
            wire_api: WireApi::Responses,
            models: vec![ProviderModelConfig {
                slug: "test-model".to_string(),
                api_model_name: None,
                model_options: model_opts.clone(),
            }],
            options: None,
            interceptors: Vec::new(),
        },
    );

    let resolver = ConfigResolver {
        models,
        providers,
        config_dir: None,
    };

    // model_options should NOT appear in resolved ModelInfo.options
    let info = resolver
        .resolve_model_info("test-provider", "test-model")
        .unwrap();
    assert!(info.options.is_none());

    // They should be on the ProviderModel instead
    let provider_info = resolver.resolve_provider("test-provider").unwrap();
    let pm = provider_info.get_model("test-model").unwrap();
    assert_eq!(pm.model_options, model_opts);
}

#[test]
fn test_resolve_provider_with_models() {
    let resolver = create_test_resolver();

    // Ensure env var is not set (use fallback key)
    // SAFETY: This is a test cleanup
    unsafe {
        env::remove_var("TEST_API_KEY");
    }

    let provider_info = resolver.resolve_provider("test-provider").unwrap();

    // Check provider fields
    assert_eq!(provider_info.name, "Test Provider");
    assert_eq!(provider_info.api, ProviderApi::Openai);
    assert_eq!(provider_info.base_url, "https://api.test.com");
    assert_eq!(provider_info.api_key, "fallback-key");
    assert_eq!(provider_info.timeout_secs, 300);
    assert!(provider_info.streaming);
    assert_eq!(provider_info.wire_api, WireApi::Responses);
    assert!(provider_info.has_api_key());

    // Check models are populated
    assert_eq!(provider_info.models.len(), 2);

    // Check model slugs
    let slugs = provider_info.model_slugs();
    assert!(slugs.contains(&"test-model"));
    assert!(slugs.contains(&"ep-12345"));

    // Check get_model returns ProviderModel
    let test_model = provider_info.get_model("test-model").unwrap();
    assert_eq!(test_model.slug(), "test-model");
    assert_eq!(
        test_model.model_info.display_name,
        Some("Test Model".to_string())
    );
    assert_eq!(test_model.model_info.max_output_tokens, Some(2048)); // From models.json
    assert!(test_model.api_model_name.is_none()); // No alias for this model

    // Check ep-12345 has api_model_name
    let ep_model = provider_info.get_model("ep-12345").unwrap();
    assert_eq!(ep_model.slug(), "ep-12345");
    assert_eq!(ep_model.api_model_name, Some("deepseek-r1".to_string()));
    assert_eq!(ep_model.api_model_name(), "deepseek-r1"); // Returns alias

    // Check api_model_name helper on ProviderInfo
    assert_eq!(
        provider_info.api_model_name("test-model"),
        Some("test-model")
    ); // No alias
    assert_eq!(
        provider_info.api_model_name("ep-12345"),
        Some("deepseek-r1")
    ); // Has alias
    assert_eq!(provider_info.api_model_name("nonexistent"), None);

    // Check effective_timeout
    assert_eq!(provider_info.effective_timeout("test-model"), 300); // Provider default (no model override)
    assert_eq!(provider_info.effective_timeout("ep-12345"), 300); // Provider default
    assert_eq!(provider_info.effective_timeout("nonexistent"), 300); // Provider default for unknown
}

#[test]
fn test_options_field_propagation() {
    // Test that options from models.json are preserved through resolution
    let mut models = HashMap::new();
    let mut user_opts = HashMap::new();
    user_opts.insert("user_key".to_string(), serde_json::json!("user_value"));
    user_opts.insert(
        "override_key".to_string(),
        serde_json::json!("user_override"),
    );

    models.insert(
        "test-model".to_string(),
        ModelInfo {
            slug: "test-model".to_string(),
            context_window: Some(4096),
            max_output_tokens: Some(1024),
            options: Some(user_opts),
            ..Default::default()
        },
    );

    let mut providers = HashMap::new();
    providers.insert(
        "test-provider".to_string(),
        ProviderConfig {
            name: "Test Provider".to_string(),
            api: ProviderApi::Openai,
            base_url: "https://api.test.com".to_string(),
            timeout_secs: 300,
            env_key: None,
            api_key: Some("test-key".to_string()),
            streaming: true,
            wire_api: WireApi::Responses,
            models: vec![ProviderModelConfig::new("test-model")],
            options: None,
            interceptors: Vec::new(),
        },
    );

    let resolver = ConfigResolver {
        models,
        providers,
        config_dir: None,
    };

    let info = resolver
        .resolve_model_info("test-provider", "test-model")
        .unwrap();

    // Options from models.json should be present
    assert!(info.options.is_some());
    let opts = info.options.unwrap();

    // User keys preserved
    assert_eq!(opts.get("user_key"), Some(&serde_json::json!("user_value")));
    assert_eq!(
        opts.get("override_key"),
        Some(&serde_json::json!("user_override"))
    );
}

#[test]
fn test_model_options_carried_on_provider_model() {
    // ProviderModelConfig.model_options are NOT merged into ModelInfo.options;
    // instead they are carried on ProviderModel and merged at SDK call time.
    let mut models = HashMap::new();
    models.insert(
        "test-model".to_string(),
        ModelInfo {
            slug: "test-model".to_string(),
            context_window: Some(4096),
            max_output_tokens: Some(1024),
            ..Default::default()
        },
    );

    let mut providers = HashMap::new();
    let mut model_options = HashMap::new();
    model_options.insert(
        "response_format".to_string(),
        serde_json::json!({"type": "json_object"}),
    );
    model_options.insert("seed".to_string(), serde_json::json!(42));

    providers.insert(
        "test-provider".to_string(),
        ProviderConfig {
            name: "Test Provider".to_string(),
            api: ProviderApi::Openai,
            base_url: "https://api.test.com".to_string(),
            timeout_secs: 300,
            env_key: None,
            api_key: Some("test-key".to_string()),
            streaming: true,
            wire_api: WireApi::Responses,
            models: vec![ProviderModelConfig {
                slug: "test-model".to_string(),
                api_model_name: None,
                model_options: model_options.clone(),
            }],
            options: None,
            interceptors: Vec::new(),
        },
    );

    let resolver = ConfigResolver {
        models,
        providers,
        config_dir: None,
    };

    // resolve_model_info should NOT contain model_options in info.options
    let info = resolver
        .resolve_model_info("test-provider", "test-model")
        .unwrap();
    assert!(info.options.is_none());

    // Instead, resolve_provider should carry model_options on ProviderModel
    let provider_info = resolver.resolve_provider("test-provider").unwrap();
    let pm = provider_info.get_model("test-model").unwrap();
    assert_eq!(pm.model_options, model_options);

    // model_options should carry the expected keys directly
    assert_eq!(
        pm.model_options.get("response_format"),
        Some(&serde_json::json!({"type": "json_object"}))
    );
    assert_eq!(pm.model_options.get("seed"), Some(&serde_json::json!(42)));
}

#[test]
fn test_required_fields_validation() {
    // Model without context_window should fail
    let mut models = HashMap::new();
    models.insert(
        "no-context".to_string(),
        ModelInfo {
            slug: "no-context".to_string(),
            max_output_tokens: Some(1024),
            ..Default::default()
        },
    );

    let resolver = ConfigResolver {
        models,
        providers: HashMap::new(),
        config_dir: None,
    };

    let result = resolver.resolve_model_info("any-provider", "no-context");
    assert!(result.is_err());
}
