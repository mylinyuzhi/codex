//! Core permission evaluation pipeline.
//!
//! TS: utils/permissions/permissions.ts (~1486 LOC)
//!     hasPermissionsToUseToolInner() — 9-step evaluation
//!
//! The Rust pipeline is a faithful port of the TS logic, organized as
//! 7 sequential steps. Steps that require external integration (classifier,
//! hooks, sandbox) are deferred to the caller via the returned decision.

use coco_types::MCP_TOOL_PREFIX;
use coco_types::MCP_TOOL_SEPARATOR;
use coco_types::PermissionBehavior;
use coco_types::PermissionDecision;
use coco_types::PermissionDecisionReason;
use coco_types::PermissionMode;
use coco_types::PermissionRule;
use coco_types::PermissionRuleSource;
use coco_types::ToolId;
use coco_types::ToolName;
use coco_types::ToolPermissionContext;
use serde_json::Value;

use crate::filesystem;
use crate::shell_rules;

// `ToolCheckResult` lives in `coco-types::permission` so the
// `coco_tool_runtime::Tool::check_permissions` trait method can
// reference it without `coco-tool-runtime` depending on
// `coco-permissions`. Re-exported below for legacy import sites.
pub use coco_types::ToolCheckResult;

/// Callback for tool-level permission checks.
///
/// TS: `tool.checkPermissions(parsedInput, context)` in permissions.ts step 1c.
/// Each tool (Bash, Write, PowerShell) implements content-specific checks
/// that the rule pipeline can't express (subcommand parsing, path safety, etc.).
///
/// The evaluate pipeline calls this after deny rules (step 1) and before
/// allow rules (step 2). The tool can return `Passthrough` to defer to rules.
pub type ToolCheckFn = dyn Fn(&ToolId, &Value, &ToolPermissionContext) -> ToolCheckResult;

/// Per-call facts supplied by the tool runtime to the rule evaluator.
///
/// These are facts about the already-validated tool input, not policy.
/// Keeping them explicit avoids making `coco-permissions` depend on
/// shell/file parsers owned by tool crates.
#[derive(Debug, Clone, Copy, Default)]
pub struct PermissionEvaluationOptions {
    /// The concrete tool/input pair is read-only even though the tool name
    /// itself may not be statically read-only, e.g. `Bash(ls | head)`.
    pub dynamic_read_only: bool,
}

/// Permission evaluator. Implements the multi-step evaluation pipeline.
pub struct PermissionEvaluator;

impl PermissionEvaluator {
    /// Evaluate permissions for a tool call.
    ///
    /// Pipeline (matches TS `hasPermissionsToUseToolInner`):
    /// 1. Deny rules — deny always wins, regardless of priority
    ///    1b. Tool-level permission check (via callback)
    /// 2. Allow rules — priority-sorted by source
    /// 3. Content-specific allow — Bash/PowerShell exact/prefix/wildcard
    /// 4. Ask rules — tool-wide ask requires user confirmation
    /// 5. Content-specific ask — Bash(pattern) ask rules
    /// 6. Path safety checks — dangerous files, traversal
    /// 7. MCP server-level rules
    /// 8. Mode-based fallthrough
    pub fn evaluate(
        tool_id: &ToolId,
        input: &Value,
        context: &ToolPermissionContext,
    ) -> PermissionDecision {
        Self::evaluate_with_tool_check(tool_id, input, context, None)
    }

    /// Evaluate with an optional tool-level permission callback.
    ///
    /// TS: step 1c calls `tool.checkPermissions()` which Bash uses for
    /// subcommand analysis, Write uses for path checks, etc.
    pub fn evaluate_with_tool_check(
        tool_id: &ToolId,
        input: &Value,
        context: &ToolPermissionContext,
        tool_check: Option<&ToolCheckFn>,
    ) -> PermissionDecision {
        Self::evaluate_with_tool_check_and_options(
            tool_id,
            input,
            context,
            tool_check,
            PermissionEvaluationOptions::default(),
        )
    }

