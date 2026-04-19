use coco_types::*;
use std::collections::HashMap;

use super::*;

fn empty_context(mode: PermissionMode) -> ToolPermissionContext {
    ToolPermissionContext {
        mode,
        additional_dirs: HashMap::new(),
        allow_rules: HashMap::new(),
        deny_rules: HashMap::new(),
        ask_rules: HashMap::new(),
        bypass_available: false,
        pre_plan_mode: None,
        stripped_dangerous_rules: None,
        session_plan_file: None,
    }
}

fn bash_input(command: &str) -> serde_json::Value {
    serde_json::json!({"command": command})
}

fn file_input(path: &str) -> serde_json::Value {
    serde_json::json!({"file_path": path})
}

fn make_rule(
    tool: &str,
    content: Option<&str>,
    behavior: PermissionBehavior,
    source: PermissionRuleSource,
) -> PermissionRule {
    PermissionRule {
        source,
        behavior,
        value: PermissionRuleValue {
            tool_pattern: tool.to_string(),
            rule_content: content.map(String::from),
        },
    }
}

// ── Step 1: Deny rules ──

#[test]
fn test_deny_rule_wins_over_allow() {
    let mut ctx = empty_context(PermissionMode::Default);
    ctx.deny_rules.insert(
        PermissionRuleSource::UserSettings,
        vec![make_rule(
            "Bash",
            None,
            PermissionBehavior::Deny,
            PermissionRuleSource::UserSettings,
        )],
    );
    ctx.allow_rules.insert(
        PermissionRuleSource::Session,
        vec![make_rule(
            "*",
            None,
            PermissionBehavior::Allow,
            PermissionRuleSource::Session,
        )],
    );
    let result =
        PermissionEvaluator::evaluate(&ToolId::Builtin(ToolName::Bash), &bash_input("ls"), &ctx);
    assert!(matches!(result, PermissionDecision::Deny { .. }));
}

#[test]
fn test_content_specific_deny() {
    let mut ctx = empty_context(PermissionMode::Default);
    ctx.deny_rules.insert(
        PermissionRuleSource::ProjectSettings,
        vec![make_rule(
            "Bash",
            Some("rm *"),
            PermissionBehavior::Deny,
            PermissionRuleSource::ProjectSettings,
        )],
    );
    // "rm -rf /" matches "rm *" → deny
    let result = PermissionEvaluator::evaluate(
        &ToolId::Builtin(ToolName::Bash),
        &bash_input("rm -rf /"),
        &ctx,
    );
    assert!(matches!(result, PermissionDecision::Deny { .. }));

    // "ls" does NOT match "rm *" → not denied
    let result =
        PermissionEvaluator::evaluate(&ToolId::Builtin(ToolName::Bash), &bash_input("ls"), &ctx);
    assert!(matches!(result, PermissionDecision::Ask { .. }));
}

// ── Step 2-3: Allow rules ──

#[test]
fn test_content_specific_allow_rule() {
    let mut ctx = empty_context(PermissionMode::Default);
    ctx.allow_rules.insert(
        PermissionRuleSource::Session,
        vec![make_rule(
            "Bash",
            Some("git *"),
            PermissionBehavior::Allow,
            PermissionRuleSource::Session,
        )],
    );
    let result = PermissionEvaluator::evaluate(
        &ToolId::Builtin(ToolName::Bash),
        &bash_input("git status"),
        &ctx,
    );
    assert!(matches!(result, PermissionDecision::Allow { .. }));

    let result = PermissionEvaluator::evaluate(
        &ToolId::Builtin(ToolName::Bash),
        &bash_input("rm -rf /"),
        &ctx,
    );
    assert!(matches!(result, PermissionDecision::Ask { .. }));
}

// ── Step 4: Ask rules (NEW) ──

