use super::*;
use tempfile::TempDir;

fn create_temp_config() -> (TempDir, ConfigLoader) {
    let temp_dir = TempDir::new().unwrap();
    let loader = ConfigLoader::from_path(temp_dir.path());
    (temp_dir, loader)
}

#[test]
fn test_default_config_dir() {
    let dir = default_config_dir();
    assert!(dir.to_string_lossy().contains(".cocode"));
}

#[test]
fn test_loader_nonexistent_dir() {
    let loader = ConfigLoader::from_path("/nonexistent/path");
    assert!(!loader.exists());

    // Should return defaults for missing files
    let models = loader.load_models().unwrap();
    assert!(models.models.is_empty());
}

#[test]
fn test_loader_ensure_dir() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("test_config");
    let loader = ConfigLoader::from_path(&config_path);

    assert!(!config_path.exists());
    loader.ensure_dir().unwrap();
    assert!(config_path.exists());
}

#[test]
fn test_load_single_model_file() {
    let (temp_dir, loader) = create_temp_config();

    // New list format for *model.json files
    let models_json = r#"[
        {
            "slug": "test-model",
            "display_name": "Test Model",
            "context_window": 4096
        }
    ]"#;

    let models_path = temp_dir.path().join("model.json");
    std::fs::write(&models_path, models_json).unwrap();

    let models = loader.load_models().unwrap();
    assert!(models.models.contains_key("test-model"));
    assert_eq!(
        models.models["test-model"].display_name,
        Some("Test Model".to_string())
    );
}

#[test]
fn test_load_multiple_model_files() {
    let (temp_dir, loader) = create_temp_config();

    // First file: gpt_model.json
    let gpt_models = r#"[
        {"slug": "gpt-5", "display_name": "GPT-5", "context_window": 128000},
        {"slug": "gpt-5-mini", "display_name": "GPT-5 Mini", "context_window": 32000}
    ]"#;
    std::fs::write(temp_dir.path().join("gpt_model.json"), gpt_models).unwrap();

    // Second file: claude_model.json
    let claude_models = r#"[
        {"slug": "claude-opus", "display_name": "Claude Opus", "context_window": 200000}
    ]"#;
    std::fs::write(temp_dir.path().join("claude_model.json"), claude_models).unwrap();

    let models = loader.load_models().unwrap();
    assert_eq!(models.models.len(), 3);
    assert!(models.models.contains_key("gpt-5"));
    assert!(models.models.contains_key("gpt-5-mini"));
    assert!(models.models.contains_key("claude-opus"));
}

#[test]
fn test_load_model_files_duplicate_error() {
    let (temp_dir, loader) = create_temp_config();

    // First file
    let file1 = r#"[{"slug": "gpt-5", "display_name": "GPT-5"}]"#;
    std::fs::write(temp_dir.path().join("a_model.json"), file1).unwrap();

    // Second file with same slug
    let file2 = r#"[{"slug": "gpt-5", "display_name": "GPT-5 Duplicate"}]"#;
    std::fs::write(temp_dir.path().join("b_model.json"), file2).unwrap();

    let result = loader.load_models();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, ConfigError::ConfigValidation { .. }));
    assert!(err.to_string().contains("duplicate model slug"));
}

#[test]
fn test_load_single_provider_file() {
    let (temp_dir, loader) = create_temp_config();

    // New list format for *provider.json files
    let providers_json = r#"[
        {
            "name": "openai",
            "type": "openai",
            "base_url": "https://api.openai.com/v1",
            "env_key": "OPENAI_API_KEY",
            "models": []
        }
    ]"#;

    let providers_path = temp_dir.path().join("provider.json");
    std::fs::write(&providers_path, providers_json).unwrap();

    let providers = loader.load_providers().unwrap();
    assert!(providers.providers.contains_key("openai"));
}

#[test]
fn test_load_multiple_provider_files() {
    let (temp_dir, loader) = create_temp_config();

    // First file
    let file1 = r#"[
        {"name": "openai", "type": "openai", "base_url": "https://api.openai.com/v1", "models": []}
    ]"#;
    std::fs::write(temp_dir.path().join("openai_provider.json"), file1).unwrap();

    // Second file
    let file2 = r#"[
        {"name": "anthropic", "type": "anthropic", "base_url": "https://api.anthropic.com", "models": []}
    ]"#;
    std::fs::write(temp_dir.path().join("anthropic_provider.json"), file2).unwrap();

    let providers = loader.load_providers().unwrap();
    assert_eq!(providers.providers.len(), 2);
    assert!(providers.providers.contains_key("openai"));
    assert!(providers.providers.contains_key("anthropic"));
}

#[test]
fn test_load_provider_files_duplicate_error() {
    let (temp_dir, loader) = create_temp_config();

    // First file
    let file1 = r#"[{"name": "openai", "type": "openai", "base_url": "https://api.openai.com/v1", "models": []}]"#;
    std::fs::write(temp_dir.path().join("a_provider.json"), file1).unwrap();

    // Second file with same name
    let file2 = r#"[{"name": "openai", "type": "openai", "base_url": "https://other.com/v1", "models": []}]"#;
    std::fs::write(temp_dir.path().join("b_provider.json"), file2).unwrap();

    let result = loader.load_providers();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, ConfigError::ConfigValidation { .. }));
    assert!(err.to_string().contains("duplicate provider name"));
}

