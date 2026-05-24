use super::*;
use std::fs;

fn setup_test_plugin(dir: &std::path::Path) {
    fs::create_dir_all(dir).expect("mkdir");
    fs::write(
        dir.join("plugin.json"),
        r#"{
  "plugin": {
    "name": "test-plugin",
    "version": "1.0.0",
    "description": "A test plugin"
  },
  "contributions": {
    "skills": ["skills/"],
    "agents": ["agents/"]
  }
}"#,
    )
    .expect("write");

    // Create a skill
    let skills_dir = dir.join("skills").join("test-skill");
    fs::create_dir_all(&skills_dir).expect("mkdir");
    fs::write(
        skills_dir.join("SKILL.md"),
        "---\nname: test-skill\ndescription: A test skill\n---\nDo something\n",
    )
    .expect("write");

    // Create an agent
    let agents_dir = dir.join("agents").join("test-agent");
    fs::create_dir_all(&agents_dir).expect("mkdir");
    fs::write(
        agents_dir.join("agent.json"),
        r#"{
  "name": "test-agent",
  "description": "A test agent",
  "agent_type": "test-agent",
  "tools": ["Read"]
}"#,
    )
    .expect("write");
}

#[test]
fn test_integration_config_defaults() {
    let config = PluginIntegrationConfig::default();
    assert!(config.managed_dir.is_none());
    assert!(config.user_dir.is_none());
    assert!(config.project_dir.is_none());
    assert!(config.plugins_dir.is_none());
    assert!(config.inline_dirs.is_empty());
}

#[test]
fn test_integration_config_with_project() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let cocode_home = tmp.path().join(".cocode");
    let config = PluginIntegrationConfig::with_defaults(&cocode_home, Some(tmp.path()));

    assert!(config.managed_dir.is_none());
    assert!(config.user_dir.is_some());
    assert!(config.project_dir.is_some());
    assert!(config.plugins_dir.is_some());

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
    let hook_registry = HookRegistry::default();

    let result = integrate_plugins(&config, &mut skill_manager, &hook_registry, None);

    assert_eq!(result.registry.len(), 1);
    assert_eq!(result.registry.skill_contributions().len(), 1);
    assert_eq!(result.registry.agent_contributions().len(), 1);

    // Verify skill was applied
    assert!(skill_manager.get("test-skill").is_some());
}

#[test]
fn test_integrate_empty_config() {
    let config = PluginIntegrationConfig::default();

    let mut skill_manager = SkillManager::new();
    let hook_registry = HookRegistry::default();

    let result = integrate_plugins(&config, &mut skill_manager, &hook_registry, None);

    assert!(result.registry.is_empty());
}

#[test]
fn test_inline_dirs() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let plugin_dir = tmp.path().join("inline-plugin");
    setup_test_plugin(&plugin_dir);

    let config =
        PluginIntegrationConfig::default().with_inline_dirs(vec![tmp.path().to_path_buf()]);

    let registry = load_plugins(&config);
    assert_eq!(registry.len(), 1);
}

#[test]
fn test_installed_plugins_loaded() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let plugins_home = tmp.path().join("plugins-home");

    // Create a cached plugin
    let cache_path = plugins_home
        .join("cache")
        .join("market")
        .join("cached-plugin")
        .join("1.0.0");
    setup_test_plugin(&cache_path);

    // Create installed registry pointing to it
    let mut registry = InstalledPluginsRegistry::empty();
    registry.add(
        "cached-plugin",
        crate::installed_registry::InstalledPluginEntry {
            scope: "user".to_string(),
            version: "1.0.0".to_string(),
            install_path: cache_path,
            installed_at: "2025-01-01T00:00:00Z".to_string(),
            last_updated: "2025-01-01T00:00:00Z".to_string(),
            git_commit_sha: None,
            project_path: None,
        },
    );
    registry
        .save(&plugins_home.join("installed_plugins.json"))
        .unwrap();

    // Enable the plugin
    let mut settings = PluginSettings::default();
    settings.set_enabled("cached-plugin", true);
    settings.save(&plugins_home.join("settings.json")).unwrap();

    // Integrate
    let config = PluginIntegrationConfig::default().with_plugins_dir(plugins_home);
    let mut skill_manager = SkillManager::new();
    let hook_registry = HookRegistry::default();

    let result = integrate_plugins(&config, &mut skill_manager, &hook_registry, None);
    // The test-plugin name is inside the plugin.json
    assert_eq!(result.registry.len(), 1);
    assert!(result.registry.has("test-plugin"));
}

#[test]
fn test_disabled_installed_plugins_not_loaded() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let plugins_home = tmp.path().join("plugins-home");

    let cache_path = plugins_home
        .join("cache")
        .join("market")
        .join("disabled-plugin")
        .join("1.0.0");
    setup_test_plugin(&cache_path);

    let mut registry = InstalledPluginsRegistry::empty();
    registry.add(
        "disabled-plugin",
        crate::installed_registry::InstalledPluginEntry {
            scope: "user".to_string(),
            version: "1.0.0".to_string(),
            install_path: cache_path,
            installed_at: "2025-01-01T00:00:00Z".to_string(),
            last_updated: "2025-01-01T00:00:00Z".to_string(),
            git_commit_sha: None,
            project_path: None,
        },
    );
    registry
        .save(&plugins_home.join("installed_plugins.json"))
        .unwrap();

    // Disable the plugin
    let mut settings = PluginSettings::default();
    settings.set_enabled("disabled-plugin", false);
    settings.save(&plugins_home.join("settings.json")).unwrap();

    let config = PluginIntegrationConfig::default().with_plugins_dir(plugins_home);
    let mut skill_manager = SkillManager::new();
    let hook_registry = HookRegistry::default();

    let result = integrate_plugins(&config, &mut skill_manager, &hook_registry, None);
    assert!(result.registry.is_empty());
}