#[test]
fn test_tool_wide_ask_rule() {
    let mut ctx = empty_context(PermissionMode::BypassPermissions);
    ctx.ask_rules.insert(
        PermissionRuleSource::ProjectSettings,
        vec![make_rule(
            "Bash",
            None,
            PermissionBehavior::Ask,
            PermissionRuleSource::ProjectSettings,
        )],
    );
    // Even in bypass mode, tool-wide ask rules force a prompt
    let result =
        PermissionEvaluator::evaluate(&ToolId::Builtin(ToolName::Bash), &bash_input("ls"), &ctx);
    assert!(matches!(result, PermissionDecision::Ask { .. }));
}

#[test]
fn test_ask_rule_does_not_affect_other_tools() {
    let mut ctx = empty_context(PermissionMode::BypassPermissions);
    ctx.ask_rules.insert(
        PermissionRuleSource::ProjectSettings,
        vec![make_rule(
            "Bash",
            None,
            PermissionBehavior::Ask,
            PermissionRuleSource::ProjectSettings,
        )],
    );
    // Read is not Bash → bypass mode allows it
    let result = PermissionEvaluator::evaluate(
        &ToolId::Builtin(ToolName::Read),
        &serde_json::json!({}),
        &ctx,
    );
    assert!(matches!(result, PermissionDecision::Allow { .. }));
}

// ── Step 5: Content-specific ask ──

#[test]
fn test_content_specific_ask_rule() {
    let mut ctx = empty_context(PermissionMode::BypassPermissions);
    ctx.ask_rules.insert(
        PermissionRuleSource::ProjectSettings,
        vec![
            make_rule(
                "Bash",
                None,
                PermissionBehavior::Ask,
                PermissionRuleSource::ProjectSettings,
            ),
            make_rule(
                "Bash",
                Some("rm *"),
                PermissionBehavior::Ask,
                PermissionRuleSource::ProjectSettings,
            ),
        ],
    );
    // "rm -rf" matches content-specific ask → ask
    let result = PermissionEvaluator::evaluate(
        &ToolId::Builtin(ToolName::Bash),
        &bash_input("rm -rf /"),
        &ctx,
    );
    assert!(matches!(result, PermissionDecision::Ask { .. }));
}

// ── Step 6: Path safety ──

#[test]
fn test_write_to_dangerous_path_asks() {
    let ctx = empty_context(PermissionMode::BypassPermissions);
    let result = PermissionEvaluator::evaluate(
        &ToolId::Builtin(ToolName::Write),
        &file_input("/home/user/.bashrc"),
        &ctx,
    );
    assert!(matches!(result, PermissionDecision::Ask { .. }));
}

#[test]
fn test_write_to_safe_path_allowed_in_bypass() {
    let ctx = empty_context(PermissionMode::BypassPermissions);
    let result = PermissionEvaluator::evaluate(
        &ToolId::Builtin(ToolName::Write),
        &file_input("src/main.rs"),
        &ctx,
    );
    assert!(matches!(result, PermissionDecision::Allow { .. }));
}

// ── Step 7: MCP rules ──

#[test]
fn test_mcp_tool_allow_by_server_wildcard() {
    let mut ctx = empty_context(PermissionMode::Default);
    ctx.allow_rules.insert(
        PermissionRuleSource::Session,
        vec![make_rule(
            "mcp__slack__*",
            None,
            PermissionBehavior::Allow,
            PermissionRuleSource::Session,
        )],
    );
    let result = PermissionEvaluator::evaluate(
        &ToolId::Mcp {
            server: "slack".into(),
            tool: "send".into(),
        },
        &serde_json::json!({}),
        &ctx,
    );
    assert!(matches!(result, PermissionDecision::Allow { .. }));
}

#[test]
fn test_mcp_server_level_rule() {
    let mut ctx = empty_context(PermissionMode::Default);
    ctx.allow_rules.insert(
        PermissionRuleSource::Session,
        vec![make_rule(
            "mcp__slack",
            None,
            PermissionBehavior::Allow,
            PermissionRuleSource::Session,
        )],
    );
    // "mcp__slack" should match "mcp__slack__send"
    let result = PermissionEvaluator::evaluate(
        &ToolId::Mcp {
            server: "slack".into(),
            tool: "send".into(),
        },
        &serde_json::json!({}),
        &ctx,
    );
    assert!(matches!(result, PermissionDecision::Allow { .. }));
}

