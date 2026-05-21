use std::path::PathBuf;

use super::*;

fn test_skill(name: &str, description: &str, prompt: &str, source: SkillSource) -> SkillDefinition {
    SkillDefinition {
        name: name.into(),
        display_name: None,
        description: description.into(),
        prompt: prompt.into(),
        source,
        aliases: vec![],
        allowed_tools: None,
        model: None,
        model_role: None,
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
        has_user_specified_description: true,
        progress_message: Some("running".to_string()),
        is_hidden: false,
        gated_by: None,
        files: std::collections::HashMap::new(),
        skill_root: None,
    }
}

#[test]
fn test_skill_manager_register_and_get() {
    let mgr = SkillManager::new();
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
    let mgr = SkillManager::new();
    let mut skill = test_skill(
        "deploy",
        "Deploy to production",
        "Run the deploy script.",
        SkillSource::Project {
            path: "/project/.claude/skills/deploy.md".into(),
        },
    );
    skill.model = Some("anthropic/claude-opus-4-7".into());
    mgr.register(skill);

    let skill = mgr.get("deploy").unwrap();
    assert!(matches!(skill.source, SkillSource::Project { .. }));
    assert_eq!(skill.model.as_deref(), Some("anthropic/claude-opus-4-7"));
}

#[test]
fn test_skill_lookup_by_alias() {
    let mgr = SkillManager::new();
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
    // No frontmatter, no heading: file body is preserved verbatim and
    // the skill name comes from the file stem (TS `getRegularCommandName`).
    // No frontmatter description → description is auto-extracted from the
    // first body line via `extractDescriptionFromMarkdown`, and
    // `has_user_specified_description` is false (TS parity).
    let content = "Run the deployment pipeline.\n";
    let skill = parse_skill_markdown(content, Path::new("/tmp/deploy.md")).unwrap();

    assert_eq!(skill.name, "deploy");
    assert_eq!(skill.prompt, "Run the deployment pipeline.");
    assert_eq!(skill.description, "Run the deployment pipeline.");
    assert!(!skill.has_user_specified_description);
    assert!(skill.allowed_tools.is_none());
}

#[test]
fn test_load_from_markdown_with_frontmatter() {
    let content = "\
---
description: Review a pull request
allowed-tools: Bash, Read, Grep
model: anthropic/claude-opus-4-7
model_role: review
---

Carefully review the PR for correctness and style.
Check for bugs, security issues, and performance.
";
    let skill = parse_skill_markdown(content, Path::new("/tmp/review-pr.md")).unwrap();

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
    assert_eq!(skill.model.as_deref(), Some("anthropic/claude-opus-4-7"));
    assert_eq!(skill.model_role, Some(coco_types::ModelRole::Review));
    assert!(skill.prompt.contains("Carefully review the PR"));
    assert!(skill.prompt.contains("security issues"));
}

#[test]
fn test_load_from_markdown_allowed_tools_underscore() {
    let content = "\
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
    let content = "---\n---\n\nDo the thing.\n";
    let skill = parse_skill_markdown(content, Path::new("/tmp/test-skill.md")).unwrap();

    assert_eq!(skill.name, "test-skill");
    assert_eq!(skill.prompt, "Do the thing.");
    // Body fallback: extracted description = first non-empty body line.
    assert_eq!(skill.description, "Do the thing.");
    assert!(!skill.has_user_specified_description);
}

#[test]
fn test_load_from_markdown_no_frontmatter_loads_body_as_prompt() {
    // TS parity: a file without frontmatter is not an error — the whole
    // file becomes the skill body and the name comes from the file stem.
    let content = "This has no frontmatter, just body text.\n";
    let skill = parse_skill_markdown(content, Path::new("/tmp/bare.md")).unwrap();
    assert_eq!(skill.name, "bare");
    assert_eq!(skill.prompt, "This has no frontmatter, just body text.");
    // Body fallback supplies the description.
    assert_eq!(
        skill.description,
        "This has no frontmatter, just body text."
    );
    assert!(!skill.has_user_specified_description);
}

#[test]
fn test_load_from_markdown_empty_loads_empty_skill() {
    // TS parity: empty content yields an empty body, not an error. The
    // name is still derived from the file path. Description falls back
    // to the default label `'Skill'` (matches TS `extractDescriptionFromMarkdown`
    // when content has no non-empty lines).
    let skill = parse_skill_markdown("", Path::new("/tmp/empty.md")).unwrap();
    assert_eq!(skill.name, "empty");
    assert!(skill.prompt.is_empty());
    assert_eq!(skill.description, "Skill");
    assert!(!skill.has_user_specified_description);
}

