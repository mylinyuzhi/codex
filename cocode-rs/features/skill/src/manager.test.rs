use super::*;
use crate::interface::ArgumentDef;

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
        version: None,
        arguments: None,
        paths: None,
        interface: None,
        command_type: CommandType::Prompt,
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

    // Namespaced command with colon
    assert_eq!(
        parse_skill_command("/ns:cmd args"),
        Some(("ns:cmd", "args"))
    );

    // Invalid command names are rejected
    assert_eq!(parse_skill_command("/bad!name"), None);
    assert_eq!(parse_skill_command("/bad@name"), None);
    assert_eq!(parse_skill_command("/ "), None);
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

    // Should have plugin skill (output-style is now a local command, not bundled)
    assert!(manager.has("plugin"));
    let skill = manager.get("plugin").unwrap();
    assert!(!skill.prompt.is_empty());
}

#[test]
fn test_register_bundled_does_not_override_user_skills() {
    let mut manager = SkillManager::new();

    // Register a user skill with the same name as a bundled skill
    manager.register(make_skill("plugin", "User's custom plugin"));

    // Now register bundled skills
    manager.register_bundled();

    // User skill should still be there, not overridden
    let skill = manager.get("plugin").unwrap();
    assert_eq!(skill.prompt, "User's custom plugin");
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

    // Skill with when_to_use - should be included
    let mut commit = make_skill("commit", "Generate commit");
    commit.when_to_use = Some("Use when the user asks to commit changes".to_string());
    manager.register(commit);

    // Skill with description but no when_to_use - should be included (has description)
    manager.register(make_skill("review", "Review code"));

    // Skill with empty description and no when_to_use (non-bundled) - should be excluded
    let mut no_desc = make_skill("no-desc", "");
    no_desc.description = String::new();
    no_desc.when_to_use = None;
    no_desc.source = SkillSource::UserSettings {
        path: PathBuf::from("/user"),
    };
    no_desc.loaded_from = LoadedFrom::UserSettings;
    manager.register(no_desc);

    // Disabled model invocation even with when_to_use - should be excluded
    let mut disabled = make_skill("internal", "Internal");
    disabled.disable_model_invocation = true;
    disabled.when_to_use = Some("never".to_string());
    manager.register(disabled);

    // Builtin skill - should be excluded (source filter)
    let mut builtin = make_skill("builtin", "Builtin");
    builtin.source = SkillSource::Builtin;
    manager.register(builtin);

    // LocalJsx skill - should be excluded (type filter)
    let mut local_jsx = make_skill("ui-cmd", "UI command");
    local_jsx.command_type = CommandType::LocalJsx;
    manager.register(local_jsx);

    let invocable = manager.llm_invocable_skills();
    let mut names: Vec<&str> = invocable.iter().map(|s| s.name.as_str()).collect();
    names.sort();
    assert_eq!(names, vec!["commit", "review"]);
}

#[test]
fn test_bundled_skills_not_llm_invocable() {
    // After registering bundled skills, plugin should NOT
    // appear in llm_invocable_skills because it is LocalJsx type.
    let manager = SkillManager::with_bundled();
    let invocable = manager.llm_invocable_skills();
    let has_plugin = invocable.iter().any(|s| s.name == "plugin");
    assert!(!has_plugin, "plugin should not be LLM invocable");
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

#[test]
fn test_simple_glob_match() {
    // *.ext matches extension
    assert!(simple_glob_match("*.rs", "main.rs"));
    assert!(simple_glob_match("*.rs", "src/lib.rs"));
    assert!(!simple_glob_match("*.rs", "main.ts"));

    // **/*.ext matches nested paths
    assert!(simple_glob_match("**/*.rs", "a/b/c.rs"));
    assert!(simple_glob_match("**/*.ts", "src/components/App.ts"));
    assert!(!simple_glob_match("**/*.rs", "file.ts"));

    // **/filename matches by suffix
    assert!(simple_glob_match("**/Cargo.toml", "crate/Cargo.toml"));
    assert!(simple_glob_match("**/Cargo.toml", "Cargo.toml"));

    // Exact match
    assert!(simple_glob_match("Cargo.toml", "Cargo.toml"));
    assert!(simple_glob_match("Cargo.toml", "sub/Cargo.toml"));

    // Backslash normalization
    assert!(simple_glob_match("**\\*.rs", "src\\main.rs"));
}

#[test]
fn test_conditional_skills() {
    let mut manager = SkillManager::new();
    manager.register(make_skill("normal", "Normal skill"));

    let mut conditional = make_skill("rust-lint", "Lint Rust files");
    conditional.paths = Some(vec!["**/*.rs".to_string()]);
    manager.register(conditional);

    assert_eq!(manager.conditional_skills().len(), 1);
    assert_eq!(manager.conditional_skills()[0].name, "rust-lint");

    // Conditional skills excluded from llm_invocable
    let invocable = manager.llm_invocable_skills();
    assert!(!invocable.iter().any(|s| s.name == "rust-lint"));
}

#[test]
fn test_activate_for_paths() {
    let mut manager = SkillManager::new();

    let mut rs_skill = make_skill("rust-lint", "Lint Rust");
    rs_skill.paths = Some(vec!["**/*.rs".to_string()]);
    manager.register(rs_skill);

    let mut ts_skill = make_skill("ts-lint", "Lint TypeScript");
    ts_skill.paths = Some(vec!["**/*.ts".to_string()]);
    manager.register(ts_skill);

    let activated = manager.activate_for_paths(&[PathBuf::from("src/main.rs")]);
    assert_eq!(activated.len(), 1);
    assert_eq!(activated[0].name, "rust-lint");

    let activated = manager.activate_for_paths(&[PathBuf::from("src/app.ts")]);
    assert_eq!(activated.len(), 1);
    assert_eq!(activated[0].name, "ts-lint");

    let activated = manager.activate_for_paths(&[PathBuf::from("readme.md")]);
    assert!(activated.is_empty());
}

#[test]
fn test_execute_skill_with_positional_args() {
    let mut manager = SkillManager::new();
    let mut skill = make_skill("greet", "Hello $1, welcome to $2");
    skill.arguments = Some(vec![
        ArgumentDef {
            name: "name".to_string(),
            description: None,
            required: false,
        },
        ArgumentDef {
            name: "place".to_string(),
            description: None,
            required: false,
        },
    ]);
    manager.register(skill);

    let result = execute_skill(&manager, "/greet Alice Wonderland").unwrap();
    assert!(result.prompt.contains("Hello Alice, welcome to Wonderland"));
}

#[test]
fn test_execute_skill_with_named_args() {
    let mut manager = SkillManager::new();
    let mut skill = make_skill("deploy", "Deploy ${args.env} with ${args.tag}");
    skill.arguments = Some(vec![
        ArgumentDef {
            name: "env".to_string(),
            description: None,
            required: false,
        },
        ArgumentDef {
            name: "tag".to_string(),
            description: None,
            required: false,
        },
    ]);
    manager.register(skill);

    let result = execute_skill(&manager, "/deploy prod v1.2.3").unwrap();
    assert!(result.prompt.contains("Deploy prod with v1.2.3"));
}

#[test]
fn test_execute_skill_with_cocode_skill_dir() {
    let mut manager = SkillManager::new();
    let mut skill = make_skill("run", "Execute ${COCODE_SKILL_DIR}/run.sh");
    skill.base_dir = Some(PathBuf::from("/skills/runner"));
    manager.register(skill);

    let result = execute_skill(&manager, "/run").unwrap();
    assert!(result.prompt.contains("Execute /skills/runner/run.sh"));
}