    /// Evaluate with an optional tool-level permission callback and
    /// per-call runtime facts.
    pub fn evaluate_with_tool_check_and_options(
        tool_id: &ToolId,
        input: &Value,
        context: &ToolPermissionContext,
        tool_check: Option<&ToolCheckFn>,
        options: PermissionEvaluationOptions,
    ) -> PermissionDecision {
        let decision = Self::evaluate_inner(tool_id, input, context, tool_check, options);
        // TS `hasPermissionsToUseTool`: dontAsk converts ANY remaining 'ask'
        // into 'deny' as the final step, so early-return asks (tool-wide ask,
        // content ask, path-safety ask, MCP server ask) are denied too — not
        // only the mode-fallthrough ask.
        if context.mode == PermissionMode::DontAsk
            && matches!(decision, PermissionDecision::Ask { .. })
        {
            return PermissionDecision::Deny {
                message: format!("{tool_id} denied: permission mode does not allow prompting"),
                reason: PermissionDecisionReason::Mode {
                    mode: PermissionMode::DontAsk,
                },
            };
        }
        decision
    }

    fn evaluate_inner(
        tool_id: &ToolId,
        input: &Value,
        context: &ToolPermissionContext,
        tool_check: Option<&ToolCheckFn>,
        options: PermissionEvaluationOptions,
    ) -> PermissionDecision {
        let tool_str = tool_id.to_string();

        tracing::trace!(
            tool_name = %tool_str,
            mode = ?context.mode,
            deny_rule_sources = context.deny_rules.len(),
            allow_rule_sources = context.allow_rules.len(),
            ask_rule_sources = context.ask_rules.len(),
            has_tool_check = tool_check.is_some(),
            dynamic_read_only = options.dynamic_read_only,
            "permission_eval: enter",
        );

        // Step 1: Deny rules (deny always wins)
        for rules in context.deny_rules.values() {
            for rule in rules {
                if matches_tool_pattern(&rule.value.tool_pattern, &tool_str) {
                    // Content-specific deny: only deny if command matches
                    if is_shell_tool(&rule.value.tool_pattern) && rule.value.rule_content.is_some()
                    {
                        let content = rule.value.rule_content.as_deref().unwrap_or("");
                        if let Some(command) = extract_shell_command(input)
                            && !shell_rules::matches_bash_rule(content, &command)
                        {
                            continue;
                        }
                    } else if is_file_rule_for_tool(&rule.value.tool_pattern, &tool_str)
                        && rule.value.rule_content.is_some()
                        && !file_rule_matches_input(rule, &tool_str, input, context)
                    {
                        continue;
                    }
                    tracing::debug!(
                        tool_name = %tool_str,
                        permission_decision = "deny",
                        rule_pattern = %rule.value.tool_pattern,
                        rule_content = ?rule.value.rule_content,
                        "permission_eval: deny rule matched",
                    );
                    return PermissionDecision::Deny {
                        message: format!("denied by rule: {}", rule.value.tool_pattern),
                        reason: PermissionDecisionReason::Rule { rule: rule.clone() },
                    };
                }
            }
        }

        // Step 1b: Tool-level permission check (TS step 1c)
        // Bash checks subcommands, PowerShell checks cmdlets, Write checks paths.
        if let Some(check_fn) = tool_check {
            match check_fn(tool_id, input, context) {
                ToolCheckResult::Deny { message } => {
                    tracing::debug!(
                        tool_name = %tool_str,
                        permission_decision = "deny",
                        "permission_eval: tool.check_permissions returned deny",
                    );
                    return PermissionDecision::Deny {
                        message,
                        reason: PermissionDecisionReason::Mode { mode: context.mode },
                    };
                }
                ToolCheckResult::Allow {
                    updated_input,
                    feedback,
                } => {
                    tracing::debug!(
                        tool_name = %tool_str,
                        permission_decision = "allow",
                        rewritten_input = updated_input.is_some(),
                        "permission_eval: tool.check_permissions returned allow",
                    );
                    return PermissionDecision::Allow {
                        updated_input,
                        feedback,
                    };
                }
                ToolCheckResult::Ask {
                    message,
                    suggestions,
                    choices,
                } => {
                    tracing::debug!(
                        tool_name = %tool_str,
                        permission_decision = "ask",
                        suggestion_count = suggestions.len(),
                        "permission_eval: tool.check_permissions returned ask",
                    );
                    return PermissionDecision::Ask {
                        message,
                        suggestions,
                        choices,
                    };
                }
                ToolCheckResult::Passthrough => {
                    tracing::trace!(
                        tool_name = %tool_str,
                        "permission_eval: tool.check_permissions passthrough",
                    );
                    // Tool has no opinion — continue pipeline
                }
            }
        }

        // Step 2: Allow rules (sorted by source priority)
        for source in RULE_PRIORITY_ORDER {
            if let Some(rules) = context.allow_rules.get(source) {
                for rule in rules {
                    if matches_tool_pattern(&rule.value.tool_pattern, &tool_str) {
                        // Step 3: Content-specific allow
                        if is_shell_tool(&rule.value.tool_pattern)
                            && rule.value.rule_content.is_some()
                        {
                            let content = rule.value.rule_content.as_deref().unwrap_or("");
                            if let Some(command) = extract_shell_command(input) {
                                if shell_rules::matches_bash_rule(content, &command) {
                                    tracing::debug!(
                                        tool_name = %tool_str,
                                        permission_decision = "allow",
                                        rule_source = ?source,
                                        rule_pattern = %rule.value.tool_pattern,
                                        rule_content = %content,
                                        "permission_eval: shell allow rule matched",
                                    );
                                    return PermissionDecision::Allow {
                                        updated_input: None,
                                        feedback: None,
                                    };
                                }
                                continue;
                            }
                        } else if is_file_rule_for_tool(&rule.value.tool_pattern, &tool_str)
                            && rule.value.rule_content.is_some()
                            && !file_rule_matches_input(rule, &tool_str, input, context)
                        {
                            continue;
                        }
                        tracing::debug!(
                            tool_name = %tool_str,
                            permission_decision = "allow",
                            rule_source = ?source,
                            rule_pattern = %rule.value.tool_pattern,
                            rule_content = ?rule.value.rule_content,
                            "permission_eval: tool-wide allow rule matched",
                        );
                        return PermissionDecision::Allow {
                            updated_input: None,
                            feedback: None,
                        };
                    }
                }
            }
        }

        // Step 4: Ask rules — tool-wide ask
        if let Some(ask_rule) = get_tool_wide_rule(context, &tool_str, PermissionBehavior::Ask) {
            // If this is a shell tool, check if there are content-specific rules first
            if is_shell_tool(&ask_rule.value.tool_pattern)
                && let Some(command) = extract_shell_command(input)
            {
                // Step 5: Content-specific ask rules
                for rules in context.ask_rules.values() {
                    for rule in rules {
                        if matches_tool_pattern(&rule.value.tool_pattern, &tool_str)
                            && let Some(content) = &rule.value.rule_content
                            && shell_rules::matches_bash_rule(content, &command)
                        {
                            tracing::debug!(
                                tool_name = %tool_str,
                                permission_decision = "ask",
                                rule_pattern = %rule.value.tool_pattern,
                                rule_content = %content,
                                "permission_eval: shell ask rule matched",
                            );
                            return PermissionDecision::Ask {
                                message: format!("ask rule matched: {tool_str}({content})"),
                                suggestions: vec![],
                                choices: None,
                            };
                        }
                    }
                }
            }

            tracing::debug!(
                tool_name = %tool_str,
                permission_decision = "ask",
                rule_pattern = %ask_rule.value.tool_pattern,
                "permission_eval: tool-wide ask rule matched",
            );
            return PermissionDecision::Ask {
                message: format!("tool-wide ask rule for {tool_str}"),
                suggestions: vec![],
                choices: None,
            };
        }

        for rules in context.ask_rules.values() {
            for rule in rules {
                if is_file_rule_for_tool(&rule.value.tool_pattern, &tool_str)
                    && rule.value.rule_content.is_some()
                    && file_rule_matches_input(rule, &tool_str, input, context)
                {
                    tracing::debug!(
                        tool_name = %tool_str,
                        permission_decision = "ask",
                        rule_pattern = %rule.value.tool_pattern,
                        rule_content = ?rule.value.rule_content,
                        "permission_eval: file ask rule matched",
                    );
                    return PermissionDecision::Ask {
                        message: format!(
                            "ask rule matched: {tool_str}({})",
                            rule.value.rule_content.as_deref().unwrap_or("")
                        ),
                        suggestions: vec![],
                        choices: None,
                    };
                }
            }
        }

        // Step 6: Path safety checks for file-modifying tools
        if is_file_modifying_tool(&tool_str)
            && let Some(path) = extract_file_path(input)
        {
            let safety = filesystem::check_path_safety_for_auto_edit(&path);
            if let filesystem::PathSafetyResult::Blocked {
                message,
                classifier_approvable: _,
            } = safety
            {
                tracing::debug!(
                    tool_name = %tool_str,
                    permission_decision = "ask",
                    path = %path,
                    "permission_eval: path safety check blocked",
                );
                return PermissionDecision::Ask {
                    message,
                    suggestions: vec![],
                    choices: None,
                };
            }
        }

        // Step 7: MCP server-level rules
        if let ToolId::Mcp { server, .. } = tool_id {
            // Check server-level allow: "mcp__server" matches "mcp__server__tool"
            let server_pattern = format!("{MCP_TOOL_PREFIX}{server}");
            for rules in context.allow_rules.values() {
                for rule in rules {
                    if rule.value.tool_pattern == server_pattern
                        || matches_tool_pattern(&rule.value.tool_pattern, &tool_str)
                    {
                        tracing::debug!(
                            tool_name = %tool_str,
                            permission_decision = "allow",
                            mcp_server = %server,
                            rule_pattern = %rule.value.tool_pattern,
                            "permission_eval: MCP server-level allow rule matched",
                        );
                        return PermissionDecision::Allow {
                            updated_input: None,
                            feedback: None,
                        };
                    }
                }
            }

            // Check server-level ask
            for rules in context.ask_rules.values() {
                for rule in rules {
                    if rule.value.tool_pattern == server_pattern {
                        tracing::debug!(
                            tool_name = %tool_str,
                            permission_decision = "ask",
                            mcp_server = %server,
                            "permission_eval: MCP server-level ask rule matched",
                        );
                        return PermissionDecision::Ask {
                            message: format!("MCP server {server} requires approval"),
                            suggestions: vec![],
                            choices: None,
                        };
                    }
                }
            }
        }

        // Step 8: Mode-based fallthrough
        let decision = mode_fallthrough(context, &tool_str, input, options);
        tracing::debug!(
            tool_name = %tool_str,
            permission_decision = decision_label(&decision),
            mode = ?context.mode,
            dynamic_read_only = options.dynamic_read_only,
            "permission_eval: fell through to mode-based decision",
        );
        decision
    }
}

