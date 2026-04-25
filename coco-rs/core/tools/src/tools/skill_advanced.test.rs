use super::*;

// ── Argument expansion tests ──

#[test]
fn test_expand_skill_prompt_no_placeholders() {
    assert_eq!(
        expand_skill_prompt_simple("Do the thing", ""),
        "Do the thing"
    );
    assert_eq!(
        expand_skill_prompt_simple("Do the thing", "with args"),
        "Do the thing\n\nARGUMENTS: with args"
    );
}

#[test]
fn test_expand_skill_prompt_arguments_placeholder() {
    let result = expand_skill_prompt_simple("Review $ARGUMENTS for issues", "src/main.rs");
    assert_eq!(result, "Review src/main.rs for issues");
}

#[test]
fn test_expand_skill_prompt_braced_arguments() {
    let result = expand_skill_prompt_simple("Fix ${ARGUMENTS} now", "the bug");
    assert_eq!(result, "Fix the bug now");
}

#[test]
fn test_expand_skill_prompt_positional_args() {
    // TS parity (argumentSubstitution.ts:129-133): $N is
    // zero-indexed — alias for $ARGUMENTS[N]. So `$0` is the
    // first parsed arg, `$1` is the second.
    let result = expand_skill_prompt_simple("Compare $0 with $1", "old.rs new.rs");
    assert_eq!(result, "Compare old.rs with new.rs");
}

#[test]
fn test_expand_skill_prompt_positional_braced() {
    let result = expand_skill_prompt_simple("File: ${0}, line: ${1}", "main.rs 42");
    assert_eq!(result, "File: main.rs, line: 42");
}

#[test]
fn test_expand_skill_prompt_unused_positional_cleared() {
    // args "only one" → parsedArgs = ["only", "one"]:
    //   $0 = "only", $1 = "one", $2 = "" (out of range).
    let result = expand_skill_prompt_simple("$0 and $1 and $2", "only one");
    assert_eq!(result, "only and one and ");
}

// ── Variable substitution tests ──

#[test]
fn test_expand_skill_prompt_claude_skill_dir() {
    let result = expand_skill_prompt(
        "Run ${CLAUDE_SKILL_DIR}/helper.sh",
        &ExpandOptions {
            args: "",
            argument_names: &[],
            skill_dir: Some("/home/user/.claude/skills/my-skill"),
            session_id: None,
            base_dir: None,
            plugin_root: None,
            plugin_data_dir: None,
            user_config: None,
        },
    );
    assert_eq!(result, "Run /home/user/.claude/skills/my-skill/helper.sh");
}

#[test]
fn test_expand_skill_prompt_claude_session_id() {
    let result = expand_skill_prompt(
        "Session: ${CLAUDE_SESSION_ID}",
        &ExpandOptions {
            args: "",
            argument_names: &[],
            skill_dir: None,
            session_id: Some("abc-123"),
            base_dir: None,
            plugin_root: None,
            plugin_data_dir: None,
            user_config: None,
        },
    );
    assert_eq!(result, "Session: abc-123");
}

#[test]
fn test_expand_skill_prompt_all_variables() {
    let result = expand_skill_prompt(
        "Dir: ${CLAUDE_SKILL_DIR}, Session: ${CLAUDE_SESSION_ID}, Args: $ARGUMENTS",
        &ExpandOptions {
            args: "hello world",
            argument_names: &[],
            skill_dir: Some("/skills/test"),
            session_id: Some("sess-1"),
            base_dir: None,
            plugin_root: None,
            plugin_data_dir: None,
            user_config: None,
        },
    );
    assert_eq!(
        result,
        "Dir: /skills/test, Session: sess-1, Args: hello world"
    );
}

#[test]
fn test_expand_skill_prompt_no_skill_dir_leaves_placeholder() {
    let result = expand_skill_prompt(
        "Dir: ${CLAUDE_SKILL_DIR}",
        &ExpandOptions {
            args: "",
            argument_names: &[],
            skill_dir: None,
            session_id: None,
            base_dir: None,
            plugin_root: None,
            plugin_data_dir: None,
            user_config: None,
        },
    );
    // Without skill_dir, the placeholder remains
    assert_eq!(result, "Dir: ${CLAUDE_SKILL_DIR}");
}

// ── Named argument tests ──

#[test]
fn test_expand_skill_prompt_named_args() {
    let names = vec!["env".to_string(), "region".to_string()];
    let result = expand_skill_prompt(
        "Deploy to $env in $region",
        &ExpandOptions {
            args: "prod us-east-1",
            argument_names: &names,
            skill_dir: None,
            session_id: None,
            base_dir: None,
            plugin_root: None,
            plugin_data_dir: None,
            user_config: None,
        },
    );
    assert_eq!(result, "Deploy to prod in us-east-1");
}

