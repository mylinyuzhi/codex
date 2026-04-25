use coco_types::{AgentColorName, AgentSource, SubagentType};
use pretty_assertions::assert_eq;
use std::path::PathBuf;
use tempfile::TempDir;

use super::*;
use crate::builtins::BuiltinAgentCatalog;
use crate::definition_store::{AgentDefinitionStore, AgentSearchPaths};
use crate::filter::{AgentToolFilter, ToolFilterContext, parse_allowed_agent_types};
use crate::prompt::{AgentToolPromptRenderer, PromptOptions, format_tools_description};

// ── builtin catalog ──

#[test]
fn builtin_catalog_includes_required_when_enabled() {
    let catalog = BuiltinAgentCatalog::all_enabled();
    let defs = builtins::builtin_definitions(catalog);
    let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "general-purpose",
            "statusline-setup",
            "Explore",
            "Plan",
            "verification",
            "claude-code-guide",
        ]
    );
}

#[test]
fn builtin_catalog_excludes_optional_when_disabled() {
    let defs = builtins::builtin_definitions(BuiltinAgentCatalog::default());
    let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, vec!["general-purpose", "statusline-setup"]);
}

#[test]
fn builtin_catalog_disable_all_returns_empty() {
    let catalog = BuiltinAgentCatalog {
        disable_all: true,
        ..BuiltinAgentCatalog::all_enabled()
    };
    assert!(builtins::builtin_definitions(catalog).is_empty());
}

#[test]
fn builtin_definition_lookup_uses_canonical_case() {
    // PascalCase Explore must hit; lowercase must not.
    assert!(builtins::builtin_definition("Explore").is_some());
    assert!(builtins::builtin_definition("explore").is_none());
    assert!(builtins::builtin_definition("verification").is_some());
    assert!(builtins::builtin_definition("Verification").is_none());
}

#[test]
fn explore_built_in_omits_claude_md_and_blocks_writes() {
    let def = builtins::builtin_definition("Explore").unwrap();
    assert!(def.omit_claude_md);
    // TS exploreAgent.ts:78 picks `haiku` for non-ant builds (the SDK
    // default); ant-only builds use `inherit` for cache parity. Default
    // 3P here matches the SDK default.
    assert_eq!(def.model.as_deref(), Some("haiku"));
    // TS exploreAgent.ts:67-73 uses FILE_EDIT_TOOL_NAME = "Edit" and
    // FILE_WRITE_TOOL_NAME = "Write" — NOT "FileEdit"/"FileWrite".
    for blocked in ["Edit", "Write", "NotebookEdit", "Agent", "ExitPlanMode"] {
        assert!(
            def.disallowed_tools.iter().any(|t| t == blocked),
            "Explore should block {blocked}; actual: {:?}",
            def.disallowed_tools
        );
    }
}

#[test]
fn claude_code_guide_uses_dont_ask_permission_mode() {
    // TS claudeCodeGuideAgent.ts:120 sets permissionMode: 'dontAsk' so the
    // guide can run its allow-listed tools without prompting.
    let def = builtins::builtin_definition("claude-code-guide").unwrap();
    assert_eq!(def.permission_mode.as_deref(), Some("dontAsk"));
    assert_eq!(
        def.allowed_tools,
        vec!["Glob", "Grep", "Read", "WebFetch", "WebSearch"]
    );
}

#[test]
fn verification_built_in_runs_in_background_with_red_color() {
    let def = builtins::builtin_definition("verification").unwrap();
    assert!(def.background);
    assert_eq!(def.color, Some(AgentColorName::Red));
}

#[test]
fn statusline_built_in_uses_orange_color_and_sonnet() {
    let def = builtins::builtin_definition("statusline-setup").unwrap();
    assert_eq!(def.color, Some(AgentColorName::Orange));
    assert_eq!(def.model.as_deref(), Some("sonnet"));
    assert_eq!(def.allowed_tools, vec!["Read", "Edit"]);
}

// ── one-shot constants ──

#[test]
fn one_shot_set_is_case_sensitive_explore_plan() {
    assert!(ONE_SHOT_BUILTIN_AGENT_TYPES.contains(&"Explore"));
    assert!(ONE_SHOT_BUILTIN_AGENT_TYPES.contains(&"Plan"));
    assert!(!ONE_SHOT_BUILTIN_AGENT_TYPES.contains(&"explore"));
    assert!(!ONE_SHOT_BUILTIN_AGENT_TYPES.contains(&"plan"));
    assert!(!ONE_SHOT_BUILTIN_AGENT_TYPES.contains(&"verification"));
    assert_eq!(ONE_SHOT_BUILTIN_AGENT_TYPES.len(), 2);
}

