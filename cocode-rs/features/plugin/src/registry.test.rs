use super::*;
use crate::contribution::PluginContributions;
use crate::error::PluginError;
use crate::manifest::PluginManifest;
use crate::manifest::PluginMetadata;
use cocode_protocol::ToolName;
use std::path::PathBuf;

fn make_plugin(name: &str, scope: PluginScope) -> LoadedPlugin {
    LoadedPlugin {
        manifest: PluginManifest {
            plugin: PluginMetadata {
                name: name.to_string(),
                version: "1.0.0".to_string(),
                description: "Test plugin".to_string(),
                author: None,
                repository: None,
                license: None,
                min_cocode_version: None,
            },
            contributions: PluginContributions::default(),
            user_config: std::collections::HashMap::new(),
        },
        path: PathBuf::from(format!("/plugins/{name}")),
        scope,
        contributions: Vec::new(),
        settings: crate::manifest::PluginRootSettings::default(),
    }
}

fn make_plugin_with_skill(name: &str, scope: PluginScope, skill_name: &str) -> LoadedPlugin {
    let skill = SkillPromptCommand {
        name: skill_name.to_string(),
        description: "Test skill".to_string(),
        prompt: "Do something".to_string(),
        allowed_tools: None,
        user_invocable: true,
        disable_model_invocation: false,
        is_hidden: false,
        source: cocode_skill::SkillSource::Bundled,
        loaded_from: cocode_skill::LoadedFrom::Bundled,
        context: cocode_skill::SkillContext::Main,
        agent: None,
        model: None,
        base_dir: None,
        when_to_use: None,
        argument_hint: None,
        aliases: Vec::new(),
        version: None,
        arguments: None,
        paths: None,
        interface: None,
        command_type: cocode_skill::CommandType::Prompt,
    };

    let mut plugin = make_plugin(name, scope);
    plugin.contributions.push(PluginContribution::Skill {
        skill,
        plugin_name: name.to_string(),
    });
    plugin
}

fn make_plugin_with_hook(name: &str, scope: PluginScope, hook_name: &str) -> LoadedPlugin {
    let hook = cocode_hooks::HookDefinition {
        name: hook_name.to_string(),
        event_type: cocode_hooks::HookEventType::SessionStart,
        handler: cocode_hooks::HookHandler::Command {
            command: "echo test".to_string(),
        },
        matcher: None,
        source: cocode_hooks::HookSource::Session,
        enabled: true,
        timeout_secs: 30,
        group_id: None,
        once: false,
        status_message: None,
        is_async: false,
        force_sync_execution: false,
    };

    let mut plugin = make_plugin(name, scope);
    plugin.contributions.push(PluginContribution::Hook {
        hook,
        plugin_name: name.to_string(),
    });
    plugin
}

fn make_plugin_with_agent(name: &str, scope: PluginScope, agent_name: &str) -> LoadedPlugin {
    let definition = AgentDefinition {
        name: agent_name.to_string(),
        description: "Test agent".to_string(),
        agent_type: agent_name.to_string(),
        tools: vec![ToolName::Read.as_str().to_string()],
        disallowed_tools: vec![],
        identity: None,
        max_turns: None,
        permission_mode: None,
        fork_context: false,
        color: None,
        critical_reminder: None,
        source: cocode_subagent::AgentSource::BuiltIn,
        skills: vec![],
        background: false,
        memory: None,
        hooks: None,
        mcp_servers: None,
        isolation: None,
        use_custom_prompt: false,
    };

    let mut plugin = make_plugin(name, scope);
    plugin.contributions.push(PluginContribution::Agent {
        definition,
        plugin_name: name.to_string(),
    });
    plugin
}

#[test]
fn test_register_and_get() {
    let mut registry = PluginRegistry::new();
    let plugin = make_plugin("test", PluginScope::User);

    registry.register(plugin).expect("register");

    assert!(registry.has("test"));
    assert!(!registry.has("other"));

    let plugin = registry.get("test").expect("get");
    assert_eq!(plugin.name(), "test");
}

