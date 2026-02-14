use super::*;

#[test]
fn test_load_skills_from_dir_success() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    let skill_dir = root.join("commit");
    fs::create_dir_all(&skill_dir).expect("mkdir");
    fs::write(
        skill_dir.join("SKILL.toml"),
        r#"
name = "commit"
description = "Generate a commit message"
prompt_inline = "Look at staged changes and generate a commit message."
allowed_tools = ["Bash"]
"#,
    )
    .expect("write SKILL.toml");

    let outcomes = load_skills_from_dir(root);
    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].is_success());
    assert_eq!(outcomes[0].skill_name(), Some("commit"));
}

#[test]
fn test_load_skills_from_dir_with_prompt_file() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    let skill_dir = root.join("review");
    fs::create_dir_all(&skill_dir).expect("mkdir");
    fs::write(
        skill_dir.join("SKILL.toml"),
        r#"
name = "review"
description = "Review code"
prompt_file = "prompt.md"
"#,
    )
    .expect("write SKILL.toml");
    fs::write(
        skill_dir.join("prompt.md"),
        "Please review the following code changes carefully.",
    )
    .expect("write prompt.md");

    let outcomes = load_skills_from_dir(root);
    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].is_success());

    if let SkillLoadOutcome::Success { skill, .. } = &outcomes[0] {
        assert_eq!(skill.name, "review");
        assert_eq!(
            skill.prompt,
            "Please review the following code changes carefully."
        );
    }
}

#[test]
fn test_load_skills_from_dir_missing_prompt_file() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    let skill_dir = root.join("bad");
    fs::create_dir_all(&skill_dir).expect("mkdir");
    fs::write(
        skill_dir.join("SKILL.toml"),
        r#"
name = "bad"
description = "Bad skill"
prompt_file = "nonexistent.md"
"#,
    )
    .expect("write SKILL.toml");

    let outcomes = load_skills_from_dir(root);
    assert_eq!(outcomes.len(), 1);
    assert!(!outcomes[0].is_success());
}

#[test]
fn test_load_skills_from_dir_invalid_toml() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    let skill_dir = root.join("broken");
    fs::create_dir_all(&skill_dir).expect("mkdir");
    fs::write(skill_dir.join("SKILL.toml"), "this is not valid toml {{{}}")
        .expect("write SKILL.toml");

    let outcomes = load_skills_from_dir(root);
    assert_eq!(outcomes.len(), 1);
    assert!(!outcomes[0].is_success());
}

#[test]
fn test_load_skills_from_dir_validation_failure() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    let skill_dir = root.join("invalid");
    fs::create_dir_all(&skill_dir).expect("mkdir");
    // Empty name should fail validation
    fs::write(
        skill_dir.join("SKILL.toml"),
        r#"
name = ""
description = "Invalid"
prompt_inline = "text"
"#,
    )
    .expect("write SKILL.toml");

    let outcomes = load_skills_from_dir(root);
    assert_eq!(outcomes.len(), 1);
    assert!(!outcomes[0].is_success());
}

#[test]
fn test_load_skills_fail_open() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    // Good skill
    let good = root.join("good");
    fs::create_dir_all(&good).expect("mkdir");
    fs::write(
        good.join("SKILL.toml"),
        "name = \"good\"\ndescription = \"Works\"\nprompt_inline = \"do it\"",
    )
    .expect("write");

    // Bad skill
    let bad = root.join("bad");
    fs::create_dir_all(&bad).expect("mkdir");
    fs::write(bad.join("SKILL.toml"), "garbage {{{}}").expect("write");

    let outcomes = load_skills_from_dir(root);
    assert_eq!(outcomes.len(), 2);

    let successes = outcomes.iter().filter(|o| o.is_success()).count();
    let failures = outcomes.iter().filter(|o| !o.is_success()).count();
    assert_eq!(successes, 1);
    assert_eq!(failures, 1);
}