/// Short tag for a `PermissionDecision` suitable for the
/// `permission_decision` tracing field.
fn decision_label(decision: &PermissionDecision) -> &'static str {
    match decision {
        PermissionDecision::Allow { .. } => "allow",
        PermissionDecision::Deny { .. } => "deny",
        PermissionDecision::Ask { .. } => "ask",
    }
}

// ── Rule helpers ──
// TS: getAllowRules, getDenyRules, getAskRules, getDenyRuleForTool, etc.

/// Collect all rules of a given behavior from all sources (flattened).
pub fn get_all_rules(
    context: &ToolPermissionContext,
    behavior: PermissionBehavior,
) -> Vec<&PermissionRule> {
    let map = match behavior {
        PermissionBehavior::Allow => &context.allow_rules,
        PermissionBehavior::Deny => &context.deny_rules,
        PermissionBehavior::Ask => &context.ask_rules,
    };
    map.values().flatten().collect()
}

/// Find a tool-wide rule (no content constraint) for a specific tool.
///
/// TS: getAskRuleForTool(), toolAlwaysAllowedRule(), getDenyRuleForTool()
pub fn get_tool_wide_rule(
    context: &ToolPermissionContext,
    tool_str: &str,
    behavior: PermissionBehavior,
) -> Option<PermissionRule> {
    let map = match behavior {
        PermissionBehavior::Allow => &context.allow_rules,
        PermissionBehavior::Deny => &context.deny_rules,
        PermissionBehavior::Ask => &context.ask_rules,
    };

    for rules in map.values() {
        for rule in rules {
            if matches_tool_pattern(&rule.value.tool_pattern, tool_str)
                && rule.value.rule_content.is_none()
            {
                return Some(rule.clone());
            }
        }
    }
    None
}

