use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;

use crate::PermissionMode;

/// Permission behavior for a rule.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionBehavior {
    Allow,
    Deny,
    Ask,
}

/// Classifier behavior (same variants as `PermissionBehavior`).
///
/// TS: `ClassifierBehavior` in types/permissions.ts
pub type ClassifierBehavior = PermissionBehavior;

/// Token usage from the classifier.
///
/// TS: `ClassifierUsage` in types/permissions.ts
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClassifierUsage {
    #[serde(default)]
    pub input_tokens: i64,
    #[serde(default)]
    pub output_tokens: i64,
    #[serde(default)]
    pub cache_read_input_tokens: i64,
    #[serde(default)]
    pub cache_creation_input_tokens: i64,
}

/// Pending classifier check — captures context for deferred bash classification.
///
/// TS: `PendingClassifierCheck` in types/permissions.ts
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingClassifierCheck {
    pub command: String,
    pub cwd: String,
    #[serde(default)]
    pub descriptions: Vec<String>,
}

/// Source of a permission rule (ordered by priority: Session is most specific).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionRuleSource {
    UserSettings,
    ProjectSettings,
    LocalSettings,
    FlagSettings,
    PolicySettings,
    CliArg,
    Command,
    Session,
}

// ── Shared rule matching functions ──
// Used by both coco-permissions (rule evaluation) and coco-hooks (if condition).

/// Parse a rule string like `"Bash(git *)"` into tool_pattern + rule_content.
///
/// This is a simplified parser for the hook `if` condition syntax. For the
/// full-featured parser with escape handling, see `coco-permissions::rule_compiler`.
pub fn parse_rule_pattern(rule_str: &str) -> PermissionRuleValue {
    if let Some(open) = rule_str.find('(')
        && let Some(close) = rule_str.rfind(')')
        && close > open
        && close == rule_str.len() - 1
    {
        let tool = &rule_str[..open];
        let content = &rule_str[open + 1..close];
        if !tool.is_empty() && !content.is_empty() && content != "*" {
            return PermissionRuleValue {
                tool_pattern: tool.to_string(),
                rule_content: Some(content.to_string()),
            };
        }
        return PermissionRuleValue {
            tool_pattern: tool.to_string(),
            rule_content: None,
        };
    }
    PermissionRuleValue {
        tool_pattern: rule_str.to_string(),
        rule_content: None,
    }
}

/// Check if a tool name matches a rule's tool pattern.
///
/// Supports: exact match, `"*"` wildcard, prefix-wildcard (`"mcp__slack__*"`),
/// and MCP server-level matching (`"mcp__server"` matches `"mcp__server__tool"`).
pub fn tool_matches_pattern(pattern: &str, tool_name: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    if let Some(prefix) = pattern.strip_suffix('*') {
        return tool_name.starts_with(prefix);
    }

    if pattern == tool_name {
        return true;
    }

    // MCP server-level match: rule "mcp__server1" matches "mcp__server1__tool1"
    if pattern.starts_with(crate::MCP_TOOL_PREFIX) && tool_name.starts_with(crate::MCP_TOOL_PREFIX)
    {
        let rule_parts: Vec<&str> = pattern.splitn(3, "__").collect();
        let tool_parts: Vec<&str> = tool_name.splitn(3, "__").collect();
        if rule_parts.len() == 2 && tool_parts.len() == 3 && rule_parts[1] == tool_parts[1] {
            return true;
        }
    }

    false
}

/// Check if tool content matches a rule's content pattern.
///
/// Supports prefix matching (ending with `*`) and exact matching.
pub fn content_matches(rule_content: &str, tool_content: &str) -> bool {
    if rule_content == "*" {
        return true;
    }

    if let Some(prefix) = rule_content.strip_suffix('*') {
        return tool_content.starts_with(prefix);
    }

    rule_content == tool_content
}

/// Check if a tool call matches a parsed rule pattern.
///
/// Combines `tool_matches_pattern` and `content_matches` for convenience.
pub fn matches_rule(
    rule: &PermissionRuleValue,
    tool_name: &str,
    tool_content: Option<&str>,
) -> bool {
    if !tool_matches_pattern(&rule.tool_pattern, tool_name) {
        return false;
    }
    match (&rule.rule_content, tool_content) {
        (Some(rc), Some(tc)) => content_matches(rc, tc),
        (Some(_), None) => false,
        (None, _) => true,
    }
}

/// Permission rule value — tool_pattern is a glob/wildcard expression.
/// Examples: "Read", "Bash(git *)", "mcp__slack__*", "*"
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRuleValue {
    pub tool_pattern: String,
    /// Command pattern within tool (e.g. "git *").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_content: Option<String>,
}

/// A single permission rule.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    pub source: PermissionRuleSource,
    pub behavior: PermissionBehavior,
    pub value: PermissionRuleValue,
}

/// Why a permission decision was made.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PermissionDecisionReason {
    Rule {
        rule: PermissionRule,
    },
    Mode {
        mode: PermissionMode,
    },
    Classifier {
        classifier: String,
        reason: String,
    },
    Hook {
        hook_name: String,
        reason: Option<String>,
    },
    SafetyCheck {
        reason: String,
        classifier_approvable: bool,
    },
    AsyncAgent {
        reason: String,
    },
    User,
    Sandboxed,
}