#[test]
fn test_expand_skill_prompt_indexed_arguments() {
    let result =
        expand_skill_prompt_simple("First: $ARGUMENTS[0], Second: $ARGUMENTS[1]", "foo bar");
    assert_eq!(result, "First: foo, Second: bar");
}

// ── Base directory prepend tests ──

#[test]
fn test_expand_skill_prompt_base_dir() {
    let result = expand_skill_prompt(
        "Do stuff",
        &ExpandOptions {
            args: "",
            argument_names: &[],
            skill_dir: None,
            session_id: None,
            base_dir: Some("/home/user/.claude/skills/my-skill"),
            plugin_root: None,
            plugin_data_dir: None,
            user_config: None,
        },
    );
    assert!(
        result.starts_with("Base directory for this skill: /home/user/.claude/skills/my-skill\n\n")
    );
    assert!(result.ends_with("Do stuff"));
}

#[test]
fn test_expand_skill_prompt_base_dir_empty_skipped() {
    let result = expand_skill_prompt(
        "Do stuff",
        &ExpandOptions {
            args: "",
            argument_names: &[],
            skill_dir: None,
            session_id: None,
            base_dir: Some(""),
            plugin_root: None,
            plugin_data_dir: None,
            user_config: None,
        },
    );
    assert_eq!(result, "Do stuff");
}

// ── Skill name tests ──

#[test]
fn test_normalize_skill_name() {
    assert_eq!(normalize_skill_name("/commit"), "commit");
    assert_eq!(normalize_skill_name("commit"), "commit");
    assert_eq!(normalize_skill_name("  /review-pr  "), "review-pr");
}

#[test]
fn test_validate_skill_name_valid() {
    assert_eq!(validate_skill_name("/commit"), Ok("commit"));
    assert_eq!(validate_skill_name("review-pr"), Ok("review-pr"));
}

#[test]
fn test_validate_skill_name_empty() {
    assert!(validate_skill_name("").is_err());
    assert!(validate_skill_name("  ").is_err());
    assert!(validate_skill_name("/").is_err());
}

#[test]
fn test_validate_skill_name_invalid_chars() {
    assert!(validate_skill_name("../escape").is_err());
    assert!(validate_skill_name("null\0byte").is_err());
}

// ── Rule matching tests ──

#[test]
fn test_skill_matches_rule_exact() {
    assert!(skill_matches_rule("commit", "commit"));
    assert!(skill_matches_rule("commit", "/commit"));
    assert!(!skill_matches_rule("commit", "review"));
}

#[test]
fn test_skill_matches_rule_wildcard() {
    assert!(skill_matches_rule("review-pr", "review:*"));
    assert!(skill_matches_rule("review-code", "/review:*"));
    assert!(!skill_matches_rule("commit", "review:*"));
}

// ── Tool computation tests ──

#[test]
fn test_compute_effective_tools_unrestricted() {
    let skill = ResolvedSkill {
        name: "test".into(),
        prompt: "do stuff".into(),
        source: SkillSource::Bundled,
        execution_mode: SkillExecutionMode::Inline,
        model_override: None,
        allowed_tools: vec![],
        disallowed_tools: vec!["Bash".into()],
        allow_model_invocation: true,
        effort: None,
    };
    let available = vec!["Read".into(), "Write".into(), "Bash".into()];
    let effective = compute_effective_tools(&skill, &available);
    assert_eq!(effective, vec!["Read", "Write"]);
}

#[test]
fn test_compute_effective_tools_restricted() {
    let skill = ResolvedSkill {
        name: "test".into(),
        prompt: "do stuff".into(),
        source: SkillSource::Bundled,
        execution_mode: SkillExecutionMode::Inline,
        model_override: None,
        allowed_tools: vec!["Read".into(), "Grep".into(), "Missing".into()],
        disallowed_tools: vec![],
        allow_model_invocation: true,
        effort: None,
    };
    let available = vec!["Read".into(), "Write".into(), "Grep".into()];
    let effective = compute_effective_tools(&skill, &available);
    // "Missing" is not in available, so excluded
    assert_eq!(effective, vec!["Read", "Grep"]);
}

// ── Execution mode tests ──

#[test]
fn test_determine_execution_mode_force_inline() {
    let skill = ResolvedSkill {
        name: "test".into(),
        prompt: "do stuff".into(),
        source: SkillSource::Bundled,
        execution_mode: SkillExecutionMode::Forked,
        model_override: None,
        allowed_tools: vec![],
        disallowed_tools: vec![],
        allow_model_invocation: true,
        effort: None,
    };
    assert_eq!(
        determine_execution_mode(&skill, /*force_inline*/ true),
        SkillExecutionMode::Inline
    );
    assert_eq!(
        determine_execution_mode(&skill, /*force_inline*/ false),
        SkillExecutionMode::Forked
    );
}

// ── Output building tests ──

