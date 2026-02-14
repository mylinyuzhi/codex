use super::*;

#[test]
fn test_contributions_default() {
    let contrib = PluginContributions::default();
    assert!(contrib.skills.is_empty());
    assert!(contrib.hooks.is_empty());
    assert!(contrib.agents.is_empty());
    assert!(contrib.commands.is_empty());
    assert!(contrib.mcp_servers.is_empty());
}

#[test]
fn test_contribution_skill() {
    let skill = SkillPromptCommand {
        name: "test".to_string(),
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
        interface: None,
        command_type: cocode_skill::CommandType::Prompt,
    };

    let contrib = PluginContribution::Skill {
        skill,
        plugin_name: "my-plugin".to_string(),
    };

    assert_eq!(contrib.name(), "test");
    assert_eq!(contrib.plugin_name(), "my-plugin");
    assert!(contrib.is_skill());
    assert!(!contrib.is_hook());
    assert!(!contrib.is_agent());
    assert!(!contrib.is_command());
    assert!(!contrib.is_mcp_server());
}

#[test]
fn test_contribution_agent() {
    let definition = AgentDefinition {
        name: "test-agent".to_string(),
        description: "A test agent".to_string(),
        agent_type: "test-agent".to_string(),
        tools: vec!["Read".to_string()],
        disallowed_tools: vec![],
        identity: None,
        max_turns: None,
        permission_mode: None,
    };

    let contrib = PluginContribution::Agent {
        definition,
        plugin_name: "my-plugin".to_string(),
    };

    assert_eq!(contrib.name(), "test-agent");
    assert_eq!(contrib.plugin_name(), "my-plugin");
    assert!(contrib.is_agent());
    assert!(!contrib.is_skill());
}

#[test]
fn test_contributions_serialize() {
    let contrib = PluginContributions {
        skills: vec!["skills/".to_string()],
        hooks: vec!["hooks.json".to_string()],
        agents: vec!["agents/".to_string()],
        commands: vec!["commands/".to_string()],
        mcp_servers: vec![],
    };

    let json_str = serde_json::to_string(&contrib).expect("serialize");
    assert!(json_str.contains("skills"));
    assert!(json_str.contains("hooks"));
    assert!(json_str.contains("agents"));
    assert!(json_str.contains("commands"));
}
