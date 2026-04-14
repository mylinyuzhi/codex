use std::path::PathBuf;

use super::*;

fn test_skill(name: &str, description: &str, prompt: &str, source: SkillSource) -> SkillDefinition {
    SkillDefinition {
        name: name.into(),
        description: description.into(),
        prompt: prompt.into(),
        source,
        aliases: vec![],
        allowed_tools: None,
        model: None,
        when_to_use: None,
        argument_names: vec![],
        paths: vec![],
        effort: None,
        context: SkillContext::Inline,
        agent: None,
        version: None,
        disabled: false,
        hooks: None,
        argument_hint: None,
        user_invocable: true,
        disable_model_invocation: false,
        shell: None,
        content_length: prompt.len() as i64,
        is_hidden: false,
    }
}

#[test]
fn test_skill_manager_register_and_get() {
    let mut mgr = SkillManager::new();
    let mut skill = test_skill(
        "commit",
        "Create a git commit",
        "Create a commit with the staged changes.",
        SkillSource::Bundled,
    );
    skill.allowed_tools = Some(vec!["Bash".into(), "Read".into()]);
    mgr.register(skill);

    assert_eq!(mgr.len(), 1);
    let skill = mgr.get("commit").unwrap();
    assert_eq!(skill.description, "Create a git commit");
    assert!(matches!(skill.source, SkillSource::Bundled));
}

#[test]
fn test_skill_not_found() {
    let mgr = SkillManager::new();
    assert!(mgr.get("nonexistent").is_none());
}

#[test]
fn test_skill_from_project() {
    let mut mgr = SkillManager::new();
    let mut skill = test_skill(
        "deploy",
        "Deploy to production",
        "Run the deploy script.",
        SkillSource::Project {
            path: "/project/.claude/skills/deploy.md".into(),
        },
    );
    skill.model = Some("opus".into());
    mgr.register(skill);

    let skill = mgr.get("deploy").unwrap();
    assert!(matches!(skill.source, SkillSource::Project { .. }));
    assert_eq!(skill.model.as_deref(), Some("opus"));
}

#[test]
fn test_skill_lookup_by_alias() {
    let mut mgr = SkillManager::new();
    let mut skill = test_skill(
        "commit",
        "Create a git commit",
        "Create a commit.",
        SkillSource::Bundled,
    );
    skill.aliases = vec!["ci".into(), "gc".into()];
    mgr.register(skill);

    assert!(mgr.get("commit").is_some());
    assert!(mgr.get("ci").is_some());
    assert!(mgr.get("gc").is_some());
    assert_eq!(mgr.get("ci").unwrap().name, "commit");
    assert!(mgr.get("nonexistent").is_none());
}

#[test]
fn test_load_from_markdown_basic() {
    let content = "# deploy\n\nRun the deployment pipeline.\n";
    let skill = parse_skill_markdown(content, Path::new("/tmp/deploy.md")).unwrap();

    assert_eq!(skill.name, "deploy");
    assert_eq!(skill.prompt, "Run the deployment pipeline.");
    assert!(skill.description.is_empty());
    assert!(skill.allowed_tools.is_none());
}

#[test]
fn test_load_from_markdown_with_frontmatter() {
    let content = "\
# review-pr
---
description: Review a pull request
allowed-tools: Bash, Read, Grep
model: opus
---

Carefully review the PR for correctness and style.
Check for bugs, security issues, and performance.
";
    let skill = parse_skill_markdown(content, Path::new("/tmp/review.md")).unwrap();

    assert_eq!(skill.name, "review-pr");
    assert_eq!(skill.description, "Review a pull request");
    assert_eq!(
        skill.allowed_tools,
        Some(vec![
            "Bash".to_string(),
            "Read".to_string(),
            "Grep".to_string(),
        ])
    );
    assert_eq!(skill.model.as_deref(), Some("opus"));
    assert!(skill.prompt.contains("Carefully review the PR"));
    assert!(skill.prompt.contains("security issues"));
}

#[test]
fn test_load_from_markdown_allowed_tools_underscore() {
    let content = "\
# test
---
allowed_tools: Bash, Read
---

Do things.
";
    let skill = parse_skill_markdown(content, Path::new("/tmp/test.md")).unwrap();
    assert_eq!(
        skill.allowed_tools,
        Some(vec!["Bash".to_string(), "Read".to_string()])
    );
}