/// Get all content-specific rules for a tool (e.g. all Bash(pattern) rules).
///
/// TS: getRuleByContentsForTool()
pub fn get_content_rules_for_tool<'a>(
    context: &'a ToolPermissionContext,
    tool_str: &str,
    behavior: PermissionBehavior,
) -> Vec<&'a PermissionRule> {
    let map = match behavior {
        PermissionBehavior::Allow => &context.allow_rules,
        PermissionBehavior::Deny => &context.deny_rules,
        PermissionBehavior::Ask => &context.ask_rules,
    };

    map.values()
        .flatten()
        .filter(|r| {
            matches_tool_pattern(&r.value.tool_pattern, tool_str) && r.value.rule_content.is_some()
        })
        .collect()
}

// ── Priority ──

/// Rule source priority order (most specific first).
const RULE_PRIORITY_ORDER: &[PermissionRuleSource] = &[
    PermissionRuleSource::Session,
    PermissionRuleSource::Command,
    PermissionRuleSource::CliArg,
    PermissionRuleSource::FlagSettings,
    PermissionRuleSource::LocalSettings,
    PermissionRuleSource::ProjectSettings,
    PermissionRuleSource::UserSettings,
    PermissionRuleSource::PolicySettings,
];

// ── Mode fallthrough ──