#[test]
fn test_duplicate_same_scope_rejected() {
    let mut registry = PluginRegistry::new();

    registry
        .register(make_plugin("test", PluginScope::User))
        .expect("first");
    let result = registry.register(make_plugin("test", PluginScope::User));

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        PluginError::AlreadyRegistered { .. }
    ));
}

#[test]
fn test_higher_priority_replaces_lower() {
    let mut registry = PluginRegistry::new();

    registry
        .register(make_plugin("test", PluginScope::User))
        .expect("register user");

    // Project has higher priority than User
    registry
        .register(make_plugin("test", PluginScope::Project))
        .expect("register project");

    let plugin = registry.get("test").expect("get");
    assert_eq!(plugin.scope, PluginScope::Project);
    assert_eq!(registry.len(), 1);
}

#[test]
fn test_lower_priority_rejected() {
    let mut registry = PluginRegistry::new();

    registry
        .register(make_plugin("test", PluginScope::Project))
        .expect("register project");

    // User has lower priority than Project
    let result = registry.register(make_plugin("test", PluginScope::User));
    assert!(result.is_err());

    let plugin = registry.get("test").expect("get");
    assert_eq!(plugin.scope, PluginScope::Project);
}

#[test]
fn test_register_all_sorts_by_priority() {
    let mut registry = PluginRegistry::new();

    // Register in reverse priority order — Flag should win
    let plugins = vec![
        make_plugin("test", PluginScope::Flag),
        make_plugin("test", PluginScope::User),
        make_plugin("test", PluginScope::Managed),
    ];
    registry.register_all(plugins);

    assert_eq!(registry.len(), 1);
    let plugin = registry.get("test").expect("get");
    assert_eq!(plugin.scope, PluginScope::Flag);
}

#[test]
fn test_names() {
    let mut registry = PluginRegistry::new();
    registry
        .register(make_plugin("beta", PluginScope::User))
        .expect("register");
    registry
        .register(make_plugin("alpha", PluginScope::Project))
        .expect("register");

    let names = registry.names();
    assert_eq!(names, vec!["alpha", "beta"]);
}

#[test]
fn test_by_scope() {
    let mut registry = PluginRegistry::new();
    registry
        .register(make_plugin("user1", PluginScope::User))
        .expect("register");
    registry
        .register(make_plugin("user2", PluginScope::User))
        .expect("register");
    registry
        .register(make_plugin("project1", PluginScope::Project))
        .expect("register");

    let user_plugins = registry.by_scope(PluginScope::User);
    assert_eq!(user_plugins.len(), 2);

    let project_plugins = registry.by_scope(PluginScope::Project);
    assert_eq!(project_plugins.len(), 1);
}

#[test]
fn test_unregister() {
    let mut registry = PluginRegistry::new();
    registry
        .register(make_plugin("test", PluginScope::User))
        .expect("register");

    let removed = registry.unregister("test");
    assert!(removed.is_some());
    assert!(!registry.has("test"));
}

// --- Issue #1: Hook source set to Plugin ---

#[test]
fn test_hook_source_set_to_plugin() {
    let mut registry = PluginRegistry::new();
    registry
        .register(make_plugin_with_hook(
            "my-plugin",
            PluginScope::User,
            "my-hook",
        ))
        .expect("register");

    let hook_registry = HookRegistry::default();
    registry.apply_hooks_to(&hook_registry);

    let hooks = hook_registry.all_hooks();
    assert_eq!(hooks.len(), 1);
    assert!(matches!(&hooks[0].source, HookSource::Plugin { name } if name == "my-plugin"));
}

// --- Issue #2: Skill source set to Plugin ---