// ── Step 8: Mode fallthrough ──

#[test]
fn test_bypass_mode_allows_all() {
    let ctx = empty_context(PermissionMode::BypassPermissions);
    let result = PermissionEvaluator::evaluate(
        &ToolId::Builtin(ToolName::Bash),
        &bash_input("rm -rf /"),
        &ctx,
    );
    assert!(matches!(result, PermissionDecision::Allow { .. }));
}

/// TS: plan mode auto-allows if bypass was available; otherwise asks.
/// Read-only tools always allowed in plan mode.
#[test]
fn test_plan_mode_asks_non_readonly() {
    let ctx = empty_context(PermissionMode::Plan);
    // Bash is not read-only → ask (not deny!)
    let result =
        PermissionEvaluator::evaluate(&ToolId::Builtin(ToolName::Bash), &bash_input("ls"), &ctx);
    assert!(matches!(result, PermissionDecision::Ask { .. }));
}

#[test]
fn test_plan_mode_allows_readonly() {
    let ctx = empty_context(PermissionMode::Plan);
    // Read is read-only → allow even in plan mode
    let result = PermissionEvaluator::evaluate(
        &ToolId::Builtin(ToolName::Read),
        &serde_json::json!({}),
        &ctx,
    );
    assert!(matches!(result, PermissionDecision::Allow { .. }));

    // TaskCreate is safe (metadata only) → allow
    let result = PermissionEvaluator::evaluate(
        &ToolId::Builtin(ToolName::TaskCreate),
        &serde_json::json!({}),
        &ctx,
    );
    assert!(matches!(result, PermissionDecision::Allow { .. }));
}

#[test]
fn test_plan_mode_with_bypass_available_allows() {
    let mut ctx = empty_context(PermissionMode::Plan);
    ctx.bypass_available = true;
    // With bypass available, plan mode auto-allows
    let result =
        PermissionEvaluator::evaluate(&ToolId::Builtin(ToolName::Bash), &bash_input("ls"), &ctx);
    assert!(matches!(result, PermissionDecision::Allow { .. }));
}

#[test]
fn test_default_mode_asks() {
    let ctx = empty_context(PermissionMode::Default);
    let result =
        PermissionEvaluator::evaluate(&ToolId::Builtin(ToolName::Bash), &bash_input("ls"), &ctx);
    assert!(matches!(result, PermissionDecision::Ask { .. }));
}

/// TS: dontAsk converts all 'ask' decisions to 'deny'.
#[test]
fn test_dont_ask_mode_denies_all() {
    let ctx = empty_context(PermissionMode::DontAsk);
    // Bash with no allow rule → fallthrough → deny (not allow!)
    let result =
        PermissionEvaluator::evaluate(&ToolId::Builtin(ToolName::Bash), &bash_input("ls"), &ctx);
    assert!(matches!(result, PermissionDecision::Deny { .. }));

    // Write also denied (not in read-only list)
    let result = PermissionEvaluator::evaluate(
        &ToolId::Builtin(ToolName::Write),
        &file_input("src/main.rs"),
        &ctx,
    );
    assert!(matches!(result, PermissionDecision::Deny { .. }));

    // WebFetch also denied (not in safe list — network side effects)
    let result = PermissionEvaluator::evaluate(
        &ToolId::Builtin(ToolName::WebFetch),
        &serde_json::json!({}),
        &ctx,
    );
    assert!(matches!(result, PermissionDecision::Deny { .. }));
}

/// TS: dontAsk still honors explicit allow rules.
#[test]
fn test_dont_ask_mode_allows_explicit_rules() {
    let mut ctx = empty_context(PermissionMode::DontAsk);
    ctx.allow_rules.insert(
        PermissionRuleSource::Session,
        vec![make_rule(
            "Bash",
            None,
            PermissionBehavior::Allow,
            PermissionRuleSource::Session,
        )],
    );
    // Explicit allow rule → allow even in dontAsk
    let result =
        PermissionEvaluator::evaluate(&ToolId::Builtin(ToolName::Bash), &bash_input("ls"), &ctx);
    assert!(matches!(result, PermissionDecision::Allow { .. }));
}