/// Mode-based fallthrough when no rules matched.
///
/// TS: `hasPermissionsToUseToolInner` steps 2a + wrapper mode transformations.
///
/// - `bypassPermissions` → auto-allow everything
/// - `dontAsk` → deny (TS converts ask→deny in wrapper, line 508)
/// - `plan` with bypass_available → auto-allow (TS line 1268-1271)
/// - `plan` without bypass_available → ask (TS falls through to prompt/classifier)
/// - `acceptEdits` → auto-allow read-only + file edits; ask for rest
/// - `default`, `auto` → auto-allow read-only; ask for rest
/// - `bubble` → ask and delegate to the parent context
fn mode_fallthrough(
    context: &ToolPermissionContext,
    tool_str: &str,
    input: &Value,
    options: PermissionEvaluationOptions,
) -> PermissionDecision {
    match context.mode {
        PermissionMode::BypassPermissions => PermissionDecision::Allow {
            updated_input: None,
            feedback: None,
        },
        // TS: dontAsk converts every remaining 'ask' into 'deny'.
        // "Don't ask me — just deny anything that would prompt."
        PermissionMode::DontAsk => PermissionDecision::Deny {
            message: format!("{tool_str} denied: permission mode does not allow prompting"),
            reason: PermissionDecisionReason::Mode {
                mode: PermissionMode::DontAsk,
            },
        },
        // TS: plan mode auto-allows if user originally had bypass mode;
        // otherwise falls through to ask (for classifier or user prompt).
        // Read-only tools are always safe in plan mode. Writes/edits to
        // the session's own plan file also auto-allow — otherwise the
        // model can't build up the plan incrementally without a prompt
        // on every edit. TS parity: `checkEditableInternalPath` /
        // `isSessionPlanFile` in utils/permissions/filesystem.ts.
        PermissionMode::Plan => {
            if context.bypass_available
                || is_read_only_tool(tool_str)
                || options.dynamic_read_only
                || is_session_plan_file_write(tool_str, input, context)
            {
                PermissionDecision::Allow {
                    updated_input: None,
                    feedback: None,
                }
            } else {
                PermissionDecision::Ask {
                    message: format!("plan mode: approve {tool_str}?"),
                    suggestions: vec![],
                    choices: None,
                }
            }
        }
        // TS: acceptEdits auto-allows read-only tools AND file-modifying
        // tools (Write, Edit, NotebookEdit). Dangerous paths are already
        // caught by step 6 (path safety), so only safe edits reach here.
        PermissionMode::AcceptEdits => {
            if is_read_only_tool(tool_str)
                || options.dynamic_read_only
                || is_file_modifying_tool(tool_str)
            {
                PermissionDecision::Allow {
                    updated_input: None,
                    feedback: None,
                }
            } else {
                PermissionDecision::Ask {
                    message: format!("approve {tool_str}?"),
                    suggestions: vec![],
                    choices: None,
                }
            }
        }
        // TS parity: safe read-only tools never need an approval prompt in
        // ordinary interactive modes. Keep Bubble out of this fast path so
        // the parent permission context remains authoritative.
        PermissionMode::Default | PermissionMode::Auto
            if is_read_only_tool(tool_str) || options.dynamic_read_only =>
        {
            PermissionDecision::Allow {
                updated_input: None,
                feedback: None,
            }
        }
        PermissionMode::Default | PermissionMode::Auto | PermissionMode::Bubble => {
            PermissionDecision::Ask {
                message: format!("approve {tool_str}?"),
                suggestions: vec![],
                choices: None,
            }
        }
    }
}