#[test]
fn test_load_from_markdown_empty_frontmatter() {
    let content = "# test-skill\n---\n---\n\nDo the thing.\n";
    let skill = parse_skill_markdown(content, Path::new("/tmp/test.md")).unwrap();

    assert_eq!(skill.name, "test-skill");
    assert_eq!(skill.prompt, "Do the thing.");
    assert!(skill.description.is_empty());
}

#[test]
fn test_load_from_markdown_no_heading_fails() {
    let content = "This has no heading.\n";
    let result = parse_skill_markdown(content, Path::new("/tmp/bad.md"));
    assert!(result.is_err());
}

#[test]
fn test_load_from_markdown_empty_fails() {
    let result = parse_skill_markdown("", Path::new("/tmp/empty.md"));
    assert!(result.is_err());
}

#[test]
fn test_load_from_markdown_aliases() {
    let content = "\
# deploy
---
aliases: d, dep
---

Deploy app.
";
    let skill = parse_skill_markdown(content, Path::new("/tmp/deploy.md")).unwrap();
    assert_eq!(skill.aliases, vec!["d", "dep"]);
}

#[test]
fn test_load_from_markdown_hooks_json() {
    let content = "\
# test
---
hooks: {\"pre_tool_use\": \"echo hi\"}
---

Test skill.
";
    let skill = parse_skill_markdown(content, Path::new("/tmp/test.md")).unwrap();
    assert!(skill.hooks.is_some());
    let hooks = skill.hooks.unwrap();
    assert!(hooks.is_object());
    assert!(hooks.get("pre_tool_use").is_some());
}

#[test]
fn test_load_from_markdown_hooks_string() {
    let content = "\
# test
---
hooks: simple-hook
---

Test skill.
";
    let skill = parse_skill_markdown(content, Path::new("/tmp/test.md")).unwrap();
    assert!(skill.hooks.is_some());
    assert_eq!(
        skill.hooks.unwrap(),
        serde_json::Value::String("simple-hook".to_string())
    );
}

#[test]
fn test_load_from_markdown_shell_string() {
    let content = "\
# test
---
shell: bash
---

Test skill.
";
    let skill = parse_skill_markdown(content, Path::new("/tmp/test.md")).unwrap();
    assert_eq!(
        skill.shell,
        Some(serde_json::Value::String("bash".to_string()))
    );
}

#[test]
fn test_load_from_markdown_shell_json() {
    let content = "\
# test
---
shell: {\"type\": \"powershell\"}
---

Test skill.
";
    let skill = parse_skill_markdown(content, Path::new("/tmp/test.md")).unwrap();
    assert!(skill.shell.is_some());
    let shell = skill.shell.unwrap();
    assert!(shell.is_object());
}

#[test]
fn test_load_from_markdown_user_invocable_false() {
    let content = "\
# internal
---
user-invocable: false
---

Internal skill.
";
    let skill = parse_skill_markdown(content, Path::new("/tmp/internal.md")).unwrap();
    assert!(!skill.user_invocable);
}

#[test]
fn test_load_from_markdown_disable_model_invocation() {
    let content = "\
# debug
---
disable-model-invocation: true
---

Debug skill.
";
    let skill = parse_skill_markdown(content, Path::new("/tmp/debug.md")).unwrap();
    assert!(skill.disable_model_invocation);
}

#[test]
fn test_discover_skill_md_directory_format() {
    let dir = tempfile::tempdir().unwrap();

    // Create SKILL.md directory format: my-skill/SKILL.md
    let skill_dir = dir.path().join("my-skill");
    std::fs::create_dir(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "# ignored-heading\n---\ndescription: My skill\n---\nDo stuff.\n",
    )
    .unwrap();

    let skills = discover_skills(&[dir.path().to_path_buf()]);
    assert_eq!(skills.len(), 1);
    // Name comes from directory, not heading
    assert_eq!(skills[0].name, "my-skill");
    assert_eq!(skills[0].description, "My skill");
}

#[test]
fn test_discover_skill_md_case_insensitive() {
    let dir = tempfile::tempdir().unwrap();

    let skill_dir = dir.path().join("case-test");
    std::fs::create_dir(&skill_dir).unwrap();
    std::fs::write(skill_dir.join("skill.md"), "# heading\n\nSkill content.\n").unwrap();

    let skills = discover_skills(&[dir.path().to_path_buf()]);
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].name, "case-test");
}

