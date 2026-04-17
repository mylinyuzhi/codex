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

/// Result of a tool-level permission check callback.
///
/// TS: tool.checkPermissions() returns behavior + optional suggestions.
/// `Passthrough` means the tool has no opinion — continue pipeline.
#[derive(Debug, Clone)]
pub enum ToolCheckResult {
    /// Tool has no opinion — continue with rule-based checks.
    Passthrough,
    /// Tool explicitly allows this input.
    Allow,
    /// Tool requires user confirmation for this input.
    Ask { message: String },
    /// Tool denies this input.
    Deny { message: String },
}

/// Callback for tool-level permission checks.
///
/// TS: `tool.checkPermissions(parsedInput, context)` in permissions.ts step 1c.
/// Each tool (Bash, Write, PowerShell) implements content-specific checks
/// that the rule pipeline can't express (subcommand parsing, path safety, etc.).
///
/// The evaluate pipeline calls this after deny rules (step 1) and before
/// allow rules (step 2). The tool can return `Passthrough` to defer to rules.
pub type ToolCheckFn = dyn Fn(&ToolId, &Value, &ToolPermissionContext) -> ToolCheckResult;

/// Permission evaluator. Implements the multi-step evaluation pipeline.
pub struct PermissionEvaluator;

impl PermissionEvaluator {
    /// Evaluate permissions for a tool call.
    ///
    /// Pipeline (matches TS `hasPermissionsToUseToolInner`):
    /// 1. Deny rules — deny always wins, regardless of priority
    /// 1b. Tool-level permission check (via callback)
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
        let tool_str = tool_id.to_string();

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
                    }
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
                    return PermissionDecision::Deny {
                        message,
                        reason: PermissionDecisionReason::Mode { mode: context.mode },
                    };
                }
                ToolCheckResult::Allow => {
                    return PermissionDecision::Allow {
                        updated_input: None,
                        feedback: None,
                    };
                }
                ToolCheckResult::Ask { message } => {
                    return PermissionDecision::Ask {
                        message,
                        suggestions: vec![],
                    };
                }
                ToolCheckResult::Passthrough => {
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
                                    return PermissionDecision::Allow {
                                        updated_input: None,
                                        feedback: None,
                                    };
                                }
                                continue;
                            }
                        }
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
                            return PermissionDecision::Ask {
                                message: format!("ask rule matched: {tool_str}({content})"),
                                suggestions: vec![],
                            };
                        }
                    }
                }
            }

            return PermissionDecision::Ask {
                message: format!("tool-wide ask rule for {tool_str}"),
                suggestions: vec![],
            };
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
                return PermissionDecision::Ask {
                    message,
                    suggestions: vec![],
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
                        return PermissionDecision::Ask {
                            message: format!("MCP server {server} requires approval"),
                            suggestions: vec![],
                        };
                    }
                }
            }
        }

        // Step 8: Mode-based fallthrough
        mode_fallthrough(context.mode, &tool_str, context.bypass_available)
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
/// - `default`, `auto`, `bubble` → ask
fn mode_fallthrough(
    mode: PermissionMode,
    tool_str: &str,
    bypass_available: bool,
) -> PermissionDecision {
    match mode {
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
        // Read-only tools are always safe in plan mode.
        PermissionMode::Plan => {
            if bypass_available {
                PermissionDecision::Allow {
                    updated_input: None,
                    feedback: None,
                }
            } else if is_read_only_tool(tool_str) {
                PermissionDecision::Allow {
                    updated_input: None,
                    feedback: None,
                }
            } else {
                PermissionDecision::Ask {
                    message: format!("plan mode: approve {tool_str}?"),
                    suggestions: vec![],
                }
            }
        }
        // TS: acceptEdits auto-allows read-only tools AND file-modifying
        // tools (Write, Edit, NotebookEdit). Dangerous paths are already
        // caught by step 6 (path safety), so only safe edits reach here.
        PermissionMode::AcceptEdits => {
            if is_read_only_tool(tool_str) || is_file_modifying_tool(tool_str) {
                PermissionDecision::Allow {
                    updated_input: None,
                    feedback: None,
                }
            } else {
                PermissionDecision::Ask {
                    message: format!("approve {tool_str}?"),
                    suggestions: vec![],
                }
            }
        }
        // Default, Auto, Bubble → ask
        _ => PermissionDecision::Ask {
            message: format!("approve {tool_str}?"),
            suggestions: vec![],
        },
    }
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
fn is_file_modifying_tool(tool_name: &str) -> bool {
    tool_name == ToolName::Write.as_str()
        || tool_name == ToolName::Edit.as_str()
        || tool_name == ToolName::NotebookEdit.as_str()
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
