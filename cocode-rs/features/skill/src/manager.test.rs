use super::*;

fn make_skill(name: &str, prompt: &str) -> SkillPromptCommand {
    SkillPromptCommand {
        name: name.to_string(),
        description: format!("{name} description"),
        prompt: prompt.to_string(),
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
    }
}

#[test]
fn test_parse_skill_command() {
    assert_eq!(parse_skill_command("/commit"), Some(("commit", "")));
    assert_eq!(
        parse_skill_command("/review file.rs"),
        Some(("review", "file.rs"))
    );
    assert_eq!(
        parse_skill_command("/test arg1 arg2"),
        Some(("test", "arg1 arg2"))
    );
    assert_eq!(parse_skill_command("not a command"), None);
    assert_eq!(parse_skill_command(""), None);
}

#[test]
fn test_manager_register_and_get() {
    let mut manager = SkillManager::new();
    manager.register(make_skill("commit", "Generate commit message"));

    assert!(manager.has("commit"));
    assert!(!manager.has("review"));

    let skill = manager.get("commit").unwrap();
    assert_eq!(skill.name, "commit");
}

#[test]
fn test_manager_names() {
    let mut manager = SkillManager::new();
    manager.register(make_skill("beta", "Beta"));
    manager.register(make_skill("alpha", "Alpha"));

    let names = manager.names();
    assert_eq!(names, vec!["alpha", "beta"]);
}

#[test]
fn test_execute_skill() {
    let mut manager = SkillManager::new();
    manager.register(make_skill("commit", "Generate a commit message"));

    let result = execute_skill(&manager, "/commit").unwrap();
    assert_eq!(result.skill_name, "commit");
    assert_eq!(result.prompt, "Generate a commit message");
    assert_eq!(result.args, "");

    // With arguments
    let result = execute_skill(&manager, "/commit --amend").unwrap();
    assert!(result.prompt.contains("--amend"));
    assert_eq!(result.args, "--amend");
}

#[test]
fn test_execute_skill_not_found() {
    let manager = SkillManager::new();
    assert!(execute_skill(&manager, "/nonexistent").is_none());
}

#[test]
fn test_execute_skill_with_arguments_placeholder() {
    let mut manager = SkillManager::new();
    let mut skill = make_skill("review", "Review PR #$ARGUMENTS");
    skill.prompt = "Review PR #$ARGUMENTS".to_string();
    manager.register(skill);

    // With placeholder and args
    let result = execute_skill(&manager, "/review 123").unwrap();
    assert_eq!(result.prompt, "Review PR #123");

    // With placeholder but no args (placeholder becomes empty)
    let result = execute_skill(&manager, "/review").unwrap();
    assert_eq!(result.prompt, "Review PR #");
}

#[test]
fn test_with_bundled() {
    let manager = SkillManager::with_bundled();

    // Should have output-style skill
    assert!(manager.has("output-style"));
    let skill = manager.get("output-style").unwrap();
    assert!(skill.prompt.contains("/output-style"));
}

#[test]
fn test_register_bundled_does_not_override_user_skills() {
    let mut manager = SkillManager::new();

    // Register a user skill with the same name as a bundled skill
    manager.register(make_skill("output-style", "User's custom output-style"));

    // Now register bundled skills
    manager.register_bundled();

    // User skill should still be there, not overridden
    let skill = manager.get("output-style").unwrap();
    assert_eq!(skill.prompt, "User's custom output-style");
}

#[test]
fn test_find_by_name_or_alias() {
    let mut manager = SkillManager::new();
    let mut skill = make_skill("commit", "Generate commit message");
    skill.aliases = vec!["ci".to_string(), "cm".to_string()];
    manager.register(skill);

    // By name
    assert!(manager.find_by_name_or_alias("commit").is_some());
    // By alias
    assert!(manager.find_by_name_or_alias("ci").is_some());
    // By alias
    assert!(manager.find_by_name_or_alias("cm").is_some());
    // Not found
    assert!(manager.find_by_name_or_alias("nonexistent").is_none());
}

#[test]
fn test_execute_skill_by_alias() {
    let mut manager = SkillManager::new();
    let mut skill = make_skill("commit", "Generate commit message");
    skill.aliases = vec!["ci".to_string()];
    manager.register(skill);

    let result = execute_skill(&manager, "/ci").unwrap();
    assert_eq!(result.skill_name, "commit");
}

#[test]
fn test_execute_skill_not_user_invocable() {
    let mut manager = SkillManager::new();
    let mut skill = make_skill("internal", "Internal skill");
    skill.user_invocable = false;
    manager.register(skill);

    // Should return None for non-user-invocable skills
    assert!(execute_skill(&manager, "/internal").is_none());
}

#[test]
fn test_llm_invocable_skills() {
    let mut manager = SkillManager::new();

    // Normal skill - should be included
    manager.register(make_skill("commit", "Generate commit"));

    // Disabled model invocation - should be excluded
    let mut disabled = make_skill("internal", "Internal");
    disabled.disable_model_invocation = true;
    manager.register(disabled);

    // Builtin skill - should be excluded
    let mut builtin = make_skill("builtin", "Builtin");
    builtin.source = SkillSource::Builtin;
    manager.register(builtin);

    let invocable = manager.llm_invocable_skills();
    assert_eq!(invocable.len(), 1);
    assert_eq!(invocable[0].name, "commit");
}

#[test]
fn test_user_visible_skills() {
    let mut manager = SkillManager::new();

    // Normal skill - should be visible
    manager.register(make_skill("commit", "Generate commit"));

    // Hidden skill - should not be visible
    let mut hidden = make_skill("hidden", "Hidden");
    hidden.is_hidden = true;
    manager.register(hidden);

    // Builtin skill - should not be visible
    let mut builtin = make_skill("builtin", "Builtin");
    builtin.source = SkillSource::Builtin;
    manager.register(builtin);

    let visible = manager.user_visible_skills();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].name, "commit");
}

#[test]
fn test_execute_skill_with_base_dir() {
    let mut manager = SkillManager::new();
    let mut skill = make_skill("deploy", "Deploy the app");
    skill.base_dir = Some(PathBuf::from("/project/skills/deploy"));
    manager.register(skill);

    let result = execute_skill(&manager, "/deploy").unwrap();
    assert!(
        result
            .prompt
            .contains("Base directory for this skill: /project/skills/deploy")
    );
    assert!(result.prompt.contains("Deploy the app"));
}

#[test]
fn test_execution_result_fields() {
    let mut manager = SkillManager::new();
    let mut skill = make_skill("deploy", "Deploy");
    skill.model = Some("sonnet".to_string());
    skill.context = SkillContext::Fork;
    skill.agent = Some("deploy-agent".to_string());
    skill.base_dir = Some(PathBuf::from("/skills/deploy"));
    manager.register(skill);

    let result = execute_skill(&manager, "/deploy").unwrap();
    assert_eq!(result.model, Some("sonnet".to_string()));
    assert_eq!(result.context, SkillContext::Fork);
    assert_eq!(result.agent, Some("deploy-agent".to_string()));
    assert_eq!(result.base_dir, Some(PathBuf::from("/skills/deploy")));
}