#[test]
fn empty_output_marker_matches_ts_literal() {
    assert_eq!(
        EMPTY_AGENT_OUTPUT_MARKER,
        "(Subagent completed but returned no output.)"
    );
}

// ── tools description ──

#[test]
fn tools_description_all_branches() {
    assert_eq!(format_tools_description(&[], &[]), "All tools");
    assert_eq!(
        format_tools_description(&[], &["Bash".into(), "Edit".into()]),
        "All tools except Bash, Edit"
    );
    assert_eq!(
        format_tools_description(&["Read".into(), "Grep".into()], &[]),
        "Read, Grep"
    );
    assert_eq!(
        format_tools_description(
            &["Read".into(), "Grep".into(), "Bash".into()],
            &["Bash".into()]
        ),
        "Read, Grep"
    );
    assert_eq!(
        format_tools_description(&["Bash".into()], &["Bash".into()]),
        "None"
    );
}

// ── allowed agent types parser ──

#[test]
fn parse_allowed_agent_types_handles_agent_and_task() {
    let from_agent = parse_allowed_agent_types("Agent(Explore, Plan)").unwrap();
    let from_task = parse_allowed_agent_types("Task(Explore,Plan)").unwrap();
    assert_eq!(from_agent.names, vec!["Explore", "Plan"]);
    assert_eq!(from_task.names, vec!["Explore", "Plan"]);
    assert!(from_agent.matches("Explore"));
    assert!(!from_agent.matches("explore"));
}

#[test]
fn parse_allowed_agent_types_ignores_unrelated_rules() {
    assert!(parse_allowed_agent_types("Bash(npm test)").is_none());
    assert!(parse_allowed_agent_types("Read").is_none());
}

#[test]
fn parse_allowed_agent_types_bare_agent_means_no_restriction() {
    // TS regex captures group 2 as undefined for bare `Agent`; the runtime
    // treats undefined / empty as "no restriction". Returning None lets
    // callers skip the matching step entirely.
    assert!(parse_allowed_agent_types("Agent").is_none());
    assert!(parse_allowed_agent_types("Agent()").is_none());
    assert!(parse_allowed_agent_types("Task").is_none());
    assert!(parse_allowed_agent_types("Task()").is_none());
}

#[test]
fn allowed_agent_types_empty_names_means_match_all() {
    // If a future caller constructs AllowedAgentTypes with an empty list
    // explicitly, matches() must return true (no restriction).
    let unrestricted = filter::AllowedAgentTypes { names: vec![] };
    assert!(unrestricted.matches("Explore"));
    assert!(unrestricted.matches("anything"));
}

// ── tool filter plan ──

fn agent(name: &str, allowed: &[&str], denied: &[&str]) -> coco_types::AgentDefinition {
    coco_types::AgentDefinition {
        agent_type: coco_types::AgentTypeId::Custom(name.into()),
        name: name.into(),
        when_to_use: Some("test".into()),
        description: Some("test".into()),
        source: AgentSource::ProjectSettings,
        allowed_tools: allowed.iter().map(|s| (*s).to_owned()).collect(),
        disallowed_tools: denied.iter().map(|s| (*s).to_owned()).collect(),
        ..Default::default()
    }
}

fn ctx<'a>(tools: &'a [String]) -> ToolFilterContext<'a> {
    ToolFilterContext {
        available_tools: tools,
        is_builtin: false,
        is_async: false,
        plan_mode: false,
        extra_allow_list: None,
    }
}

#[test]
fn filter_plan_default_keeps_safe_tools_and_blocks_universal() {
    let tools: Vec<String> = vec![
        "Read".into(),
        "Bash".into(),
        "Agent".into(),
        "AskUserQuestion".into(),
        "TaskOutput".into(),
        "TaskStop".into(),
        "EnterPlanMode".into(),
        "ExitPlanMode".into(),
    ];
    let def = agent("custom", &[], &[]);
    let plan = AgentToolFilter::plan(&def, ctx(&tools));
    assert!(plan.uses_default_allow_list);
    // Every entry of ALL_AGENT_DISALLOWED_TOOLS must be removed.
    assert_eq!(plan.allowed_tools, vec!["Read", "Bash"]);
}

