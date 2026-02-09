use super::*;
use std::fs;

fn setup_test_plugin(dir: &std::path::Path) {
    fs::create_dir_all(dir).expect("mkdir");
    fs::write(
        dir.join("PLUGIN.toml"),
        r#"
[plugin]
name = "test-plugin"
version = "1.0.0"
description = "A test plugin"

[contributions]
skills = ["skills/"]
agents = ["agents/"]
"#,
    )
    .expect("write");

    // Create a skill
    let skills_dir = dir.join("skills").join("test-skill");
    fs::create_dir_all(&skills_dir).expect("mkdir");
    fs::write(
        skills_dir.join("SKILL.toml"),
        r#"
name = "test-skill"
description = "A test skill"
prompt_inline = "Do something"
"#,
    )
    .expect("write");

    // Create an agent
    let agents_dir = dir.join("agents").join("test-agent");
    fs::create_dir_all(&agents_dir).expect("mkdir");
    fs::write(
        agents_dir.join("AGENT.toml"),
        r#"
name = "test-agent"
description = "A test agent"
agent_type = "test-agent"
tools = ["Read"]
"#,
    )
    .expect("write");
}

#[test]
fn test_integration_config_defaults() {
    let config = PluginIntegrationConfig::default();
    assert!(config.managed_dir.is_none());
    assert!(config.user_dir.is_none());
    assert!(config.project_dir.is_none());
}

#[test]
fn test_integration_config_with_project() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let config = PluginIntegrationConfig::with_defaults(Some(tmp.path()));

    assert!(config.managed_dir.is_none());
    assert!(config.user_dir.is_some());
    assert!(config.project_dir.is_some());

    let project_dir = config.project_dir.unwrap();
    assert!(project_dir.ends_with(".cocode/plugins"));
}

#[test]
fn test_load_plugins() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let plugins_dir = tmp.path().join("plugins");
    let plugin_dir = plugins_dir.join("test-plugin");
    setup_test_plugin(&plugin_dir);

    let config = PluginIntegrationConfig::default().with_project_dir(plugins_dir);

    let registry = load_plugins(&config);
    assert_eq!(registry.len(), 1);
    assert!(registry.has("test-plugin"));
}

#[test]
fn test_integrate_plugins() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let plugins_dir = tmp.path().join("plugins");
    let plugin_dir = plugins_dir.join("test-plugin");
    setup_test_plugin(&plugin_dir);

    let config = PluginIntegrationConfig::default().with_project_dir(plugins_dir);

    let mut skill_manager = SkillManager::new();
    let mut hook_registry = HookRegistry::default();
    let mut subagent_manager = SubagentManager::new();

    let registry = integrate_plugins(
        &config,
        &mut skill_manager,
        &mut hook_registry,
        &mut subagent_manager,
    );

    assert_eq!(registry.len(), 1);
    assert_eq!(registry.skill_contributions().len(), 1);
    assert_eq!(registry.agent_contributions().len(), 1);

    // Verify skill was applied
    assert!(skill_manager.get("test-skill").is_some());
}

#[test]
fn test_integrate_empty_config() {
    let config = PluginIntegrationConfig::default();

    let mut skill_manager = SkillManager::new();
    let mut hook_registry = HookRegistry::default();
    let mut subagent_manager = SubagentManager::new();

    let registry = integrate_plugins(
        &config,
        &mut skill_manager,
        &mut hook_registry,
        &mut subagent_manager,
    );

    assert!(registry.is_empty());
}