/// Result of a tool's own permission opinion (the step-1c slot in
/// the central evaluator pipeline).
///
/// TS: `tool.checkPermissions()` returns
/// `{ behavior: 'allow' | 'ask' | 'deny', updatedInput?, feedback? }`
/// or is absent (== passthrough). `Passthrough` is the explicit
/// "no opinion — defer to rule pipeline" signal. Tools that don't
/// implement content-specific safety checks return `Passthrough`.
///
/// Lives in `coco-types` rather than `coco-permissions` so the
/// `coco_tool_runtime::Tool::check_permissions` trait method can
/// reference it without the L4 Tool trait depending on the L3
/// permissions evaluator.
#[derive(Debug, Clone)]
pub enum ToolCheckResult {
    /// Tool has no opinion — continue with rule-based checks.
    Passthrough,
    /// Tool explicitly allows this input. `updated_input` carries
    /// any normalization the tool applied (TS `updatedInput`);
    /// `feedback` carries an optional user-facing rationale that
    /// the evaluator threads onto the resulting `PermissionDecision`.
    Allow {
        updated_input: Option<serde_json::Value>,
        feedback: Option<String>,
    },
    /// Tool requires user confirmation for this input.
    Ask { message: String },
    /// Tool denies this input.
    Deny { message: String },
}

/// The result of a permission check.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum PermissionDecision {
    Allow {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        updated_input: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        feedback: Option<String>,
    },
    Ask {
        message: String,
        #[serde(default)]
        suggestions: Vec<PermissionUpdate>,
    },
    Deny {
        message: String,
        reason: PermissionDecisionReason,
    },
}

/// A permission update action.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PermissionUpdate {
    AddRules {
        rules: Vec<PermissionRule>,
        destination: PermissionUpdateDestination,
    },
    ReplaceRules {
        rules: Vec<PermissionRule>,
        destination: PermissionUpdateDestination,
    },
    RemoveRules {
        rules: Vec<PermissionRule>,
        destination: PermissionUpdateDestination,
    },
    SetMode {
        mode: PermissionMode,
    },
    AddDirectories {
        directories: Vec<String>,
        destination: PermissionUpdateDestination,
    },
    RemoveDirectories {
        directories: Vec<String>,
        destination: PermissionUpdateDestination,
    },
}

impl PermissionUpdate {
    /// Destination of this update, if any. `SetMode` has no destination
    /// (it changes session state, not a settings layer).
    pub const fn destination(&self) -> Option<PermissionUpdateDestination> {
        match self {
            Self::AddRules { destination, .. }
            | Self::ReplaceRules { destination, .. }
            | Self::RemoveRules { destination, .. }
            | Self::AddDirectories { destination, .. }
            | Self::RemoveDirectories { destination, .. } => Some(*destination),
            Self::SetMode { .. } => None,
        }
    }
}

/// Destination for persisting permission updates.
///
/// Persistable destinations (`User`/`Project`/`LocalSettings`) write to
/// disk; in-memory destinations (`Session`/`CliArg`/`Command`) live only
/// for the running session. TS parity: same split as
/// `persistPermissionUpdates` in `PermissionUpdate.ts`.
///
/// `Command` is reserved for rules contributed by an invoked command or
/// skill's frontmatter (`allowed-tools:`). TS parity:
/// `alwaysAllowRules.command` populated by `SkillTool` /
/// `createGetAppStateWithAllowedTools`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionUpdateDestination {
    UserSettings,
    ProjectSettings,
    LocalSettings,
    Session,
    CliArg,
    /// Rules contributed by a command/skill frontmatter `allowed-tools`.
    /// In-memory only; cleared on session end.
    Command,
}

/// Rules grouped by source (for ToolPermissionContext).
pub type PermissionRulesBySource = HashMap<PermissionRuleSource, Vec<PermissionRule>>;

/// Source of a working directory addition.
///
/// TS: `WorkingDirectorySource` in types/permissions.ts
pub type WorkingDirectorySource = PermissionUpdateDestination;

/// Additional working directory info for permission evaluation.
///
/// TS: `AdditionalWorkingDirectory` in types/permissions.ts — tracks source origin.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdditionalWorkingDir {
    pub path: String,
    /// Where this directory was configured from.
    pub source: WorkingDirectorySource,
}

/// Context for evaluating tool permissions.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPermissionContext {
    pub mode: PermissionMode,
    #[serde(default)]
    pub additional_dirs: HashMap<String, AdditionalWorkingDir>,
    #[serde(default)]
    pub allow_rules: PermissionRulesBySource,
    #[serde(default)]
    pub deny_rules: PermissionRulesBySource,
    #[serde(default)]
    pub ask_rules: PermissionRulesBySource,
    #[serde(default)]
    pub bypass_available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_plan_mode: Option<PermissionMode>,
    /// Rules stashed during auto-mode entry (dangerous classifier-bypass rules).
    /// Restored on auto-mode exit.
    ///
    /// TS: `strippedDangerousRules` field on ToolPermissionContext.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stripped_dangerous_rules: Option<PermissionRulesBySource>,
    /// Pre-resolved session plan file path — pushed in by the engine so
    /// the permission evaluator can auto-allow writes to it in Plan mode
    /// without re-deriving the slug. TS parity: `isSessionPlanFile` in
    /// `utils/permissions/filesystem.ts:245-255`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_plan_file: Option<std::path::PathBuf>,
}
