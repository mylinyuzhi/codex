use super::*;

fn make_default_command() -> SkillPromptCommand {
    SkillPromptCommand {
        name: "commit".to_string(),
        description: "Generate a commit message".to_string(),
        prompt: "Analyze the diff...".to_string(),
        allowed_tools: None,
        user_invocable: true,
        disable_model_invocation: false,
        is_hidden: false,
        source: SkillSource::Bundled,
        loaded_from: LoadedFrom::Bundled,
        context: SkillContext::Main,
        agent: None,
        model: None,
        base_dir: None,
        when_to_use: None,
        argument_hint: None,
        aliases: Vec::new(),
        interface: None,
        command_type: CommandType::Prompt,
    }
}

#[test]
fn test_skill_prompt_command_display() {
    let cmd = make_default_command();
    assert_eq!(cmd.to_string(), "/commit - Generate a commit message");
}

#[test]
fn test_slash_command_display() {
    let cmd = SlashCommand {
        name: "review".to_string(),
        description: "Review code changes".to_string(),
        command_type: CommandType::Prompt,
    };
    assert_eq!(cmd.to_string(), "/review [prompt] - Review code changes");
}

#[test]
fn test_command_type_display() {
    assert_eq!(CommandType::Prompt.to_string(), "prompt");
    assert_eq!(CommandType::Local.to_string(), "local");
    assert_eq!(CommandType::LocalJsx.to_string(), "local-jsx");
}

#[test]
fn test_skill_prompt_command_serialize_roundtrip() {
    let cmd = make_default_command();
    let json = serde_json::to_string(&cmd).expect("serialize");
    let deserialized: SkillPromptCommand = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(deserialized.name, "commit");
    assert!(deserialized.user_invocable);
    assert!(!deserialized.disable_model_invocation);
}

#[test]
fn test_skill_prompt_command_deserialize_minimal() {
    let json = r#"{"name":"x","description":"d","prompt":"p"}"#;
    let cmd: SkillPromptCommand = serde_json::from_str(json).expect("deserialize");
    assert_eq!(cmd.name, "x");
    assert!(cmd.allowed_tools.is_none());
    assert!(cmd.user_invocable); // default true
    assert!(!cmd.disable_model_invocation); // default false
}

#[test]
fn test_classification_methods() {
    let mut cmd = make_default_command();
    assert!(cmd.is_user_invocable());
    assert!(cmd.is_llm_invocable());
    assert!(cmd.is_visible_in_help());

    cmd.user_invocable = false;
    cmd.is_hidden = true;
    assert!(!cmd.is_user_invocable());
    assert!(!cmd.is_visible_in_help());

    // disable_model_invocation blocks LLM invocation
    cmd.disable_model_invocation = true;
    assert!(!cmd.is_llm_invocable());

    // LocalJsx command_type also blocks LLM invocation
    cmd.disable_model_invocation = false;
    cmd.command_type = CommandType::LocalJsx;
    assert!(!cmd.is_llm_invocable());
}

#[test]
fn test_skill_context_default() {
    let ctx = SkillContext::default();
    assert_eq!(ctx, SkillContext::Main);
}

#[test]
fn test_skill_context_display() {
    assert_eq!(SkillContext::Main.to_string(), "main");
    assert_eq!(SkillContext::Fork.to_string(), "fork");
}
