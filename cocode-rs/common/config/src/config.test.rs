use super::*;

#[test]
fn test_config_default() {
    let config = Config::default();
    assert!(config.models.is_empty());
    assert!(config.providers.is_empty());
    assert!(config.resolved_models.is_empty());
    assert!(config.user_instructions.is_none());
    assert!(!config.ephemeral);
    assert_eq!(config.sandbox_mode, SandboxMode::ReadOnly);
}

#[test]
fn test_config_overrides_builder() {
    let overrides = ConfigOverrides::new()
        .with_cwd("/my/project")
        .with_sandbox_mode(SandboxMode::WorkspaceWrite)
        .with_ephemeral(true)
        .with_feature("web_fetch", true);

    assert_eq!(overrides.cwd, Some(PathBuf::from("/my/project")));
    assert_eq!(overrides.sandbox_mode, Some(SandboxMode::WorkspaceWrite));
    assert_eq!(overrides.ephemeral, Some(true));
    assert_eq!(overrides.features.get("web_fetch"), Some(&true));
}

#[test]
fn test_is_path_writable_read_only() {
    let config = Config {
        sandbox_mode: SandboxMode::ReadOnly,
        ..Default::default()
    };

    assert!(!config.is_path_writable(&PathBuf::from("/any/path")));
}

#[test]
fn test_is_path_writable_full_access() {
    let config = Config {
        sandbox_mode: SandboxMode::FullAccess,
        ..Default::default()
    };

    assert!(config.is_path_writable(&PathBuf::from("/any/path")));
}

#[test]
fn test_is_path_writable_workspace_write() {
    let config = Config {
        sandbox_mode: SandboxMode::WorkspaceWrite,
        writable_roots: vec![PathBuf::from("/workspace")],
        ..Default::default()
    };

    assert!(config.is_path_writable(&PathBuf::from("/workspace/file.txt")));
    assert!(config.is_path_writable(&PathBuf::from("/workspace/sub/dir/file.txt")));
    assert!(!config.is_path_writable(&PathBuf::from("/other/path")));
}

#[test]
fn test_allows_write() {
    assert!(
        !Config {
            sandbox_mode: SandboxMode::ReadOnly,
            ..Default::default()
        }
        .allows_write()
    );

    assert!(
        Config {
            sandbox_mode: SandboxMode::WorkspaceWrite,
            ..Default::default()
        }
        .allows_write()
    );

    assert!(
        Config {
            sandbox_mode: SandboxMode::FullAccess,
            ..Default::default()
        }
        .allows_write()
    );
}

#[test]
fn test_model_for_role_fallback() {
    let main_info = ModelInfo {
        slug: "main-model".to_string(),
        display_name: Some("Main Model".to_string()),
        context_window: Some(128000),
        max_output_tokens: Some(16384),
        ..Default::default()
    };

    let mut resolved_models = HashMap::new();
    resolved_models.insert(ModelRole::Main, main_info.clone());

    let config = Config {
        resolved_models,
        ..Default::default()
    };

    // Main role returns main model
    assert_eq!(
        config.model_for_role(ModelRole::Main).unwrap().slug,
        "main-model"
    );

    // Fast role falls back to main
    assert_eq!(
        config.model_for_role(ModelRole::Fast).unwrap().slug,
        "main-model"
    );

    // Vision role falls back to main
    assert_eq!(
        config.model_for_role(ModelRole::Vision).unwrap().slug,
        "main-model"
    );
}

#[test]
fn test_model_for_role_specific() {
    let main_info = ModelInfo {
        slug: "main-model".to_string(),
        display_name: Some("Main Model".to_string()),
        context_window: Some(128000),
        max_output_tokens: Some(16384),
        ..Default::default()
    };

    let fast_info = ModelInfo {
        slug: "fast-model".to_string(),
        display_name: Some("Fast Model".to_string()),
        ..main_info.clone()
    };

    let mut resolved_models = HashMap::new();
    resolved_models.insert(ModelRole::Main, main_info);
    resolved_models.insert(ModelRole::Fast, fast_info);

    let config = Config {
        resolved_models,
        ..Default::default()
    };

    // Fast role returns specific model
    assert_eq!(
        config.model_for_role(ModelRole::Fast).unwrap().slug,
        "fast-model"
    );

    // Vision still falls back to main
    assert_eq!(
        config.model_for_role(ModelRole::Vision).unwrap().slug,
        "main-model"
    );
}

#[test]
fn test_configured_roles() {
    let main_info = ModelInfo {
        slug: "main-model".to_string(),
        display_name: Some("Main Model".to_string()),
        context_window: Some(128000),
        max_output_tokens: Some(16384),
        ..Default::default()
    };

    let mut resolved_models = HashMap::new();
    resolved_models.insert(ModelRole::Main, main_info.clone());
    resolved_models.insert(
        ModelRole::Fast,
        ModelInfo {
            slug: "fast-model".to_string(),
            ..main_info
        },
    );

    let config = Config {
        resolved_models,
        ..Default::default()
    };

    let roles = config.configured_roles();
    assert_eq!(roles.len(), 2);
}