#[test]
fn test_load_config_json() {
    let (temp_dir, loader) = create_temp_config();

    let config_json = r#"{
        "models": {
            "main": "openai/gpt-5"
        },
        "profile": "fast"
    }"#;
    std::fs::write(temp_dir.path().join(CONFIG_FILE), config_json).unwrap();

    let config = loader.load_config().unwrap();
    assert!(config.models.is_some());
    let models = config.models.as_ref().unwrap();
    assert_eq!(models.main.as_ref().unwrap().provider, "openai");
    assert_eq!(models.main.as_ref().unwrap().model, "gpt-5");
    assert_eq!(config.profile, Some("fast".to_string()));
}

#[test]
fn test_load_all() {
    let (temp_dir, loader) = create_temp_config();

    // Create model file
    let models = r#"[{"slug": "test-model", "display_name": "Test"}]"#;
    std::fs::write(temp_dir.path().join("model.json"), models).unwrap();

    // Create provider file
    let providers =
        r#"[{"name": "test", "type": "openai", "base_url": "https://test.com", "models": []}]"#;
    std::fs::write(temp_dir.path().join("provider.json"), providers).unwrap();

    // Create config file
    let config = r#"{"models": {"main": "test/test-model"}}"#;
    std::fs::write(temp_dir.path().join(CONFIG_FILE), config).unwrap();

    let loaded = loader.load_all().unwrap();
    assert!(loaded.models.models.contains_key("test-model"));
    assert!(loaded.providers.providers.contains_key("test"));
    assert!(loaded.config.models.is_some());
}

#[test]
fn test_load_empty_model_file() {
    let (temp_dir, loader) = create_temp_config();

    let models_path = temp_dir.path().join("model.json");
    std::fs::write(&models_path, "").unwrap();

    // Empty file should return empty list (default)
    let models = loader.load_models().unwrap();
    assert!(models.models.is_empty());
}

#[test]
fn test_load_invalid_json() {
    let (temp_dir, loader) = create_temp_config();

    let models_path = temp_dir.path().join("model.json");
    std::fs::write(&models_path, "{ invalid json }").unwrap();

    let result = loader.load_models();
    assert!(result.is_err());
    let err = result.unwrap_err();
    // With JSONC parser, unquoted keys are allowed, so this parses differently
    // The error is now from serde deserialization, not JSON parsing
    assert!(
        matches!(err, ConfigError::JsonParse { .. })
            || matches!(err, ConfigError::JsoncParse { .. })
    );
}

#[test]
fn test_load_jsonc_with_comments() {
    let (temp_dir, loader) = create_temp_config();

    // JSONC content with comments and trailing commas
    let jsonc_content = r#"[
        // This is a line comment
        {
            "slug": "test-model",
            "display_name": "Test Model", // inline comment
            "context_window": 4096,  // trailing comma allowed
        },
        /* Block comment */
    ]"#;

    std::fs::write(temp_dir.path().join("model.json"), jsonc_content).unwrap();

    let models = loader.load_models().unwrap();
    assert!(models.models.contains_key("test-model"));
    assert_eq!(
        models.models["test-model"].display_name,
        Some("Test Model".to_string())
    );
    assert_eq!(models.models["test-model"].context_window, Some(4096));
}

#[test]
fn test_load_jsonc_with_unquoted_keys() {
    let (temp_dir, loader) = create_temp_config();

    // JSONC content with unquoted keys (only simple alphanumeric names work)
    // Note: underscores in unquoted keys are not supported by jsonc-parser 0.24
    let jsonc_content = r#"[
        {
            slug: "unquoted-model"
        }
    ]"#;

    std::fs::write(temp_dir.path().join("model.json"), jsonc_content).unwrap();

    let models = loader.load_models().unwrap();
    assert!(models.models.contains_key("unquoted-model"));
}

#[test]
fn test_load_jsonc_config_file() {
    let (temp_dir, loader) = create_temp_config();

    // JSONC config with comments
    let config_jsonc = r#"{
        // Model configuration
        "models": {
            "main": "openai/gpt-5", // primary model
        },
        "profile": "fast", // trailing comma
    }"#;

    std::fs::write(temp_dir.path().join(CONFIG_FILE), config_jsonc).unwrap();

    let config = loader.load_config().unwrap();
    assert!(config.models.is_some());
    let models = config.models.as_ref().unwrap();
    assert_eq!(models.main.as_ref().unwrap().provider, "openai");
    assert_eq!(models.main.as_ref().unwrap().model, "gpt-5");
    assert_eq!(config.profile, Some("fast".to_string()));
}

#[test]
fn test_find_config_files_sorted() {
    let (temp_dir, loader) = create_temp_config();

    // Create files in non-alphabetical order
    std::fs::write(temp_dir.path().join("z_model.json"), "[]").unwrap();
    std::fs::write(temp_dir.path().join("a_model.json"), "[]").unwrap();
    std::fs::write(temp_dir.path().join("m_model.json"), "[]").unwrap();

    let files = loader.find_config_files("model");
    assert_eq!(files.len(), 3);
    assert!(files[0].ends_with("a_model.json"));
    assert!(files[1].ends_with("m_model.json"));
    assert!(files[2].ends_with("z_model.json"));
}

#[test]
fn test_find_config_files_excludes_non_matching() {
    let (temp_dir, loader) = create_temp_config();

    std::fs::write(temp_dir.path().join("model.json"), "[]").unwrap();
    std::fs::write(temp_dir.path().join("provider.json"), "[]").unwrap();
    std::fs::write(temp_dir.path().join("config.json"), "{}").unwrap();
    std::fs::write(temp_dir.path().join("other.json"), "{}").unwrap();

    let model_files = loader.find_config_files("model");
    assert_eq!(model_files.len(), 1);
    assert!(model_files[0].ends_with("model.json"));

    let provider_files = loader.find_config_files("provider");
    assert_eq!(provider_files.len(), 1);
    assert!(provider_files[0].ends_with("provider.json"));
}