/// Auto-allow writes/edits targeted at the session's own plan file.
///
/// TS: `checkEditableInternalPath` in `utils/permissions/filesystem.ts:1479`
/// + `isSessionPlanFile` at `filesystem.ts:245-255` — the plan file is
///   carved out so the model can build up its plan in plan mode without
///   per-write prompts.
///
/// TS matches with `normalize(path).startsWith("<plansDir>/<slug>") &&
/// endsWith(".md")`. The `normalize()` step is security-load-bearing: it
/// collapses `..` and `.` segments so a crafted target like
/// `<plansDir>/<slug>/../../etc/passwd.md` resolves to `/etc/passwd.md`
/// and fails the prefix check. Without it, the raw string starts with
/// the slug prefix and the auto-allow would let the model write anywhere.
///
/// Returns `true` only when:
/// - the tool is a file-modifying tool (Write / Edit / NotebookEdit), and
/// - the context has a resolved `session_plan_file`, and
/// - the lexically-normalized target starts with the plans-dir + slug
///   prefix, and
/// - the target ends with `.md`.
fn is_session_plan_file_write(
    tool_str: &str,
    input: &Value,
    context: &ToolPermissionContext,
) -> bool {
    let Some(plan_file) = context.session_plan_file.as_ref() else {
        return false;
    };
    if !is_file_modifying_tool(tool_str) {
        return false;
    }
    let Some(target) = extract_file_modifying_path(tool_str, input) else {
        return false;
    };
    // Recover the `<plansDir>/<slug>` prefix by dropping the `.md` suffix
    // off the stashed plan file. String-based on purpose: `Path::starts_with`
    // is component-aware so `/a/foo.md` would NOT `starts_with(/a/foo)`, but
    // TS uses raw `string.startsWith` which does. Mirror TS to keep both
    // `<slug>.md` and `<slug>-agent-*.md` allowed from the same context.
    let plan_file_str = plan_file.to_string_lossy();
    let Some(prefix) = plan_file_str.strip_suffix(".md") else {
        return false;
    };
    // Lexical normalization: collapse `.` and `..` segments before the
    // prefix check. TS parity: `normalize(absolutePath)` in
    // filesystem.ts:251. Without this the prefix check trivially accepts
    // `<prefix>/../../etc/passwd.md`.
    let normalized = lexical_normalize(std::path::Path::new(&target));
    let normalized_str = normalized.to_string_lossy();
    let has_md_ext = normalized
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("md"));
    has_md_ext && normalized_str.starts_with(prefix)
}

/// Purely lexical path normalization — collapses `.` and `..` segments
/// without touching the filesystem. Mirrors Node's `path.normalize`.
/// Used to prevent path-traversal bypasses in plan-file auto-allow.
fn lexical_normalize(path: &std::path::Path) -> std::path::PathBuf {
    use std::path::Component;
    let mut out = std::path::PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                out.push(c.as_os_str());
            }
        }
    }
    if out.as_os_str().is_empty() {
        std::path::PathBuf::from(".")
    } else {
        out
    }
}

/// Extract the file-path argument from a file-modifying tool's input.
///
/// Write + Edit use `file_path`; NotebookEdit uses `notebook_path`.
pub(crate) fn extract_file_modifying_path(tool_str: &str, input: &Value) -> Option<String> {
    let key = if tool_str == ToolName::NotebookEdit.as_str() {
        "notebook_path"
    } else {
        "file_path"
    };
    input.get(key).and_then(|v| v.as_str()).map(String::from)
}

// ── Predicates ──