#[test]
fn test_build_inline_output() {
    let skill = ResolvedSkill {
        name: "commit".into(),
        prompt: "create a commit".into(),
        source: SkillSource::Bundled,
        execution_mode: SkillExecutionMode::Inline,
        model_override: Some("fast".into()),
        allowed_tools: vec!["Bash".into(), "Read".into()],
        disallowed_tools: vec![],
        allow_model_invocation: true,
        effort: None,
    };
    let output = build_inline_output(&skill);
    assert!(output.success);
    assert_eq!(output.command_name, "commit");
    assert_eq!(output.status, SkillExecutionMode::Inline);
    assert_eq!(output.model, Some("fast".into()));
    assert_eq!(
        output.allowed_tools,
        Some(vec!["Bash".into(), "Read".into()])
    );
    assert!(output.result.is_none());
    assert!(output.agent_id.is_none());
}

#[test]
fn test_build_forked_output() {
    let output = build_forked_output("review-pr", "agent-123", "PR looks good");
    assert!(output.success);
    assert_eq!(output.command_name, "review-pr");
    assert_eq!(output.status, SkillExecutionMode::Forked);
    assert_eq!(output.result, Some("PR looks good".into()));
    assert_eq!(output.agent_id, Some("agent-123".into()));
    assert!(output.allowed_tools.is_none());
}

// ── SkillSource tests ──

// ── Plugin variable substitution tests ──

#[test]
fn test_expand_skill_prompt_plugin_root() {
    let result = expand_skill_prompt(
        "Run ${CLAUDE_PLUGIN_ROOT}/scripts/setup.sh",
        &ExpandOptions {
            args: "",
            argument_names: &[],
            skill_dir: None,
            session_id: None,
            base_dir: None,
            plugin_root: Some("/home/user/.claude/plugins/cache/mkt/my-plugin"),
            plugin_data_dir: None,
            user_config: None,
        },
    );
    assert_eq!(
        result,
        "Run /home/user/.claude/plugins/cache/mkt/my-plugin/scripts/setup.sh"
    );
}

#[test]
fn test_expand_skill_prompt_plugin_data() {
    let result = expand_skill_prompt(
        "Save to ${CLAUDE_PLUGIN_DATA}/output.json",
        &ExpandOptions {
            args: "",
            argument_names: &[],
            skill_dir: None,
            session_id: None,
            base_dir: None,
            plugin_root: None,
            plugin_data_dir: Some("/home/user/.claude/plugins/data/my-plugin"),
            user_config: None,
        },
    );
    assert_eq!(
        result,
        "Save to /home/user/.claude/plugins/data/my-plugin/output.json"
    );
}

#[test]
fn test_expand_skill_prompt_user_config_nonsensitive() {
    let config = [("api_url", "https://api.example.com", false)];
    let result = expand_skill_prompt(
        "Connect to ${user_config.api_url}",
        &ExpandOptions {
            args: "",
            argument_names: &[],
            skill_dir: None,
            session_id: None,
            base_dir: None,
            plugin_root: None,
            plugin_data_dir: None,
            user_config: Some(&config),
        },
    );
    assert_eq!(result, "Connect to https://api.example.com");
}

#[test]
fn test_expand_skill_prompt_user_config_sensitive_masked() {
    let config = [("api_key", "sk-secret-123", true)];
    let result = expand_skill_prompt(
        "Auth with ${user_config.api_key}",
        &ExpandOptions {
            args: "",
            argument_names: &[],
            skill_dir: None,
            session_id: None,
            base_dir: None,
            plugin_root: None,
            plugin_data_dir: None,
            user_config: Some(&config),
        },
    );
    assert_eq!(result, "Auth with [SENSITIVE:api_key]");
}

#[test]
fn test_expand_skill_prompt_all_plugin_variables() {
    let config = [
        ("url", "https://example.com", false),
        ("token", "secret", true),
    ];
    let result = expand_skill_prompt(
        "Root: ${CLAUDE_PLUGIN_ROOT}, Data: ${CLAUDE_PLUGIN_DATA}, URL: ${user_config.url}, Token: ${user_config.token}",
        &ExpandOptions {
            args: "",
            argument_names: &[],
            skill_dir: None,
            session_id: None,
            base_dir: None,
            plugin_root: Some("/plugins/test"),
            plugin_data_dir: Some("/data/test"),
            user_config: Some(&config),
        },
    );
    assert_eq!(
        result,
        "Root: /plugins/test, Data: /data/test, URL: https://example.com, Token: [SENSITIVE:token]"
    );
}

// ── SkillSource tests ──

#[test]
fn test_skill_source_managed_variant() {
    let source = SkillSource::Managed;
    assert_eq!(source, SkillSource::Managed);
}

#[test]
fn test_skill_source_mcp_variant() {
    let source = SkillSource::Mcp;
    assert_eq!(source, SkillSource::Mcp);
}