#[test]
fn test_skill_source_set_to_plugin() {
    let mut registry = PluginRegistry::new();
    registry
        .register(make_plugin_with_skill(
            "my-plugin",
            PluginScope::User,
            "my-skill",
        ))
        .expect("register");

    let mut skill_manager = SkillManager::new();
    registry.apply_skills_to(&mut skill_manager);

    // Check namespaced skill
    let namespaced = skill_manager.get("my-plugin:my-skill").expect("namespaced");
    assert_eq!(namespaced.loaded_from, LoadedFrom::Plugin);
    assert!(
        matches!(&namespaced.source, SkillSource::Plugin { plugin_name } if plugin_name == "my-plugin")
    );

    // Check alias skill
    let alias = skill_manager.get("my-skill").expect("alias");
    assert_eq!(alias.loaded_from, LoadedFrom::Plugin);
    assert!(
        matches!(&alias.source, SkillSource::Plugin { plugin_name } if plugin_name == "my-plugin")
    );
}

// --- Issue #5: Agent namespacing ---

#[test]
fn test_agent_namespacing() {
    let mut registry = PluginRegistry::new();
    registry
        .register(make_plugin_with_agent(
            "my-plugin",
            PluginScope::User,
            "my-agent",
        ))
        .expect("register");

    let mut subagent_manager = SubagentManager::new();
    registry.apply_agents_to(&mut subagent_manager);

    let definitions = subagent_manager.definitions();
    assert_eq!(definitions.len(), 2);

    let namespaced = definitions
        .iter()
        .find(|d| d.name == "my-plugin:my-agent")
        .expect("namespaced agent");
    assert_eq!(namespaced.source, cocode_subagent::AgentSource::Plugin);

    let alias = definitions
        .iter()
        .find(|d| d.name == "my-agent")
        .expect("alias agent");
    assert_eq!(alias.source, cocode_subagent::AgentSource::Plugin);
}

#[test]
fn test_agent_ambiguous_name_no_alias() {
    let mut registry = PluginRegistry::new();
    registry
        .register(make_plugin_with_agent(
            "plugin-a",
            PluginScope::User,
            "shared-agent",
        ))
        .expect("register");
    registry
        .register(make_plugin_with_agent(
            "plugin-b",
            PluginScope::Project,
            "shared-agent",
        ))
        .expect("register");

    let mut subagent_manager = SubagentManager::new();
    registry.apply_agents_to(&mut subagent_manager);

    let names: Vec<&str> = subagent_manager
        .definitions()
        .iter()
        .map(|d| d.name.as_str())
        .collect();
    // Only namespaced names, no bare alias
    assert!(names.contains(&"plugin-a:shared-agent"));
    assert!(names.contains(&"plugin-b:shared-agent"));
    assert!(!names.contains(&"shared-agent"));
}

// --- Issue #4: Shell command handler ---

#[test]
fn test_shell_command_wired_as_skill() {
    use crate::command::CommandHandler;
    use crate::command::PluginCommand;

    let cmd = PluginCommand {
        name: "lint".to_string(),
        description: "Run linter".to_string(),
        handler: CommandHandler::Shell {
            command: "cargo clippy".to_string(),
            timeout_sec: None,
        },
        visible: true,
    };

    let mut plugin = make_plugin("my-plugin", PluginScope::User);
    plugin.contributions.push(PluginContribution::Command {
        command: cmd,
        plugin_name: "my-plugin".to_string(),
    });

    let mut registry = PluginRegistry::new();
    registry.register(plugin).expect("register");

    let mut skill_manager = SkillManager::new();
    registry.apply_commands_to(&mut skill_manager, None);

    let skill = skill_manager.get("lint").expect("lint skill");
    assert_eq!(skill.loaded_from, LoadedFrom::Plugin);
    assert!(skill.prompt.contains("cargo clippy"));
    assert_eq!(
        skill.allowed_tools,
        Some(vec![ToolName::Bash.as_str().to_string()])
    );
}
