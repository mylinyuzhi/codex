use super::*;

#[test]
fn test_merge_section_returns_override_when_present() {
    let overrides = ConfigOverrides {
        tool_config: Some(ToolConfig {
            max_tool_concurrency: 42,
            ..Default::default()
        }),
        ..Default::default()
    };
    let resolved = ResolvedAppConfig::default();
    let env_loader = EnvLoader::new();

    let config: ToolConfig = merge_section(&overrides, &resolved, &env_loader);
    assert_eq!(config.max_tool_concurrency, 42);
}

#[test]
fn test_merge_section_falls_through_to_env_and_json() {
    let overrides = ConfigOverrides::default();
    let resolved = ResolvedAppConfig::default();
    let env_loader = EnvLoader::new();

    let config: ToolConfig = merge_section(&overrides, &resolved, &env_loader);
    assert_eq!(config.max_tool_concurrency, DEFAULT_MAX_TOOL_CONCURRENCY);
}
