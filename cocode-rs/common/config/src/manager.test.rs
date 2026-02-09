use super::*;
use crate::loader::CONFIG_FILE;
use tempfile::TempDir;

fn create_test_manager() -> (TempDir, ConfigManager) {
    let temp_dir = TempDir::new().unwrap();

    // Create test config files (new list format for *provider.json)
    let providers_json = r#"[
        {
            "name": "test-openai",
            "type": "openai",
            "base_url": "https://api.openai.com/v1",
            "api_key": "test-key",
            "models": [
                {"slug": "gpt-5"},
                {"slug": "gpt-5-mini"}
            ]
        }
    ]"#;
    std::fs::write(temp_dir.path().join("provider.json"), providers_json).unwrap();

    // Create config.json
    let config_json = r#"{
        "models": {
            "main": "test-openai/gpt-5"
        }
    }"#;
    std::fs::write(temp_dir.path().join(CONFIG_FILE), config_json).unwrap();

    let manager = ConfigManager::from_path(temp_dir.path()).unwrap();
    (temp_dir, manager)
}

#[test]
fn test_from_default_succeeds() {
    // Should succeed even if ~/.cocode doesn't exist
    let manager = ConfigManager::from_default();
    assert!(manager.is_ok());
}

#[test]
fn test_empty_manager() {
    let manager = ConfigManager::empty();
    let (provider, model) = manager.current();
    assert_eq!(provider, "openai");
    assert_eq!(model, "gpt-5");
}

#[test]
fn test_current_from_config() {
    let (_temp, manager) = create_test_manager();
    let (provider, model) = manager.current();
    assert_eq!(provider, "test-openai");
    assert_eq!(model, "gpt-5");
}

#[test]
fn test_switch_provider_model() {
    let (_temp, manager) = create_test_manager();

    manager.switch("test-openai", "gpt-5-mini").unwrap();
    let (provider, model) = manager.current();
    assert_eq!(provider, "test-openai");
    assert_eq!(model, "gpt-5-mini");
}

#[test]
fn test_resolve_model_info() {
    let (_temp, manager) = create_test_manager();

    let info = manager.resolve_model_info("test-openai", "gpt-5").unwrap();
    assert_eq!(info.slug, "gpt-5");
    assert_eq!(info.display_name, Some("GPT-5".to_string()));
    assert_eq!(info.context_window, Some(272000));
}

#[test]
fn test_list_providers() {
    let (_temp, manager) = create_test_manager();

    let providers = manager.list_providers();
    assert!(providers.iter().any(|p| p.name == "test-openai"));

    // Should also include built-in providers
    assert!(providers.iter().any(|p| p.name == "openai"));
}

#[test]
fn test_list_models() {
    let (_temp, manager) = create_test_manager();

    let models = manager.list_models("test-openai");
    assert!(!models.is_empty());
}

#[test]
fn test_reload() {
    let (temp_dir, manager) = create_test_manager();

    // Modify config
    let new_config_json = r#"{
        "models": {
            "main": "test-openai/gpt-5-mini"
        }
    }"#;
    std::fs::write(temp_dir.path().join(CONFIG_FILE), new_config_json).unwrap();

    manager.reload().unwrap();

    // Reset runtime overrides to use JSON config
    manager.set_runtime_overrides(RuntimeOverrides::default());

    let (provider, model) = manager.current();
    assert_eq!(provider, "test-openai");
    assert_eq!(model, "gpt-5-mini");
}

#[test]
fn test_has_provider() {
    let (_temp, manager) = create_test_manager();

    assert!(manager.has_provider("test-openai"));
    assert!(manager.has_provider("openai")); // Built-in
    assert!(!manager.has_provider("nonexistent"));
}

#[test]
fn test_get_model_config() {
    let (_temp, manager) = create_test_manager();

    // Built-in model
    let config = manager.get_model_config("gpt-5");
    assert!(config.is_some());
    assert_eq!(config.unwrap().display_name, Some("GPT-5".to_string()));
}