#[test]
fn test_discover_skills_ignores_flat_md_in_skills_format() {
    let dir = tempfile::tempdir().unwrap();

    // Flat .md file should be ignored in SkillMdOnly format
    std::fs::write(dir.path().join("flat.md"), "# flat\n\nFlat skill.\n").unwrap();

    let skills = discover_skills(&[dir.path().to_path_buf()]);
    assert!(skills.is_empty());
}

#[test]
fn test_discover_skills_legacy_format_flat_md() {
    let dir = tempfile::tempdir().unwrap();

    // Legacy format supports flat .md files
    std::fs::write(dir.path().join("flat.md"), "# flat\n\nFlat skill.\n").unwrap();

    let skills = discover_skills_with_format(&[dir.path().to_path_buf()], SkillDirFormat::Legacy);
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].name, "flat");
}

#[test]
fn test_discover_skills_deduplicates_by_path() {
    let dir = tempfile::tempdir().unwrap();

    let skill_dir = dir.path().join("my-skill");
    std::fs::create_dir(&skill_dir).unwrap();
    std::fs::write(skill_dir.join("SKILL.md"), "# heading\n\nContent.\n").unwrap();

    // Discover the same directory twice - should deduplicate
    let skills = discover_skills(&[dir.path().to_path_buf(), dir.path().to_path_buf()]);
    assert_eq!(skills.len(), 1);
}

#[test]
fn test_discover_skills_nonexistent_dir() {
    let skills = discover_skills(&[PathBuf::from("/nonexistent/path/xyz")]);
    assert!(skills.is_empty());
}

#[test]
fn test_load_from_dirs_with_legacy() {
    let commands_dir = tempfile::tempdir().unwrap();
    // Simulate .claude/commands/ with flat .md
    std::fs::write(
        commands_dir.path().join("old-cmd.md"),
        "# old-cmd\n\nLegacy command.\n",
    )
    .unwrap();

    let mut mgr = SkillManager::new();
    // Simulate path ending in "commands"
    let _cmd_path = commands_dir.path().to_path_buf();
    // load_from_dirs checks if path ends with "commands"
    let commands_path = tempfile::tempdir().unwrap();
    let actual_cmd_dir = commands_path.path().join("commands");
    std::fs::create_dir(&actual_cmd_dir).unwrap();
    std::fs::write(
        actual_cmd_dir.join("legacy.md"),
        "# legacy\n\nLegacy skill.\n",
    )
    .unwrap();
    mgr.load_from_dirs(&[actual_cmd_dir]);

    assert_eq!(mgr.len(), 1);
    assert!(mgr.get("legacy").is_some());
}

#[test]
fn test_load_skill_from_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("greet.md");
    std::fs::write(
        &path,
        "# greet\n---\ndescription: Say hello\n---\nHello world.\n",
    )
    .unwrap();

    let skill = load_skill_from_file(&path).unwrap();
    assert_eq!(skill.name, "greet");
    assert_eq!(skill.description, "Say hello");
    assert_eq!(skill.prompt, "Hello world.");
}