#[test]
fn filter_plan_async_safe_set_excludes_repl() {
    // TS SHELL_TOOL_NAMES = [Bash, PowerShell] only — REPL is NOT
    // async-safe (`utils/shell/shellToolUtils.ts:6`).
    let tools: Vec<String> = vec!["Bash".into(), "PowerShell".into(), "REPL".into()];
    let def = agent("custom", &[], &[]);
    let mut filter_ctx = ctx(&tools);
    filter_ctx.is_async = true;
    let plan = AgentToolFilter::plan(&def, filter_ctx);
    assert_eq!(plan.allowed_tools, vec!["Bash", "PowerShell"]);
}

#[test]
fn filter_plan_allow_list_does_not_pass_mcp_through() {
    // TS `resolveAgentTools` (`agentToolUtils.ts:175-216`) builds the
    // available-tool map from `allowedAvailableTools` and only includes a
    // tool if `agentTools` lists it BY NAME. MCP tools must NOT survive
    // an explicit allow-list unless the allow-list lists them.
    let tools: Vec<String> = vec!["Read".into(), "mcp__slack__send".into()];
    let def = agent("custom", &["Read"], &[]);
    let plan = AgentToolFilter::plan(&def, ctx(&tools));
    assert_eq!(plan.allowed_tools, vec!["Read"]);
}

#[test]
fn filter_plan_extra_allow_list_does_not_pass_mcp_through() {
    let tools: Vec<String> = vec!["Read".into(), "mcp__slack__send".into()];
    let def = agent("custom", &[], &[]);
    let mut filter_ctx = ctx(&tools);
    let extras: Vec<String> = vec!["Read".into()];
    filter_ctx.extra_allow_list = Some(&extras);
    let plan = AgentToolFilter::plan(&def, filter_ctx);
    assert_eq!(plan.allowed_tools, vec!["Read"]);
}

#[test]
fn filter_plan_deny_then_allow_marks_double_listed_tool_as_unknown() {
    // TS deny applies to `filteredAvailableTools` BEFORE the allow-list
    // intersection. A tool listed in BOTH allow and deny is `invalidTool`
    // (= unknown_tools in coco-rs).
    let tools: Vec<String> = vec!["Read".into(), "Bash".into()];
    let def = agent("custom", &["Read", "Bash"], &["Bash"]);
    let plan = AgentToolFilter::plan(&def, ctx(&tools));
    assert_eq!(plan.allowed_tools, vec!["Read"]);
    assert_eq!(plan.unknown_tools, vec!["Bash"]);
}

#[test]
fn frontmatter_wildcard_tools_collapses_to_default_allow_list() {
    // TS `parseAgentToolsFromFrontmatter` (`utils/markdownConfigLoader.ts:122-124`)
    // turns `tools: ['*']` into `undefined` (= use default allow set).
    // Coco-rs represents that with an empty allow-list.
    let project = TempDir::new().unwrap();
    write_md(
        project.path(),
        "wild.md",
        "---\nname: wild\ndescription: wildcard\ntools:\n  - '*'\n---\nbody",
    );
    let mut store = AgentDefinitionStore::new(
        BuiltinAgentCatalog::default(),
        AgentSearchPaths {
            project_dirs: vec![project.path().to_path_buf()],
            ..Default::default()
        },
    );
    store.load();
    let def = store.snapshot().find_active("wild").cloned().unwrap();
    assert!(def.allowed_tools.is_empty(), "got: {:?}", def.allowed_tools);
}

#[test]
fn frontmatter_description_unescapes_backslash_n() {
    // TS `loadAgentsDir.ts:565` does `.replace(/\\n/g, '\n')`.
    let project = TempDir::new().unwrap();
    write_md(
        project.path(),
        "multi.md",
        "---\nname: multi\ndescription: \"line1\\nline2\"\n---\nbody",
    );
    let mut store = AgentDefinitionStore::new(
        BuiltinAgentCatalog::default(),
        AgentSearchPaths {
            project_dirs: vec![project.path().to_path_buf()],
            ..Default::default()
        },
    );
    store.load();
    let def = store.snapshot().find_active("multi").cloned().unwrap();
    assert_eq!(def.when_to_use.as_deref(), Some("line1\nline2"));
}

#[test]
fn frontmatter_invalid_effort_warns_and_drops() {
    let project = TempDir::new().unwrap();
    write_md(
        project.path(),
        "bad.md",
        "---\nname: bad\ndescription: x\neffort: potato\n---\nbody",
    );
    let mut store = AgentDefinitionStore::new(
        BuiltinAgentCatalog::default(),
        AgentSearchPaths {
            project_dirs: vec![project.path().to_path_buf()],
            ..Default::default()
        },
    );
    store.load();
    let def = store.snapshot().find_active("bad").cloned().unwrap();
    assert!(def.effort.is_none());
    assert!(!store.last_report().warnings.is_empty());
}