#[test]
fn test_runtime_switch_is_in_memory() {
    let (temp_dir, manager) = create_test_manager();

    manager.switch("test-openai", "gpt-5-mini").unwrap();
    let (provider, model) = manager.current();
    assert_eq!(provider, "test-openai");
    assert_eq!(model, "gpt-5-mini");

    // Create new manager - switch should NOT persist (in-memory only)
    let manager2 = ConfigManager::from_path(temp_dir.path()).unwrap();
    let (provider2, model2) = manager2.current();
    // Should fall back to JSON config
    assert_eq!(provider2, "test-openai");
    assert_eq!(model2, "gpt-5"); // Default from config.json, not gpt-5-mini
}

// ==========================================================
// Tests for build_config
// ==========================================================

#[test]
fn test_build_config_basic() {
    let (_temp, manager) = create_test_manager();

    let config = manager.build_config(ConfigOverrides::default()).unwrap();

    // Should have main model resolved
    assert!(config.main_model_info().is_some());
    let main = config.main_model_info().unwrap();
    assert_eq!(main.slug, "gpt-5");
    assert_eq!(main.display_name, Some("GPT-5".to_string()));

    // Should have providers resolved
    assert!(config.providers.contains_key("test-openai"));

    // Should have default sandbox mode
    assert_eq!(config.sandbox_mode, SandboxMode::default());
    assert!(!config.ephemeral);
}

#[test]
fn test_build_config_with_overrides() {
    let (_temp, manager) = create_test_manager();

    let overrides = ConfigOverrides::new()
        .with_cwd("/custom/path")
        .with_sandbox_mode(SandboxMode::WorkspaceWrite)
        .with_ephemeral(true);

    let config = manager.build_config(overrides).unwrap();

    assert_eq!(config.cwd, PathBuf::from("/custom/path"));
    assert_eq!(config.sandbox_mode, SandboxMode::WorkspaceWrite);
    assert!(config.ephemeral);

    // Default writable root should be cwd for WorkspaceWrite
    assert!(
        config
            .writable_roots
            .contains(&PathBuf::from("/custom/path"))
    );
}

#[test]
fn test_build_config_with_custom_writable_roots() {
    let (_temp, manager) = create_test_manager();

    let overrides = ConfigOverrides::new()
        .with_sandbox_mode(SandboxMode::WorkspaceWrite)
        .with_writable_roots(vec![PathBuf::from("/a"), PathBuf::from("/b")]);

    let config = manager.build_config(overrides).unwrap();

    assert_eq!(config.writable_roots.len(), 2);
    assert!(config.writable_roots.contains(&PathBuf::from("/a")));
    assert!(config.writable_roots.contains(&PathBuf::from("/b")));
}

#[test]
fn test_build_config_role_fallback() {
    let (_temp, manager) = create_test_manager();

    let config = manager.build_config(ConfigOverrides::default()).unwrap();

    // Fast role should fall back to main
    let fast = config.model_for_role(ModelRole::Fast);
    assert!(fast.is_some());
    assert_eq!(fast.unwrap().slug, "gpt-5"); // Falls back to main

    // Vision role should also fall back to main
    let vision = config.model_for_role(ModelRole::Vision);
    assert!(vision.is_some());
    assert_eq!(vision.unwrap().slug, "gpt-5");
}

#[test]
fn test_build_config_empty_manager() {
    let manager = ConfigManager::empty();
    let config = manager.build_config(ConfigOverrides::default()).unwrap();

    // Empty manager has no main model configured, so resolved_models is empty
    assert!(config.main_model_info().is_none());
    assert!(config.models.is_empty());
}

#[test]
fn test_build_config_with_user_instructions() {
    let (_temp, manager) = create_test_manager();

    let overrides =
        ConfigOverrides::new().with_user_instructions("Custom instructions for testing");

    let config = manager.build_config(overrides).unwrap();

    assert_eq!(
        config.user_instructions,
        Some("Custom instructions for testing".to_string())
    );
}

#[test]
fn test_build_config_feature_overrides() {
    let (_temp, manager) = create_test_manager();

    let overrides = ConfigOverrides::new().with_feature("web_fetch", true);

    let config = manager.build_config(overrides).unwrap();

    assert!(config.is_feature_enabled(cocode_protocol::Feature::WebFetch));
}