#[test]
fn test_load_from_markdown_extended_frontmatter() {
    let content = "\
# deploy

---
description: Deploy to production
when-to-use: When the user asks to deploy
argument-names: env, region
allowed-tools: Bash, Read
paths: src/**/*.rs, deploy/**
effort: high
context: fork
agent: general-purpose
version: 1.2.0
disabled: false
---

Run the deployment pipeline for the specified environment.
";
    let skill = parse_skill_markdown(content, Path::new("/tmp/deploy.md")).unwrap();

    assert_eq!(skill.name, "deploy");
    assert_eq!(skill.description, "Deploy to production");
    assert_eq!(
        skill.when_to_use.as_deref(),
        Some("When the user asks to deploy")
    );
    assert_eq!(skill.argument_names, vec!["env", "region"]);
    assert_eq!(skill.paths, vec!["src/**/*.rs", "deploy/**"]);
    assert_eq!(skill.effort.as_deref(), Some("high"));
    assert_eq!(skill.context, SkillContext::Fork);
    assert_eq!(skill.agent.as_deref(), Some("general-purpose"));
    assert_eq!(skill.version.as_deref(), Some("1.2.0"));
    assert!(!skill.disabled);
}

#[test]
fn test_disabled_skill_skipped_in_discovery() {
    let dir = tempfile::tempdir().unwrap();

    let active_dir = dir.path().join("active");
    std::fs::create_dir(&active_dir).unwrap();
    std::fs::write(active_dir.join("SKILL.md"), "# active\n\nActive skill.\n").unwrap();

    let disabled_dir = dir.path().join("disabled");
    std::fs::create_dir(&disabled_dir).unwrap();
    std::fs::write(
        disabled_dir.join("SKILL.md"),
        "# disabled\n---\ndisabled: true\n---\nDisabled skill.\n",
    )
    .unwrap();

    let skills = discover_skills(&[dir.path().to_path_buf()]);
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].name, "active");
}

// ── Brace expansion tests ──

#[test]
fn test_expand_braces_no_braces() {
    assert_eq!(expand_braces("*.rs"), vec!["*.rs"]);
}

#[test]
fn test_expand_braces_simple() {
    let mut result = expand_braces("*.{ts,tsx}");
    result.sort();
    assert_eq!(result, vec!["*.ts", "*.tsx"]);
}

#[test]
fn test_expand_braces_nested() {
    let mut result = expand_braces("{a,{b,c}}");
    result.sort();
    assert_eq!(result, vec!["a", "b", "c"]);
}

#[test]
fn test_expand_braces_with_prefix_suffix() {
    let mut result = expand_braces("src/*.{js,ts}");
    result.sort();
    assert_eq!(result, vec!["src/*.js", "src/*.ts"]);
}

#[test]
fn test_expand_braces_unclosed() {
    // Unclosed brace returns as-is
    assert_eq!(expand_braces("*.{ts"), vec!["*.{ts"]);
}

// ── Token-budgeted listing tests ──

#[test]
fn test_inject_skill_listing_empty() {
    let result = inject_skill_listing(&[], 8000);
    assert!(result.listing.is_empty());
    assert_eq!(result.included, 0);
    assert_eq!(result.total, 0);
}

#[test]
fn test_inject_skill_listing_includes_bundled() {
    let skill = test_skill("commit", "Create a commit", "prompt", SkillSource::Bundled);
    let refs: Vec<&SkillDefinition> = vec![&skill];
    let result = inject_skill_listing(&refs, 8000);

    assert!(result.listing.contains("/commit"));
    assert!(result.listing.contains("Create a commit"));
    assert_eq!(result.included, 1);
    assert_eq!(result.total, 1);
}

#[test]
fn test_inject_skill_listing_budget_enforced() {
    let bundled = test_skill("commit", "Create a commit", "p", SkillSource::Bundled);
    let user = test_skill(
        "long-skill",
        &"x".repeat(1000),
        "p",
        SkillSource::User {
            path: "/tmp/s.md".into(),
        },
    );
    let refs: Vec<&SkillDefinition> = vec![&bundled, &user];
    // Budget too small for user skill
    let result = inject_skill_listing(&refs, 100);
    assert_eq!(result.included, 1); // only bundled
    assert_eq!(result.total, 2);
}

#[test]
fn test_inject_skill_listing_with_when_to_use() {
    let mut skill = test_skill("test", "Description", "p", SkillSource::Bundled);
    skill.when_to_use = Some("When doing X".to_string());
    let refs: Vec<&SkillDefinition> = vec![&skill];
    let result = inject_skill_listing(&refs, 8000);

    assert!(result.listing.contains("When doing X"));
}

// ── Managed skills path tests ──

#[test]
fn test_get_skill_paths_includes_managed() {
    let paths = get_skill_paths(Path::new("/home/user/.coco"), Path::new("/project"));
    assert!(paths.len() >= 4);
    // First should be managed
    let managed = &paths[0];
    assert!(
        managed.to_string_lossy().contains("claude-code")
            || managed.to_string_lossy().contains("ClaudeCode")
    );
}

#[test]
fn test_get_skill_paths_order() {
    let paths = get_skill_paths(Path::new("/home/user/.coco"), Path::new("/project"));
    // managed, user, project, legacy
    assert_eq!(paths[1], PathBuf::from("/home/user/.coco/skills"));
    assert_eq!(paths[2], PathBuf::from("/project/.claude/skills"));
    assert_eq!(paths[3], PathBuf::from("/project/.claude/commands"));
}

// ── Paths with brace expansion ──

#[test]
fn test_frontmatter_paths_brace_expansion() {
    let content = "\
# test
---
paths: *.{ts,tsx}, src/**/*.{js,jsx}
---

Test.
";
    let skill = parse_skill_markdown(content, Path::new("/tmp/test.md")).unwrap();
    assert!(skill.paths.contains(&"*.ts".to_string()));
    assert!(skill.paths.contains(&"*.tsx".to_string()));
    assert!(skill.paths.contains(&"src/**/*.js".to_string()));
    assert!(skill.paths.contains(&"src/**/*.jsx".to_string()));
}