/// TS: WebFetch/WebSearch are NOT in the safe allowlist (network effects).
#[test]
fn test_web_tools_not_in_safe_list() {
    let ctx = empty_context(PermissionMode::AcceptEdits);
    // WebFetch has network side effects → ask
    let result = PermissionEvaluator::evaluate(
        &ToolId::Builtin(ToolName::WebFetch),
        &serde_json::json!({}),
        &ctx,
    );
    assert!(matches!(result, PermissionDecision::Ask { .. }));
}

/// TS: Task management tools are in the safe allowlist.
#[test]
fn test_task_tools_are_safe() {
    let ctx = empty_context(PermissionMode::AcceptEdits);
    for tool in [
        ToolName::TaskCreate,
        ToolName::TaskGet,
        ToolName::TaskList,
        ToolName::TodoWrite,
    ] {
        let result =
            PermissionEvaluator::evaluate(&ToolId::Builtin(tool), &serde_json::json!({}), &ctx);
        assert!(
            matches!(result, PermissionDecision::Allow { .. }),
            "{tool:?} should be auto-allowed"
        );
    }
}

/// TS: acceptEdits allows read-only tools AND file-modifying tools.
#[test]
fn test_accept_edits_allows_read_only_and_file_edits() {
    let ctx = empty_context(PermissionMode::AcceptEdits);
    // Read-only tools → allow
    let result = PermissionEvaluator::evaluate(
        &ToolId::Builtin(ToolName::Read),
        &serde_json::json!({}),
        &ctx,
    );
    assert!(matches!(result, PermissionDecision::Allow { .. }));

    // File-modifying tools → allow (dangerous paths caught by step 6)
    let result = PermissionEvaluator::evaluate(
        &ToolId::Builtin(ToolName::Write),
        &file_input("src/main.rs"),
        &ctx,
    );
    assert!(matches!(result, PermissionDecision::Allow { .. }));

    let result = PermissionEvaluator::evaluate(
        &ToolId::Builtin(ToolName::Edit),
        &file_input("src/lib.rs"),
        &ctx,
    );
    assert!(matches!(result, PermissionDecision::Allow { .. }));

    // Non-file, non-read-only tools → ask
    let result =
        PermissionEvaluator::evaluate(&ToolId::Builtin(ToolName::Bash), &bash_input("ls"), &ctx);
    assert!(matches!(result, PermissionDecision::Ask { .. }));
}

/// TS: acceptEdits still catches dangerous paths via step 6.
#[test]
fn test_accept_edits_catches_dangerous_paths() {
    let ctx = empty_context(PermissionMode::AcceptEdits);
    // Dangerous path → step 6 catches → ask (even in acceptEdits)
    let result = PermissionEvaluator::evaluate(
        &ToolId::Builtin(ToolName::Write),
        &file_input("/home/user/.bashrc"),
        &ctx,
    );
    assert!(matches!(result, PermissionDecision::Ask { .. }));
}

// ── Rule helpers ──

#[test]
fn test_get_tool_wide_rule() {
    let mut ctx = empty_context(PermissionMode::Default);
    ctx.ask_rules.insert(
        PermissionRuleSource::UserSettings,
        vec![
            make_rule(
                "Bash",
                None,
                PermissionBehavior::Ask,
                PermissionRuleSource::UserSettings,
            ),
            make_rule(
                "Bash",
                Some("rm *"),
                PermissionBehavior::Ask,
                PermissionRuleSource::UserSettings,
            ),
        ],
    );

    // Tool-wide rule exists
    let rule = get_tool_wide_rule(&ctx, "Bash", PermissionBehavior::Ask);
    assert!(rule.is_some());
    assert!(rule.unwrap().value.rule_content.is_none());

    // No tool-wide deny rule
    assert!(get_tool_wide_rule(&ctx, "Bash", PermissionBehavior::Deny).is_none());
}

