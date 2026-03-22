use super::*;
use crate::interface::ArgumentDef;
use std::path::PathBuf;

// ── parse_skill_args ──

#[test]
fn test_parse_simple() {
    assert_eq!(parse_skill_args("foo bar baz"), vec!["foo", "bar", "baz"]);
}

#[test]
fn test_parse_double_quoted() {
    assert_eq!(
        parse_skill_args(r#"foo "hello world" bar"#),
        vec!["foo", "hello world", "bar"]
    );
}

#[test]
fn test_parse_single_quoted() {
    assert_eq!(
        parse_skill_args("foo 'hello world' bar"),
        vec!["foo", "hello world", "bar"]
    );
}

#[test]
fn test_parse_empty() {
    assert!(parse_skill_args("").is_empty());
}

#[test]
fn test_parse_escaped_double_quote() {
    // say \"hello\" → ["say", "\"hello\""]
    let args = parse_skill_args(r#"say \"hello\""#);
    assert_eq!(args, vec!["say", r#""hello""#]);
}

#[test]
fn test_parse_backslash_in_double_quotes() {
    // "say \"hi\"" → ["say \"hi\""]  (backslash escapes inside double quotes)
    let args = parse_skill_args(r#""say \"hi\"""#);
    assert_eq!(args, vec![r#"say "hi""#]);
}

#[test]
fn test_parse_backslash_in_single_quotes_literal() {
    // In single quotes, backslash is literal
    let args = parse_skill_args(r"'hello\nworld'");
    assert_eq!(args, vec![r"hello\nworld"]);
}

#[test]
fn test_parse_escaped_space() {
    // hello\ world → ["hello world"]
    let args = parse_skill_args(r"hello\ world");
    assert_eq!(args, vec!["hello world"]);
}

// ── substitute_skill_args ──

#[test]
fn test_substitute_arguments_placeholder() {
    let result = substitute_skill_args("Review PR #$ARGUMENTS", "123", None, None);
    assert_eq!(result, "Review PR #123");
}

#[test]
fn test_substitute_no_placeholder_no_args() {
    let result = substitute_skill_args("Generate a commit message", "", None, None);
    assert_eq!(result, "Generate a commit message");
}

#[test]
fn test_substitute_no_placeholder_with_args() {
    let result = substitute_skill_args("Generate a commit message", "--amend", None, None);
    assert_eq!(result, "Generate a commit message\n\nArguments: --amend");
}

#[test]
fn test_substitute_positional_args() {
    let defs = vec![
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
    ];
    let result = substitute_skill_args(
        "Hello $1, welcome to $2",
        "Alice Wonderland",
        Some(&defs),
        None,
    );
    assert_eq!(result, "Hello Alice, welcome to Wonderland");
}

#[test]
fn test_substitute_named_args() {
    let defs = vec![
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
    ];
    let result = substitute_skill_args(
        "Deploy ${args.env} with ${args.tag}",
        "prod v1.2.3",
        Some(&defs),
        None,
    );
    assert_eq!(result, "Deploy prod with v1.2.3");
}

#[test]
fn test_substitute_skill_dir() {
    let base = PathBuf::from("/project/skills/deploy");
    let result = substitute_skill_args("Run ${COCODE_SKILL_DIR}/deploy.sh", "", None, Some(&base));
    assert!(result.contains("Run /project/skills/deploy/deploy.sh"));
    assert!(result.contains("Base directory for this skill: /project/skills/deploy"));
}

#[test]
fn test_substitute_base_dir_prefix() {
    let base = PathBuf::from("/skills/deploy");
    let result = substitute_skill_args("Deploy the app", "", None, Some(&base));
    assert!(result.starts_with("Base directory for this skill: /skills/deploy\n\n"));
    assert!(result.ends_with("Deploy the app"));
}

#[test]
fn test_substitute_missing_positional_defaults_to_empty() {
    let defs = vec![ArgumentDef {
        name: "name".to_string(),
        description: None,
        required: false,
    }];
    // No args provided — $1 becomes ""
    let result = substitute_skill_args("Hello $1!", "", Some(&defs), None);
    assert_eq!(result, "Hello !");
}