#[test]
fn test_load_from_markdown_aliases() {
    let content = "\
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
---
hooks: {\"PreToolUse\": \"echo hi\"}
---

Test skill.
";
    let skill = parse_skill_markdown(content, Path::new("/tmp/test.md")).unwrap();
    assert!(skill.hooks.is_some());
    let hooks = skill.hooks.unwrap();
    assert!(hooks.is_object());
    assert!(hooks.get("PreToolUse").is_some());
}

#[test]
fn test_load_from_markdown_hooks_string() {
    let content = "\
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
        "---\ndescription: My skill\n---\nDo stuff.\n",
    )
    .unwrap();

    let skills = discover_skills(&[dir.path().to_path_buf()]);
    assert_eq!(skills.len(), 1);
    // Name always comes from the directory (TS `getSkillCommandName`).
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

    let mgr = SkillManager::new();
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
    std::fs::write(&path, "---\ndescription: Say hello\n---\nHello world.\n").unwrap();

    let skill = load_skill_from_file(&path).unwrap();
    assert_eq!(skill.name, "greet");
    assert_eq!(skill.description, "Say hello");
    assert_eq!(skill.prompt, "Hello world.");
}

#[test]
fn test_load_from_markdown_extended_frontmatter() {
    // TS frontmatter `arguments` is whitespace-separated (mirrors
    // `parseArgumentNames` from `utils/argumentSubstitution.ts:50`).
    // Comma-separation is the legacy disk format; we keep the legacy
    // alias keys but the TS-canonical key is `arguments`.
    let content = "\
---
description: Deploy to production
when-to-use: When the user asks to deploy
arguments: env region
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
    // TS strips trailing `/**` because the `ignore` library treats a bare
    // path as matching both the path itself and its descendants.
    assert_eq!(skill.paths, vec!["src/**/*.rs", "deploy"]);
    assert_eq!(skill.effort, Some(coco_types::ReasoningEffort::High));
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
    std::fs::write(active_dir.join("SKILL.md"), "Active skill.\n").unwrap();

    let disabled_dir = dir.path().join("disabled");
    std::fs::create_dir(&disabled_dir).unwrap();
    std::fs::write(
        disabled_dir.join("SKILL.md"),
        "---\ndisabled: true\n---\nDisabled skill.\n",
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
fn test_arguments_field_whitespace_split_filters_numeric() {
    let content = "\
---
description: Test arg parsing
arguments: env region 42 user
---

Body
";
    let skill = parse_skill_markdown(content, Path::new("/tmp/x.md")).unwrap();
    // Numeric `42` is filtered (would conflict with `$N` shorthand).
    assert_eq!(skill.argument_names, vec!["env", "region", "user"]);
}

#[test]
fn test_arguments_field_legacy_aliases_still_work() {
    let content = "\
---
description: Test arg parsing
argument-names: env region
---

Body
";
    let skill = parse_skill_markdown(content, Path::new("/tmp/x.md")).unwrap();
    assert_eq!(skill.argument_names, vec!["env", "region"]);
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

// ── TS-format SKILL.md compatibility ──
//
// The reference TS loader (`claude-code-kim/src/skills/loadSkillsDir.ts`)
// puts YAML frontmatter at the top of the file and takes the skill name
// from the directory. These tests cover that layout end-to-end, plus the
// real-YAML features it exercises (nested mappings, sequence syntax).

#[test]
fn test_load_from_markdown_ts_format_frontmatter_first() {
    // No `# Name` heading — frontmatter sits at the top of the file.
    let content = "\
---
name: lark-base
version: 1.2.0
description: \"Operate Lark Base via lark-cli\"
---

Body content explaining the skill.
";
    let skill = parse_skill_markdown(content, Path::new("/tmp/lark-base.md")).unwrap();
    assert_eq!(skill.name, "lark-base");
    assert_eq!(skill.description, "Operate Lark Base via lark-cli");
    assert_eq!(skill.version.as_deref(), Some("1.2.0"));
    assert!(skill.prompt.starts_with("Body content"));
}

#[test]
fn test_load_from_markdown_ts_format_nested_metadata_ignored() {
    // The TS spec doesn't define `metadata`, but a real YAML parser must
    // tolerate (and silently drop) unknown nested shapes — the rest of
    // the file should still load.
    let content = "\
---
name: lark-base
description: \"Lark Base operations\"
metadata:
  requires:
    bins: [\"lark-cli\"]
  cliHelp: \"lark-cli base --help\"
---

Body.
";
    let skill = parse_skill_markdown(content, Path::new("/tmp/lark-base.md")).unwrap();
    assert_eq!(skill.name, "lark-base");
    assert_eq!(skill.description, "Lark Base operations");
    assert_eq!(skill.prompt, "Body.");
}

#[test]
fn test_load_from_markdown_ts_format_yaml_list_allowed_tools() {
    // Real YAML supports list syntax for allowed-tools; both forms must
    // produce the same result.
    let content = "\
---
name: review-pr
description: Review a PR
allowed-tools:
  - Bash
  - Read
  - Grep
---

Review the diff.
";
    let skill = parse_skill_markdown(content, Path::new("/tmp/review.md")).unwrap();
    assert_eq!(
        skill.allowed_tools,
        Some(vec!["Bash".into(), "Read".into(), "Grep".into()])
    );
}

#[test]
fn test_load_from_markdown_ts_format_yaml_list_paths() {
    let content = "\
---
name: rust-skill
description: Rust skill
paths:
  - src/**/*.rs
  - tests/**
---

Body.
";
    let skill = parse_skill_markdown(content, Path::new("/tmp/rust.md")).unwrap();
    // Trailing `/**` stripped per TS `parseSkillPaths`.
    assert_eq!(skill.paths, vec!["src/**/*.rs", "tests"]);
}

#[test]
fn test_discover_ts_format_skill_md_takes_name_from_directory() {
    // The actual lark-base scenario: TS-format SKILL.md inside a named
    // directory. Name should come from the directory, not the (absent)
    // heading or the frontmatter `name` field.
    let dir = tempfile::tempdir().unwrap();
    let skill_dir = dir.path().join("lark-base");
    std::fs::create_dir(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\n\
name: this-is-overridden-by-dir\n\
version: 1.2.0\n\
description: \"Operate Lark Base\"\n\
metadata:\n  \
  requires:\n    \
    bins: [\"lark-cli\"]\n\
---\n\n\
Body of the skill.\n",
    )
    .unwrap();

    let skills = discover_skills(&[dir.path().to_path_buf()]);
    assert_eq!(skills.len(), 1, "lark-base SKILL.md should load");
    assert_eq!(
        skills[0].name, "lark-base",
        "name should come from the directory in SKILL.md format"
    );
    assert_eq!(skills[0].description, "Operate Lark Base");
    assert_eq!(skills[0].version.as_deref(), Some("1.2.0"));
}

#[test]
fn test_load_from_markdown_plain_prose_loads_as_body() {
    // TS parity: plain prose is not an error — it becomes the skill body.
    // The only way `parse_skill_markdown` returns Err is if the path has
    // no usable file name (covered by `derive_skill_name_from_path`).
    let content = "Just some plain text, not a skill at all.\n";
    let skill = parse_skill_markdown(content, Path::new("/tmp/bad.md")).unwrap();
    assert_eq!(skill.name, "bad");
    assert_eq!(skill.prompt, "Just some plain text, not a skill at all.");
}

#[test]
fn test_display_name_from_frontmatter_name() {
    // TS: `displayName: frontmatter.name` (loadSkillsDir.ts:239). The
    // path-derived name is unchanged; display_name overrides only the
    // user-facing surface.
    let content = "\
---
name: \"My Pretty Name\"
description: A skill
---
body
";
    let skill = parse_skill_markdown(content, Path::new("/tmp/raw-name.md")).unwrap();
    assert_eq!(skill.name, "raw-name", "name comes from path stem");
    assert_eq!(
        skill.display_name.as_deref(),
        Some("My Pretty Name"),
        "display_name comes from frontmatter `name` field"
    );
    assert_eq!(
        skill.user_facing_name(),
        "My Pretty Name",
        "user_facing_name prefers display_name over name"
    );
}

#[test]
fn test_user_facing_name_falls_back_to_name() {
    // TS: `userFacingName(): displayName || skillName`. With no
    // frontmatter `name`, display_name is None and the canonical name
    // is used.
    let content = "---\ndescription: A skill\n---\nbody\n";
    let skill = parse_skill_markdown(content, Path::new("/tmp/raw-name.md")).unwrap();
    assert!(skill.display_name.is_none());
    assert_eq!(skill.user_facing_name(), "raw-name");
}

#[test]
fn test_display_name_does_not_change_lookup_identity() {
    // SKILL.md in a directory: name comes from the directory; the
    // frontmatter `name` populates display_name but does NOT change
    // how the skill is keyed in the manager.
    let dir = tempfile::tempdir().unwrap();
    let skill_dir = dir.path().join("internal-id");
    std::fs::create_dir(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: \"Pretty Display\"\ndescription: x\n---\nbody\n",
    )
    .unwrap();

    let skills = discover_skills(&[dir.path().to_path_buf()]);
    assert_eq!(skills.len(), 1);
    let s = &skills[0];
    // Lookup name = directory; display_name = frontmatter name.
    assert_eq!(s.name, "internal-id");
    assert_eq!(s.display_name.as_deref(), Some("Pretty Display"));
    assert_eq!(s.user_facing_name(), "Pretty Display");
}

// ── has_user_specified_description / extract_description_from_markdown ──

#[test]
fn test_has_user_specified_description_true_when_frontmatter_set() {
    let content = "---\ndescription: Explicit user-supplied desc\n---\nbody\n";
    let skill = parse_skill_markdown(content, Path::new("/tmp/x.md")).unwrap();
    assert_eq!(skill.description, "Explicit user-supplied desc");
    assert!(skill.has_user_specified_description);
}

#[test]
fn test_has_user_specified_description_false_when_extracted_from_body() {
    // No frontmatter description → fallback to first non-empty body line.
    let content = "---\nname: foo\n---\n\n# My Skill Heading\nMore text.\n";
    let skill = parse_skill_markdown(content, Path::new("/tmp/x.md")).unwrap();
    // TS strips leading `# ` before storing the description.
    assert_eq!(skill.description, "My Skill Heading");
    assert!(!skill.has_user_specified_description);
}

#[test]
fn test_extract_description_caps_at_100_chars() {
    let long = "a".repeat(200);
    let got = extract_description_from_markdown(&long, "Skill");
    assert!(got.ends_with("..."), "long text gets ellipsis: {got}");
    assert_eq!(got.chars().count(), 100, "exactly 97 chars + '...'");
}

#[test]
fn test_extract_description_falls_back_to_default() {
    let got = extract_description_from_markdown("\n\n   \n", "Skill");
    assert_eq!(got, "Skill");
}

#[test]
fn test_progress_message_defaults_to_running() {
    let content = "---\ndescription: x\n---\nbody\n";
    let skill = parse_skill_markdown(content, Path::new("/tmp/x.md")).unwrap();
    assert_eq!(
        skill.progress_message.as_deref(),
        Some("running"),
        "TS createSkillCommand hard-codes progressMessage = 'running'"
    );
}

#[test]
fn test_finance_skills_real_world_example_loads() {
    // Verbatim SKILL.md from the alirezarezvani/claude-skills `finance`
    // bundle. Exercises: quoted strings in frontmatter, YAML sequences
    // (`tags`, `agents`), unknown fields (`author`, `license`, `tags`,
    // `agents`) silently ignored, multi-segment version (`1.0.0` parsed as
    // string), body containing its own `# Finance Skills` heading.
    let content = "\
---
name: \"finance-skills\"
description: \"Financial analyst agent skill and plugin for Claude Code, Codex, Gemini CLI, Cursor, OpenClaw. Ratio analysis, DCF valuation, budget variance, rolling forecasts. 4 Python tools (stdlib-only).\"
version: 1.0.0
author: Alireza Rezvani
license: MIT
tags:
  - finance
  - financial-analysis
agents:
  - claude-code
  - codex-cli
---

# Finance Skills

Production-ready financial analysis skill for strategic decision-making.

## Quick Start

### Claude Code
```
/read finance/financial-analyst/SKILL.md
```

### Codex CLI
```bash
npx agent-skills-cli add alirezarezvani/claude-skills/finance
```

## Skills Overview

| Skill | Folder | Focus |
|-------|--------|-------|
| Financial Analyst | `financial-analyst/` | Ratio analysis, DCF, budget variance, forecasting |

## Python Tools

4 scripts, all stdlib-only:

```bash
python3 financial-analyst/scripts/ratio_calculator.py --help
python3 financial-analyst/scripts/dcf_valuation.py --help
```

## Rules

- Load only the specific skill SKILL.md you need
- Always validate financial outputs against source data
";

    // Discover via the SKILL.md-in-directory convention so the name comes
    // from the directory (TS-strict; the frontmatter `name` field is
    // ignored for skill identity).
    let dir = tempfile::tempdir().unwrap();
    let skill_dir = dir.path().join("finance-skills");
    std::fs::create_dir(&skill_dir).unwrap();
    std::fs::write(skill_dir.join("SKILL.md"), content).unwrap();

    let skills = discover_skills(&[dir.path().to_path_buf()]);
    assert_eq!(skills.len(), 1, "finance-skills SKILL.md must load");

    let s = &skills[0];
    assert_eq!(s.name, "finance-skills");
    assert!(
        s.description
            .starts_with("Financial analyst agent skill and plugin"),
        "description must come from frontmatter (got: {:?})",
        s.description
    );
    assert!(
        s.description.contains("DCF valuation"),
        "description must be the full quoted string"
    );
    assert_eq!(s.version.as_deref(), Some("1.0.0"));

    // Body fields the schema does not model are silently ignored — the
    // file still loads. Body is preserved verbatim including the heading
    // `# Finance Skills` (TS does not strip Markdown headings from body).
    assert!(s.prompt.starts_with("# Finance Skills"));
    assert!(s.prompt.contains("Quick Start"));
    assert!(s.prompt.contains("Python Tools"));

    // Defaults that should not have been disturbed by unknown fields.
    assert!(s.user_invocable);
    assert!(!s.disable_model_invocation);
    assert!(!s.disabled);
    assert!(s.allowed_tools.is_none());
    assert!(s.argument_names.is_empty());
}