#[test]
fn test_get_content_rules_for_tool() {
    let mut ctx = empty_context(PermissionMode::Default);
    ctx.allow_rules.insert(
        PermissionRuleSource::Session,
        vec![
            make_rule(
                "Bash",
                None,
                PermissionBehavior::Allow,
                PermissionRuleSource::Session,
            ),
            make_rule(
                "Bash",
                Some("git *"),
                PermissionBehavior::Allow,
                PermissionRuleSource::Session,
            ),
            make_rule(
                "Bash",
                Some("npm *"),
                PermissionBehavior::Allow,
                PermissionRuleSource::Session,
            ),
        ],
    );

    let content_rules = get_content_rules_for_tool(&ctx, "Bash", PermissionBehavior::Allow);
    assert_eq!(content_rules.len(), 2); // "git *" and "npm *", not the tool-wide one
}

#[test]
fn test_mcp_server_level_pattern_matching() {
    // "mcp__slack" should match "mcp__slack__send"
    assert!(matches_tool_pattern("mcp__slack", "mcp__slack__send"));
    // But not "mcp__github__send"
    assert!(!matches_tool_pattern("mcp__slack", "mcp__github__send"));
    // Exact match still works
    assert!(matches_tool_pattern("mcp__slack__send", "mcp__slack__send"));
}

// ── Plan-mode plan-file auto-allow (TS: checkEditableInternalPath) ──

#[test]
fn plan_mode_auto_allows_write_to_session_plan_file() {
    let mut ctx = empty_context(PermissionMode::Plan);
    ctx.session_plan_file = Some(std::path::PathBuf::from("/tmp/plans/test.md"));

    let decision = PermissionEvaluator::evaluate(
        &ToolId::Builtin(ToolName::Write),
        &file_input("/tmp/plans/test.md"),
        &ctx,
    );
    assert!(
        matches!(decision, PermissionDecision::Allow { .. }),
        "Write to plan file must auto-allow in Plan mode, got: {decision:?}"
    );
}

#[test]
fn plan_mode_auto_allows_edit_to_session_plan_file() {
    let mut ctx = empty_context(PermissionMode::Plan);
    ctx.session_plan_file = Some(std::path::PathBuf::from("/tmp/plans/test.md"));

    let decision = PermissionEvaluator::evaluate(
        &ToolId::Builtin(ToolName::Edit),
        &file_input("/tmp/plans/test.md"),
        &ctx,
    );
    assert!(matches!(decision, PermissionDecision::Allow { .. }));
}

#[test]
fn plan_mode_asks_for_write_to_other_files() {
    let mut ctx = empty_context(PermissionMode::Plan);
    ctx.session_plan_file = Some(std::path::PathBuf::from("/tmp/plans/test.md"));

    let decision = PermissionEvaluator::evaluate(
        &ToolId::Builtin(ToolName::Write),
        &file_input("/tmp/other/file.rs"),
        &ctx,
    );
    assert!(
        matches!(decision, PermissionDecision::Ask { .. }),
        "Writes outside the plan file must still ask in Plan mode"
    );
}

#[test]
fn plan_mode_no_auto_allow_without_resolved_plan_file() {
    // If the engine forgot to populate session_plan_file, fall back to
    // the safe default: ask. No plan file = no auto-allow.
    let ctx = empty_context(PermissionMode::Plan);
    let decision = PermissionEvaluator::evaluate(
        &ToolId::Builtin(ToolName::Write),
        &file_input("/tmp/plans/test.md"),
        &ctx,
    );
    assert!(matches!(decision, PermissionDecision::Ask { .. }));
}

#[test]
fn plan_mode_read_only_still_allowed() {
    let ctx = empty_context(PermissionMode::Plan);
    let decision = PermissionEvaluator::evaluate(
        &ToolId::Builtin(ToolName::Read),
        &file_input("/tmp/anything.rs"),
        &ctx,
    );
    assert!(matches!(decision, PermissionDecision::Allow { .. }));
}

