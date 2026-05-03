use super::*;
use coco_subagent::definition_store::AgentSearchPaths;
use std::path::PathBuf;

fn empty_paths() -> AgentSearchPaths {
    AgentSearchPaths {
        user_dir: None,
        project_dirs: Vec::<PathBuf>::new(),
        flag_dirs: Vec::<PathBuf>::new(),
        policy_dirs: Vec::<PathBuf>::new(),
        plugin_dirs: Vec::<PathBuf>::new(),
    }
}

#[test]
fn list_with_only_builtins_renders_count_and_each_agent() {
    let out = render("list", empty_paths()).unwrap();
    // Built-in catalog (interactive) ships at least general-purpose +
    // statusline-setup + Explore + Plan + claude-code-guide. Don't pin
    // exact count — just that the list is non-empty and well-formed.
    assert!(out.starts_with(|c: char| c.is_ascii_digit()), "got: {out}");
    assert!(out.contains("general-purpose"));
    assert!(out.contains("[built-in"));
}

#[test]
fn empty_subcommand_aliases_to_list() {
    let listed = render("list", empty_paths()).unwrap();
    let empty = render("", empty_paths()).unwrap();
    assert_eq!(listed, empty);
}

#[test]
fn show_unknown_agent_reports_not_found() {
    let out = render("show no-such-agent", empty_paths()).unwrap();
    assert!(out.contains("No active agent named"));
    assert!(out.contains("no-such-agent"));
}

#[test]
fn show_known_builtin_renders_metadata() {
    let out = render("show general-purpose", empty_paths()).unwrap();
    assert!(out.contains("# general-purpose"));
    assert!(out.contains("Source:"));
    assert!(out.contains("built-in"));
}

#[test]
fn show_without_name_returns_usage() {
    let out = render("show", empty_paths()).unwrap();
    assert!(out.contains("Usage: /agents show <name>"));
}

#[test]
fn paths_lists_built_in_first() {
    let out = render("paths", empty_paths()).unwrap();
    assert!(out.contains("built-in"));
}

#[test]
fn paths_includes_configured_dirs() {
    let paths = AgentSearchPaths {
        user_dir: Some(PathBuf::from("/home/u/.coco/agents")),
        project_dirs: vec![PathBuf::from("/proj/.claude/agents")],
        flag_dirs: Vec::<PathBuf>::new(),
        policy_dirs: Vec::<PathBuf>::new(),
        plugin_dirs: Vec::<PathBuf>::new(),
    };
    let out = render("paths", paths).unwrap();
    assert!(out.contains("/home/u/.coco/agents"));
    assert!(out.contains("/proj/.claude/agents"));
}

#[test]
fn validate_with_no_diagnostics_reports_clean() {
    let out = render("validate", empty_paths()).unwrap();
    assert!(out.contains("loaded with no warnings"));
}

#[test]
fn unknown_subcommand_returns_usage_hint() {
    let out = render("explode", empty_paths()).unwrap();
    assert!(out.contains("Unknown /agents subcommand: explode"));
    assert!(out.contains("Usage:"));
}