#[test]
fn frontmatter_invalid_permission_mode_warns_and_drops() {
    let project = TempDir::new().unwrap();
    write_md(
        project.path(),
        "bad.md",
        "---\nname: bad\ndescription: x\npermissionMode: yolo\n---\nbody",
    );
    let mut store = AgentDefinitionStore::new(
        BuiltinAgentCatalog::default(),
        AgentSearchPaths {
            project_dirs: vec![project.path().to_path_buf()],
            ..Default::default()
        },
    );
    store.load();
    let def = store.snapshot().find_active("bad").cloned().unwrap();
    assert!(def.permission_mode.is_none());
    let warnings = &store.last_report().warnings;
    assert!(
        warnings.iter().any(|w| matches!(
            w.error,
            crate::validation::ValidationError::InvalidPermissionMode { .. }
        )),
        "got: {warnings:?}"
    );
}

#[test]
fn load_report_distinguishes_failures_from_warnings() {
    // A recoverable warning (invalid color dropped) should leave the
    // report failure-free; only true parse / validation failures count.
    let project = TempDir::new().unwrap();
    write_md(
        project.path(),
        "warn.md",
        "---\nname: warn\ndescription: x\ncolor: chartreuse\n---\nbody",
    );
    let mut store = AgentDefinitionStore::new(
        BuiltinAgentCatalog::default(),
        AgentSearchPaths {
            project_dirs: vec![project.path().to_path_buf()],
            ..Default::default()
        },
    );
    store.load();
    let report = store.last_report();
    assert!(!report.is_silent());
    assert!(!report.has_failures());
}

#[test]
fn filter_plan_sync_plan_mode_keeps_exit_plan_mode() {
    // TS agentToolUtils.ts:88-93 bypasses ExitPlanMode for plan_mode BEFORE
    // the universal block — applies to sync agents too, not just async.
    let tools: Vec<String> = vec!["Read".into(), "ExitPlanMode".into()];
    let def = agent("planner", &[], &[]);
    let mut filter_ctx = ctx(&tools);
    filter_ctx.plan_mode = true; // sync, not async
    let plan = AgentToolFilter::plan(&def, filter_ctx);
    assert!(plan.allowed_tools.contains(&"ExitPlanMode".to_owned()));
}

#[test]
fn filter_plan_async_restricts_to_async_safe_set() {
    let tools: Vec<String> = vec![
        "Read".into(),
        "Bash".into(),
        "AskUserQuestion".into(),
        "EnterPlanMode".into(),
    ];
    let def = agent("custom", &[], &[]);
    let mut filter_ctx = ctx(&tools);
    filter_ctx.is_async = true;
    let plan = AgentToolFilter::plan(&def, filter_ctx);
    // EnterPlanMode is not in async-safe set; Read+Bash are.
    assert_eq!(plan.allowed_tools, vec!["Read", "Bash"]);
}

#[test]
fn filter_plan_async_plan_mode_keeps_exit_plan_mode() {
    let tools: Vec<String> = vec!["Read".into(), "ExitPlanMode".into()];
    let def = agent("planner", &[], &[]);
    let mut filter_ctx = ctx(&tools);
    filter_ctx.is_async = true;
    filter_ctx.plan_mode = true;
    let plan = AgentToolFilter::plan(&def, filter_ctx);
    assert!(plan.allowed_tools.contains(&"ExitPlanMode".to_owned()));
}

#[test]
fn filter_plan_allow_list_intersection_records_unknown_tools() {
    let tools: Vec<String> = vec!["Read".into(), "Bash".into()];
    let def = agent("custom", &["Read", "ImaginaryTool"], &[]);
    let plan = AgentToolFilter::plan(&def, ctx(&tools));
    assert_eq!(plan.allowed_tools, vec!["Read"]);
    assert_eq!(plan.unknown_tools, vec!["ImaginaryTool"]);
    assert!(!plan.uses_default_allow_list);
}

#[test]
fn filter_plan_deny_list_overrides_allow_list() {
    let tools: Vec<String> = vec!["Read".into(), "Bash".into()];
    let def = agent("custom", &["Read", "Bash"], &["Bash"]);
    let plan = AgentToolFilter::plan(&def, ctx(&tools));
    assert_eq!(plan.allowed_tools, vec!["Read"]);
}

