use super::*;

#[test]
fn test_load_skills_from_dir_success() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    let skill_dir = root.join("commit");
    fs::create_dir_all(&skill_dir).expect("mkdir");
    fs::write(
        skill_dir.join("SKILL.md"),
        r#"---
name: commit
description: Generate a commit message
allowed-tools:
  - Bash
---
Look at staged changes and generate a commit message.
"#,
    )
    .expect("write SKILL.md");

    let outcomes = load_skills_from_dir(root);
    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].is_success());
    assert_eq!(outcomes[0].skill_name(), Some("commit"));
}

#[test]
fn test_load_skills_prompt_from_body() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    let skill_dir = root.join("review");
    fs::create_dir_all(&skill_dir).expect("mkdir");
    fs::write(
        skill_dir.join("SKILL.md"),
        r#"---
name: review
description: Review code
---
Please review the following code changes carefully.
"#,
    )
    .expect("write SKILL.md");

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
fn test_load_skills_from_dir_empty_body() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    let skill_dir = root.join("bad");
    fs::create_dir_all(&skill_dir).expect("mkdir");
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: bad\ndescription: Bad skill\n---\n",
    )
    .expect("write SKILL.md");

    let outcomes = load_skills_from_dir(root);
    assert_eq!(outcomes.len(), 1);
    assert!(!outcomes[0].is_success()); // empty prompt fails validation
}

#[test]
fn test_load_skills_from_dir_invalid_yaml() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    let skill_dir = root.join("broken");
    fs::create_dir_all(&skill_dir).expect("mkdir");
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\n: this is not valid yaml {{{}}\n---\nbody\n",
    )
    .expect("write SKILL.md");

    let outcomes = load_skills_from_dir(root);
    assert_eq!(outcomes.len(), 1);
    assert!(!outcomes[0].is_success());
}

#[test]
fn test_load_skills_from_dir_no_frontmatter() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();

    let skill_dir = root.join("nofm");
    fs::create_dir_all(&skill_dir).expect("mkdir");
    fs::write(
        skill_dir.join("SKILL.md"),
        "This is just markdown without frontmatter.",
    )
    .expect("write SKILL.md");

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
        skill_dir.join("SKILL.md"),
        "---\nname: \"\"\ndescription: Invalid\n---\nsome prompt\n",
    )
    .expect("write SKILL.md");

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
        good.join("SKILL.md"),
        "---\nname: good\ndescription: Works\n---\ndo it\n",
    )
    .expect("write");

    // Bad skill (no frontmatter)
    let bad = root.join("bad");
    fs::create_dir_all(&bad).expect("mkdir");
    fs::write(bad.join("SKILL.md"), "garbage content without frontmatter").expect("write");

    let outcomes = load_skills_from_dir(root);
    assert_eq!(outcomes.len(), 2);

    let successes: Vec<_> = outcomes.iter().filter(|o| o.is_success()).collect();
    let failures: Vec<_> = outcomes.iter().filter(|o| !o.is_success()).collect();
    assert_eq!(successes.len(), 1);
    assert_eq!(failures.len(), 1);
}

#[test]
fn test_load_all_skills_multiple_roots() {
    let tmp1 = tempfile::tempdir().expect("create temp dir");
    let tmp2 = tempfile::tempdir().expect("create temp dir");

    let skill1 = tmp1.path().join("s1");
    fs::create_dir_all(&skill1).expect("mkdir");
    fs::write(
        skill1.join("SKILL.md"),
        "---\nname: s1\ndescription: d\n---\np\n",
    )
    .expect("write");

    let skill2 = tmp2.path().join("s2");
    fs::create_dir_all(&skill2).expect("mkdir");
    fs::write(
        skill2.join("SKILL.md"),
        "---\nname: s2\ndescription: d\n---\np\n",
    )
    .expect("write");

    let roots = vec![tmp1.path().to_path_buf(), tmp2.path().to_path_buf()];
    let outcomes = load_all_skills(&roots);
    assert_eq!(outcomes.len(), 2);
    assert!(outcomes.iter().all(|o| o.is_success()));
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
        skill_dir.join("SKILL.md"),
        r#"---
name: deploy
description: Deploy to staging
user-invocable: false
disable-model-invocation: true
model: sonnet
context: fork
agent: deploy-agent
argument-hint: "<environment>"
when-to-use: When deploying
aliases:
  - dep
  - ship
---
Deploy the app
"#,
    )
    .expect("write SKILL.md");

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
        skill_dir.join("SKILL.md"),
        "---\nname: simple\ndescription: Simple skill\n---\nDo it\n",
    )
    .expect("write SKILL.md");

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