#[test]
fn test_build_config_provider_for_role() {
    let (_temp, manager) = create_test_manager();

    let config = manager.build_config(ConfigOverrides::default()).unwrap();

    // Main role should have provider
    let provider = config.provider_for_role(ModelRole::Main);
    assert!(provider.is_some());
    assert_eq!(provider.unwrap().name, "test-openai");
}

#[test]
fn test_build_config_with_model_overrides() {
    use cocode_protocol::model::ModelRoles;
    use cocode_protocol::model::ModelSpec;

    let (_temp_dir, manager) = create_test_manager();

    // First create the model roles override
    let mut models = ModelRoles::default();
    models.set(ModelRole::Main, ModelSpec::new("test-openai", "gpt-5-mini"));
    let overrides = ConfigOverrides::new().with_models(models);

    let config = manager.build_config(overrides).unwrap();

    // Main model should be the overridden one
    let main = config.main_model().unwrap();
    assert_eq!(main.model, "gpt-5-mini");
}

// ==========================================================
// Tests for role switching
// ==========================================================

#[test]
fn test_switch_role() {
    let (_temp, manager) = create_test_manager();

    // Switch fast role
    manager
        .switch_role(ModelRole::Fast, "test-openai", "gpt-5-mini")
        .unwrap();

    // Fast role should use the new model
    let (provider, model) = manager.current_for_role(ModelRole::Fast);
    assert_eq!(provider, "test-openai");
    assert_eq!(model, "gpt-5-mini");

    // Main role should be unchanged
    let (provider, model) = manager.current_for_role(ModelRole::Main);
    assert_eq!(provider, "test-openai");
    assert_eq!(model, "gpt-5");
}

#[test]
fn test_switch_role_with_thinking() {
    let (_temp, manager) = create_test_manager();

    // Switch with thinking level
    manager
        .switch_role_with_thinking(
            ModelRole::Main,
            "test-openai",
            "gpt-5",
            ThinkingLevel::high().set_budget(32000),
        )
        .unwrap();

    // Check the selection
    let selection = manager.current_selection(ModelRole::Main).unwrap();
    assert_eq!(selection.model.model, "gpt-5");
    assert!(selection.thinking_level.is_some());
    assert_eq!(
        selection.thinking_level.as_ref().unwrap().budget_tokens,
        Some(32000)
    );
}

#[test]
fn test_switch_thinking_level() {
    let (_temp, manager) = create_test_manager();

    // First set up a role
    manager
        .switch_role(ModelRole::Main, "test-openai", "gpt-5")
        .unwrap();

    // Switch just the thinking level
    let updated = manager
        .switch_thinking_level(ModelRole::Main, ThinkingLevel::medium())
        .unwrap();
    assert!(updated);

    // Check the selection
    let selection = manager.current_selection(ModelRole::Main).unwrap();
    assert!(selection.thinking_level.is_some());
    assert_eq!(
        selection.thinking_level.as_ref().unwrap().effort,
        cocode_protocol::model::ReasoningEffort::Medium
    );
}

#[test]
fn test_switch_thinking_level_no_selection() {
    let manager = ConfigManager::empty();

    // Try to switch thinking level for a role that has no selection
    let updated = manager
        .switch_thinking_level(ModelRole::Vision, ThinkingLevel::high())
        .unwrap();

    // Should return false since no selection exists
    assert!(!updated);
}

#[test]
fn test_current_selections() {
    let (_temp, manager) = create_test_manager();

    // Set up multiple roles
    manager
        .switch_role(ModelRole::Main, "test-openai", "gpt-5")
        .unwrap();
    manager
        .switch_role(ModelRole::Fast, "test-openai", "gpt-5-mini")
        .unwrap();

    let selections = manager.current_selections();
    assert!(selections.get(ModelRole::Main).is_some());
    assert!(selections.get(ModelRole::Fast).is_some());
    assert!(selections.get(ModelRole::Vision).is_none());
}

#[test]
fn test_role_fallback_to_main() {
    let (_temp, manager) = create_test_manager();

    // Only set main role
    manager
        .switch_role(ModelRole::Main, "test-openai", "gpt-5-mini")
        .unwrap();

    // Fast role should fallback to main
    let (provider, model) = manager.current_for_role(ModelRole::Fast);
    assert_eq!(provider, "test-openai");
    assert_eq!(model, "gpt-5-mini");
}