#[test]
fn test_load_all_skills_multiple_roots() {
    let tmp1 = tempfile::tempdir().expect("create temp dir");
    let tmp2 = tempfile::tempdir().expect("create temp dir");

    let skill1 = tmp1.path().join("s1");
    fs::create_dir_all(&skill1).expect("mkdir");
    fs::write(
        skill1.join("SKILL.toml"),
        "name = \"s1\"\ndescription = \"d\"\nprompt_inline = \"p\"",
    )
    .expect("write");

    let skill2 = tmp2.path().join("s2");
    fs::create_dir_all(&skill2).expect("mkdir");
    fs::write(
        skill2.join("SKILL.toml"),
        "name = \"s2\"\ndescription = \"d\"\nprompt_inline = \"p\"",
    )
    .expect("write");

    let roots = vec![tmp1.path().to_path_buf(), tmp2.path().to_path_buf()];
    let outcomes = load_all_skills(&roots);
    assert_eq!(outcomes.len(), 2);
    assert!(
        outcomes
            .iter()
            .all(super::super::outcome::SkillLoadOutcome::is_success)
    );
}

#[test]
fn test_load_all_skills_nonexistent_root() {
    let roots = vec![PathBuf::from("/nonexistent/xyz")];
    let outcomes = load_all_skills(&roots);
    assert!(outcomes.is_empty());
}

#[test]
fn test_determine_source_project_settings() {
    let source = determine_source(
        Path::new("/project/.cocode/skills/commit"),
        Path::new("/project/.cocode/skills"),
    );
    assert!(matches!(source, SkillSource::ProjectSettings { .. }));
}

#[test]
fn test_determine_source_user_settings() {
    let source = determine_source(
        Path::new("/home/user/.config/cocode/skills/review"),
        Path::new("/home/user/.config/cocode/skills"),
    );
    assert!(matches!(source, SkillSource::UserSettings { .. }));
}

#[test]
fn test_load_skill_maps_new_fields() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    let skill_dir = root.join("deploy");
    fs::create_dir_all(&skill_dir).expect("mkdir");
    fs::write(
        skill_dir.join("SKILL.toml"),
        r#"
name = "deploy"
description = "Deploy to staging"
prompt_inline = "Deploy the app"
user_invocable = false
disable_model_invocation = true
model = "sonnet"
context = "fork"
agent = "deploy-agent"
argument_hint = "<environment>"
when_to_use = "When deploying"
aliases = ["dep", "ship"]
"#,
    )
    .expect("write SKILL.toml");

    let outcomes = load_skills_from_dir(root);
    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].is_success());

    if let SkillLoadOutcome::Success { skill, .. } = &outcomes[0] {
        assert_eq!(skill.name, "deploy");
        assert!(!skill.user_invocable);
        assert!(skill.disable_model_invocation);
        assert!(skill.is_hidden);
        assert_eq!(skill.model, Some("sonnet".to_string()));
        assert_eq!(skill.context, SkillContext::Fork);
        assert_eq!(skill.agent, Some("deploy-agent".to_string()));
        assert_eq!(skill.argument_hint, Some("<environment>".to_string()));
        assert_eq!(skill.when_to_use, Some("When deploying".to_string()));
        assert_eq!(skill.aliases, vec!["dep".to_string(), "ship".to_string()]);
        assert!(skill.base_dir.is_some());
    }
}

#[test]
fn test_load_skill_defaults() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    let skill_dir = root.join("simple");
    fs::create_dir_all(&skill_dir).expect("mkdir");
    fs::write(
        skill_dir.join("SKILL.toml"),
        r#"
name = "simple"
description = "Simple skill"
prompt_inline = "Do it"
"#,
    )
    .expect("write SKILL.toml");

    let outcomes = load_skills_from_dir(root);
    assert!(outcomes[0].is_success());

    if let SkillLoadOutcome::Success { skill, .. } = &outcomes[0] {
        assert!(skill.user_invocable);
        assert!(!skill.disable_model_invocation);
        assert!(!skill.is_hidden);
        assert_eq!(skill.context, SkillContext::Main);
        assert!(skill.model.is_none());
        assert!(skill.agent.is_none());
        assert!(skill.aliases.is_empty());
    }
}