#[test]
fn plan_mode_allows_subagent_plan_variant_from_main_ctx() {
    // TS `isSessionPlanFile` (filesystem.ts:245-255) prefix-matches on
    // `<plansDir>/<slug>`, so both the main-agent plan (`<slug>.md`) and
    // the sub-agent variants (`<slug>-agent-<id>.md`) auto-allow from the
    // same context.
    let mut ctx = empty_context(PermissionMode::Plan);
    ctx.session_plan_file = Some(std::path::PathBuf::from("/tmp/plans/lucid-otter.md"));
    let decision = PermissionEvaluator::evaluate(
        &ToolId::Builtin(ToolName::Write),
        &file_input("/tmp/plans/lucid-otter-agent-a1b2c3d4.md"),
        &ctx,
    );
    assert!(
        matches!(decision, PermissionDecision::Allow { .. }),
        "Write to sub-agent plan variant must auto-allow in Plan mode, got: {decision:?}"
    );
}

#[test]
fn plan_mode_rejects_neighbor_md_file_with_different_slug() {
    // Prefix match must not bleed into a neighboring .md file that
    // happens to share the plans dir but has a different slug.
    let mut ctx = empty_context(PermissionMode::Plan);
    ctx.session_plan_file = Some(std::path::PathBuf::from("/tmp/plans/lucid-otter.md"));
    let decision = PermissionEvaluator::evaluate(
        &ToolId::Builtin(ToolName::Write),
        &file_input("/tmp/plans/other-plan.md"),
        &ctx,
    );
    assert!(
        matches!(decision, PermissionDecision::Ask { .. }),
        "Writes to unrelated plans-dir files must still ask, got: {decision:?}"
    );
}

#[test]
fn plan_mode_rejects_non_md_file_with_matching_slug_prefix() {
    // Extension check prevents `<slug>.txt` or `<slug>.rs` from slipping
    // through. TS: `normalizedPath.endsWith('.md')`.
    let mut ctx = empty_context(PermissionMode::Plan);
    ctx.session_plan_file = Some(std::path::PathBuf::from("/tmp/plans/lucid-otter.md"));
    let decision = PermissionEvaluator::evaluate(
        &ToolId::Builtin(ToolName::Write),
        &file_input("/tmp/plans/lucid-otter.rs"),
        &ctx,
    );
    assert!(
        matches!(decision, PermissionDecision::Ask { .. }),
        "Non-.md files must not auto-allow, got: {decision:?}"
    );
}

#[test]
fn plan_mode_rejects_path_traversal_escape_from_plans_dir() {
    // SECURITY: a crafted target like `<prefix>/../../etc/passwd.md`
    // starts with the slug prefix as a raw string but must not auto-allow
    // — `..` segments escape the plans dir. TS parity: `normalize()` in
    // filesystem.ts:251 collapses the `..` before the prefix check.
    let mut ctx = empty_context(PermissionMode::Plan);
    ctx.session_plan_file = Some(std::path::PathBuf::from("/tmp/plans/lucid-otter.md"));
    let decision = PermissionEvaluator::evaluate(
        &ToolId::Builtin(ToolName::Write),
        &file_input("/tmp/plans/lucid-otter/../../etc/passwd.md"),
        &ctx,
    );
    assert!(
        matches!(decision, PermissionDecision::Ask { .. }),
        "Path-traversal escape must not auto-allow, got: {decision:?}"
    );
}

#[test]
fn plan_mode_allows_noop_dot_segments_within_plans_dir() {
    // `.` segments are benign; a legitimate plan write with a `./` in the
    // middle should still auto-allow after lexical normalization.
    let mut ctx = empty_context(PermissionMode::Plan);
    ctx.session_plan_file = Some(std::path::PathBuf::from("/tmp/plans/lucid-otter.md"));
    let decision = PermissionEvaluator::evaluate(
        &ToolId::Builtin(ToolName::Write),
        &file_input("/tmp/plans/./lucid-otter.md"),
        &ctx,
    );
    assert!(
        matches!(decision, PermissionDecision::Allow { .. }),
        "Benign `./` segment must still auto-allow after normalize, got: {decision:?}"
    );
}