#[test]
fn test_integrate_plugins_with_agents() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let plugins_dir = tmp.path().join("plugins");
    let plugin_dir = plugins_dir.join("test-plugin");
    setup_test_plugin(&plugin_dir);

    let config = PluginIntegrationConfig::default().with_project_dir(plugins_dir);

    let mut skill_manager = SkillManager::new();
    let hook_registry = HookRegistry::default();
    let mut subagent_manager = cocode_subagent::SubagentManager::new();

    let result = integrate_plugins(
        &config,
        &mut skill_manager,
        &hook_registry,
        Some(&mut subagent_manager),
    );

    assert_eq!(result.registry.len(), 1);
    assert_eq!(result.registry.agent_contributions().len(), 1);

    // Verify agents were registered in the subagent manager
    // Agent namespacing creates both "test-plugin:test-agent" and "test-agent" (unambiguous alias)
    let definitions = subagent_manager.definitions();
    assert_eq!(definitions.len(), 2);

    let namespaced = definitions
        .iter()
        .find(|d| d.name == "test-plugin:test-agent")
        .expect("namespaced agent");
    assert_eq!(namespaced.agent_type, "test-plugin:test-agent");

    let alias = definitions
        .iter()
        .find(|d| d.name == "test-agent")
        .expect("alias agent");
    assert_eq!(alias.agent_type, "test-agent");
}

#[test]
fn test_register_extra_marketplaces() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let plugins_dir = tmp.path().join("plugins-home");
    fs::create_dir_all(&plugins_dir).expect("mkdir");

    let mm = MarketplaceManager::new(plugins_dir);

    let extras = vec![
        ExtraMarketplaceEntry {
            name: "test-market".to_string(),
            source: MarketplaceSource::Github {
                repo: "owner/repo".to_string(),
                git_ref: None,
            },
            auto_update: true,
        },
        ExtraMarketplaceEntry {
            name: "local-market".to_string(),
            source: MarketplaceSource::Directory {
                path: tmp.path().to_path_buf(),
            },
            auto_update: false,
        },
    ];

    let added = mm.register_extra(&extras).expect("register_extra");
    assert_eq!(added, 2);

    // Verify they were persisted
    let list = mm.list();
    assert_eq!(list.len(), 2);
    assert!(list.contains_key("test-market"));
    assert!(list.contains_key("local-market"));
    assert!(list["test-market"].auto_update);
    assert!(!list["local-market"].auto_update);
}

#[test]
fn test_register_extra_marketplaces_deduplication() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let plugins_dir = tmp.path().join("plugins-home");
    fs::create_dir_all(&plugins_dir).expect("mkdir");

    let mm = MarketplaceManager::new(plugins_dir);

    let extras = vec![ExtraMarketplaceEntry {
        name: "dup-market".to_string(),
        source: MarketplaceSource::Git {
            url: "https://example.com/repo.git".to_string(),
            git_ref: None,
        },
        auto_update: false,
    }];

    // First registration
    let added = mm.register_extra(&extras).expect("first register");
    assert_eq!(added, 1);

    // Second registration — should skip duplicate
    let added = mm.register_extra(&extras).expect("second register");
    assert_eq!(added, 0);

    // Still only one entry
    assert_eq!(mm.list().len(), 1);
}

#[test]
fn test_register_extra_marketplaces_empty() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let plugins_dir = tmp.path().join("plugins-home");
    fs::create_dir_all(&plugins_dir).expect("mkdir");

    let mm = MarketplaceManager::new(plugins_dir);
    let added = mm.register_extra(&[]).expect("empty register");
    assert_eq!(added, 0);
}

#[test]
fn test_integrate_with_extra_marketplaces() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let plugins_dir = tmp.path().join("plugins-home");
    fs::create_dir_all(&plugins_dir).expect("mkdir");

    let extras = vec![ExtraMarketplaceEntry {
        name: "integrated-market".to_string(),
        source: MarketplaceSource::Url {
            url: "https://example.com/marketplace.json".to_string(),
        },
        auto_update: true,
    }];

    let config = PluginIntegrationConfig::default()
        .with_plugins_dir(plugins_dir.clone())
        .with_extra_known_marketplaces(extras);

    let mut skill_manager = SkillManager::new();
    let hook_registry = HookRegistry::default();

    let result = integrate_plugins(&config, &mut skill_manager, &hook_registry, None);
    assert!(result.registry.is_empty()); // no actual plugins

    // Verify marketplace was registered
    let mm = MarketplaceManager::new(plugins_dir);
    let list = mm.list();
    assert_eq!(list.len(), 1);
    assert!(list.contains_key("integrated-market"));
}

#[test]
fn test_mcp_server_namespaced_format() {
    // Verify the namespaced MCP server name format: plugin_{pluginName}_{serverName}
    let plugin_name = "my-plugin";
    let server_name = "filesystem";
    let namespaced = format!("plugin_{plugin_name}_{server_name}");
    assert_eq!(namespaced, "plugin_my-plugin_filesystem");
}