/// Tools that are always safe to auto-allow (no side effects).
///
/// TS: `SAFE_YOLO_ALLOWLISTED_TOOLS` in classifierDecision.ts
/// These tools skip the classifier in auto mode and are auto-allowed
/// in acceptEdits/plan modes.
fn is_read_only_tool(tool_name: &str) -> bool {
    const READ_ONLY_TOOLS: &[&str] = &[
        // File I/O (read-only)
        ToolName::Read.as_str(),
        ToolName::Glob.as_str(),
        ToolName::Grep.as_str(),
        ToolName::Lsp.as_str(),
        ToolName::ToolSearch.as_str(),
        // MCP read-only
        ToolName::ListMcpResources.as_str(),
        ToolName::ReadMcpResource.as_str(),
        // Task management (metadata only)
        ToolName::TodoWrite.as_str(),
        ToolName::TaskCreate.as_str(),
        ToolName::TaskGet.as_str(),
        ToolName::TaskUpdate.as_str(),
        ToolName::TaskList.as_str(),
        ToolName::TaskStop.as_str(),
        ToolName::TaskOutput.as_str(),
        // Plan mode / UI (control flow, no execution)
        ToolName::AskUserQuestion.as_str(),
        ToolName::EnterPlanMode.as_str(),
        ToolName::ExitPlanMode.as_str(),
        ToolName::VerifyPlanExecution.as_str(),
        // Swarm coordination (internal state, no external effects)
        ToolName::TeamCreate.as_str(),
        ToolName::TeamDelete.as_str(),
        ToolName::SendMessage.as_str(),
        // Scheduling read-only
        ToolName::CronList.as_str(),
        // Misc safe
        ToolName::Sleep.as_str(),
        ToolName::Brief.as_str(),
    ];
    READ_ONLY_TOOLS.contains(&tool_name)
}

/// Whether a tool pattern targets a shell tool (Bash/PowerShell).
fn is_shell_tool(tool_pattern: &str) -> bool {
    tool_pattern == ToolName::Bash.as_str() || tool_pattern == ToolName::PowerShell.as_str()
}

/// Whether a tool modifies files (Write, Edit, NotebookEdit).
pub(crate) fn is_file_modifying_tool(tool_name: &str) -> bool {
    tool_name == ToolName::Write.as_str()
        || tool_name == ToolName::Edit.as_str()
        || tool_name == ToolName::NotebookEdit.as_str()
        || tool_name == ToolName::ApplyPatch.as_str()
}

fn is_file_rule_for_tool(rule_tool_pattern: &str, tool_name: &str) -> bool {
    is_file_modifying_tool(tool_name)
        && (rule_tool_pattern == ToolName::Edit.as_str() || rule_tool_pattern == tool_name)
}

fn file_rule_matches_input(
    rule: &PermissionRule,
    tool_name: &str,
    input: &Value,
    context: &ToolPermissionContext,
) -> bool {
    let Some(path) = extract_file_modifying_path(tool_name, input) else {
        return false;
    };
    let cwd = std::env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "/".to_string());
    let paths_to_check = filesystem::get_paths_for_permission_check(&path, &cwd);
    let match_context = crate::file_rules::FileRuleMatchContext::new(&cwd)
        .with_source_roots(&context.permission_rule_source_roots);
    crate::file_rules::file_rule_matches_paths(
        rule,
        &paths_to_check,
        crate::file_rules::FileRuleToolType::Edit,
        &match_context,
    )
}

fn extract_shell_command(input: &Value) -> Option<String> {
    input
        .get("command")
        .and_then(|v| v.as_str())
        .map(String::from)
}

fn extract_file_path(input: &Value) -> Option<String> {
    input
        .get("file_path")
        .and_then(|v| v.as_str())
        .map(String::from)
}

/// Simple tool pattern matching (exact, prefix-wildcard, or MCP pattern).
fn matches_tool_pattern(pattern: &str, tool: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return tool.starts_with(prefix);
    }
    // Handle Bash(command) pattern: "Bash" matches tool "Bash"
    if let Some(paren) = pattern.find('(') {
        let base = &pattern[..paren];
        return tool == base;
    }
    // MCP server-level: "mcp__server" matches "mcp__server__tool"
    if let Some(pattern_rest) = pattern.strip_prefix(MCP_TOOL_PREFIX)
        && let Some(tool_rest) = tool.strip_prefix(MCP_TOOL_PREFIX)
    {
        // pattern_rest = "server" (no separator = server-level rule)
        // tool_rest = "server__tool"
        if !pattern_rest.contains(MCP_TOOL_SEPARATOR) {
            // Server-level pattern: extract server from tool
            let tool_server = tool_rest
                .split_once(MCP_TOOL_SEPARATOR)
                .map(|(s, _)| s)
                .unwrap_or(tool_rest);
            return pattern_rest == tool_server;
        }
    }
    pattern == tool
}

#[cfg(test)]
#[path = "evaluate.test.rs"]
mod tests;