#[test]
fn filter_plan_mcp_tools_bypass_universal_block() {
    let tools: Vec<String> = vec![
        "Agent".into(),
        "mcp__slack__send".into(),
        "AskUserQuestion".into(),
    ];
    let def = agent("custom", &[], &[]);
    let plan = AgentToolFilter::plan(&def, ctx(&tools));
    assert_eq!(plan.allowed_tools, vec!["mcp__slack__send"]);
}

#[test]
fn filter_plan_extra_allow_list_intersects() {
    let tools: Vec<String> = vec!["Read".into(), "Bash".into(), "Grep".into()];
    let def = agent("custom", &[], &[]);
    let mut filter_ctx = ctx(&tools);
    let extras: Vec<String> = vec!["Read".into(), "Grep".into()];
    filter_ctx.extra_allow_list = Some(&extras);
    let plan = AgentToolFilter::plan(&def, filter_ctx);
    assert_eq!(plan.allowed_tools, vec!["Read", "Grep"]);
}

// ── definition store + prompt rendering ──

fn write_md(dir: &std::path::Path, name: &str, content: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

#[test]
fn store_loads_user_then_project_with_project_winning() {
    let user = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();

    write_md(
        user.path(),
        "build.md",
        "---\nname: build\ndescription: User build\nmodel: haiku\n---\nuser body",
    );
    write_md(
        project.path(),
        "build.md",
        "---\nname: build\ndescription: Project build\nmodel: sonnet\n---\nproject body",
    );

    let mut store = AgentDefinitionStore::new(
        BuiltinAgentCatalog::default(),
        AgentSearchPaths {
            user_dir: Some(user.path().to_path_buf()),
            project_dirs: vec![project.path().to_path_buf()],
            ..Default::default()
        },
    );
    store.load();
    let snap = store.snapshot();
    let active = snap.find_active("build").unwrap();
    assert_eq!(active.source, AgentSource::ProjectSettings);
    assert_eq!(active.model.as_deref(), Some("sonnet"));
    assert_eq!(active.system_prompt.as_deref(), Some("project body"));
}

#[test]
fn store_records_failed_files() {
    let project = TempDir::new().unwrap();
    write_md(
        project.path(),
        "broken.md",
        "---\ndescription: missing name\n---\nbody",
    );
    let mut store = AgentDefinitionStore::new(
        BuiltinAgentCatalog::default(),
        AgentSearchPaths {
            project_dirs: vec![project.path().to_path_buf()],
            ..Default::default()
        },
    );
    let report = store.load().clone();
    let snap = store.snapshot();
    assert!(snap.find_active("broken").is_none());
    assert_eq!(report.failed.len(), 1);
}

#[test]
fn store_records_color_warning_for_invalid_color() {
    // Invalid color is dropped and surfaces as a warning, while the
    // definition still loads and enters the active set.
    let project = TempDir::new().unwrap();
    write_md(
        project.path(),
        "fancy.md",
        "---\nname: fancy\ndescription: A colorful agent\ncolor: chartreuse\n---\nbody",
    );
    let mut store = AgentDefinitionStore::new(
        BuiltinAgentCatalog::default(),
        AgentSearchPaths {
            project_dirs: vec![project.path().to_path_buf()],
            ..Default::default()
        },
    );
    store.load();
    let snap = store.snapshot();
    assert!(snap.find_active("fancy").unwrap().color.is_none());
    let warnings = &store.last_report().warnings;
    assert_eq!(warnings.len(), 1);
    assert!(matches!(
        warnings[0].error,
        crate::validation::ValidationError::InvalidColor { .. }
    ));
}

#[test]
fn store_priority_chain_policy_overrides_everything() {
    let plugin = TempDir::new().unwrap();
    let user = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let flag = TempDir::new().unwrap();
    let policy = TempDir::new().unwrap();
    for (dir, model_label) in [
        (&plugin, "plugin-model"),
        (&user, "user-model"),
        (&project, "project-model"),
        (&flag, "flag-model"),
        (&policy, "policy-model"),
    ] {
        write_md(
            dir.path(),
            "build.md",
            &format!("---\nname: build\ndescription: build agent\nmodel: {model_label}\n---\nbody"),
        );
    }
    let mut store = AgentDefinitionStore::new(
        BuiltinAgentCatalog::default(),
        AgentSearchPaths {
            user_dir: Some(user.path().to_path_buf()),
            project_dirs: vec![project.path().to_path_buf()],
            flag_dirs: vec![flag.path().to_path_buf()],
            policy_dirs: vec![policy.path().to_path_buf()],
            plugin_dirs: vec![plugin.path().to_path_buf()],
        },
    );
    store.load();
    let active = store.snapshot();
    let build = active.find_active("build").unwrap();
    assert_eq!(build.source, AgentSource::PolicySettings);
    assert_eq!(build.model.as_deref(), Some("policy-model"));
}

#[test]
fn store_intra_dir_load_order_is_deterministic() {
    // Two same-name agents in one project dir resolve identically across
    // platforms (alphabetical filename wins because last-loaded wins).
    let project = TempDir::new().unwrap();
    write_md(
        project.path(),
        "a-build.md",
        "---\nname: build\ndescription: A\n---\nbody-a",
    );
    write_md(
        project.path(),
        "z-build.md",
        "---\nname: build\ndescription: Z\n---\nbody-z",
    );
    let mut store = AgentDefinitionStore::new(
        BuiltinAgentCatalog::default(),
        AgentSearchPaths {
            project_dirs: vec![project.path().to_path_buf()],
            ..Default::default()
        },
    );
    store.load();
    let snap = store.snapshot();
    let build = snap.find_active("build").unwrap();
    assert_eq!(build.system_prompt.as_deref(), Some("body-z"));
}

#[test]
fn frontmatter_tools_csv_string_is_split_on_commas() {
    let project = TempDir::new().unwrap();
    write_md(
        project.path(),
        "csv.md",
        "---\nname: csv\ndescription: CSV tools\ntools: Read, Edit, Write\n---\nbody",
    );
    let mut store = AgentDefinitionStore::new(
        BuiltinAgentCatalog::default(),
        AgentSearchPaths {
            project_dirs: vec![project.path().to_path_buf()],
            ..Default::default()
        },
    );
    store.load();
    let snap = store.snapshot();
    let def = snap.find_active("csv").unwrap();
    assert_eq!(def.allowed_tools, vec!["Read", "Edit", "Write"]);
}

#[test]
fn store_includes_builtins_alongside_custom() {
    let project = TempDir::new().unwrap();
    write_md(
        project.path(),
        "build.md",
        "---\nname: build\ndescription: Custom build agent\n---\nbody",
    );
    let mut store = AgentDefinitionStore::new(
        BuiltinAgentCatalog::all_enabled(),
        AgentSearchPaths {
            project_dirs: vec![project.path().to_path_buf()],
            ..Default::default()
        },
    );
    store.load();
    let snap = store.snapshot();
    assert!(snap.find_active("Explore").is_some());
    assert!(snap.find_active("build").is_some());
    assert!(snap.find_active("general-purpose").is_some());
}

#[test]
fn prompt_lists_active_agents_in_alphabetical_order() {
    let project = TempDir::new().unwrap();
    write_md(
        project.path(),
        "build.md",
        "---\nname: build\ndescription: Build verification\ntools:\n  - Bash\n  - Read\n---\nbody",
    );
    let mut store = AgentDefinitionStore::new(
        BuiltinAgentCatalog::default(),
        AgentSearchPaths {
            project_dirs: vec![project.path().to_path_buf()],
            ..Default::default()
        },
    );
    store.load();
    let snap = store.snapshot();
    let renderer = AgentToolPromptRenderer::new(&snap);
    let listing = renderer.agent_list(&PromptOptions::default());
    let lines: Vec<&str> = listing.lines().collect();
    assert_eq!(
        lines,
        vec![
            "- build: Build verification (Tools: Bash, Read)",
            "- general-purpose: General-purpose agent for researching complex questions, searching for code, and executing multi-step tasks. (Tools: All tools)",
            "- statusline-setup: Use this agent to configure the user's Claude Code status line setting. (Tools: Read, Edit)",
        ]
    );
}

#[test]
fn prompt_filters_by_allowed_agent_types() {
    let mut store = AgentDefinitionStore::new(
        BuiltinAgentCatalog::all_enabled(),
        AgentSearchPaths::empty(),
    );
    store.load();
    let snap = store.snapshot();
    let renderer = AgentToolPromptRenderer::new(&snap);
    let listing = renderer.agent_list(&PromptOptions {
        allowed_agent_types: Some(vec![SubagentType::Explore.as_str().to_owned()]),
        ..Default::default()
    });
    assert_eq!(listing.lines().count(), 1);
    assert!(listing.starts_with("- Explore: "));
}