// ── R7-T10: discover_skill_dirs_for_paths ──
//
// TS `loadSkillsDir.ts:861-915` walks up from each file path collecting
// `<ancestor>/.claude/skills/` directories that exist. The walk stops
// at (but excludes) cwd, since cwd-level skills are loaded at startup.
// The tests below cover the core walk, the cwd boundary, deepest-first
// ordering, and the missing-dir fast path.

#[test]
fn test_discover_skill_dirs_finds_nested() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path();
    // Create a nested project with a skills dir at the inner level only.
    let project = cwd.join("project");
    let inner = project.join("subdir");
    std::fs::create_dir_all(inner.join(".claude").join("skills")).unwrap();
    let file = inner.join("foo.rs");
    std::fs::write(&file, "// touched by Read").unwrap();

    let result = discover_skill_dirs_for_paths(&[file.as_path()], cwd);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], inner.join(".claude").join("skills"));
}

#[test]
fn test_discover_skill_dirs_excludes_cwd_level() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path();
    // Skills dir AT cwd should NOT be returned — cwd-level skills are
    // loaded at startup, the dynamic walker only finds nested ones.
    std::fs::create_dir_all(cwd.join(".claude").join("skills")).unwrap();
    let file = cwd.join("readme.md");
    std::fs::write(&file, "").unwrap();

    let result = discover_skill_dirs_for_paths(&[file.as_path()], cwd);
    assert!(
        result.is_empty(),
        "cwd-level skills should be excluded, got: {result:?}"
    );
}

#[test]
fn test_discover_skill_dirs_deepest_first() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path();
    // Two skills dirs at different depths.
    let outer = cwd.join("project");
    let inner = outer.join("module");
    std::fs::create_dir_all(outer.join(".claude").join("skills")).unwrap();
    std::fs::create_dir_all(inner.join(".claude").join("skills")).unwrap();
    let file = inner.join("hot.rs");
    std::fs::write(&file, "").unwrap();

    let result = discover_skill_dirs_for_paths(&[file.as_path()], cwd);
    assert_eq!(result.len(), 2);
    // Inner (more components) must come before outer.
    assert_eq!(result[0], inner.join(".claude").join("skills"));
    assert_eq!(result[1], outer.join(".claude").join("skills"));
}

#[test]
fn test_discover_skill_dirs_no_skills_dir_returns_empty() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path();
    let project = cwd.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let file = project.join("plain.rs");
    std::fs::write(&file, "").unwrap();

    let result = discover_skill_dirs_for_paths(&[file.as_path()], cwd);
    assert!(result.is_empty());
}

#[test]
fn test_discover_skill_dirs_dedupes_across_paths() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path();
    let project = cwd.join("project");
    std::fs::create_dir_all(project.join(".claude").join("skills")).unwrap();
    let file1 = project.join("a.rs");
    let file2 = project.join("b.rs");
    std::fs::write(&file1, "").unwrap();
    std::fs::write(&file2, "").unwrap();

    let result = discover_skill_dirs_for_paths(&[file1.as_path(), file2.as_path()], cwd);
    // Same skills dir should only appear once even though both files
    // resolve to it.
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], project.join(".claude").join("skills"));
}
