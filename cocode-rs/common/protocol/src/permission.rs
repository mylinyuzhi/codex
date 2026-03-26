//! Permission types for tool execution control.
//!
//! These types control how the agent requests and receives permissions
//! for potentially dangerous operations.

use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::IntoStaticStr;

/// Permission mode that controls how the agent handles tool execution permissions.
///
/// Determines the overall permission strategy for a session.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, Display, IntoStaticStr,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum PermissionMode {
    /// Default mode - ask for permission on sensitive operations.
    #[default]
    Default,
    /// Plan mode - read-only, no execution without approval.
    Plan,
    /// Accept edits automatically, but ask for other operations.
    AcceptEdits,
    /// Bypass all permission checks (dangerous).
    Bypass,
    /// Never ask for permission, deny if not pre-approved.
    DontAsk,
}

impl PermissionMode {
    /// Check if this mode requires explicit approval for writes.
    pub fn requires_write_approval(&self) -> bool {
        matches!(self, PermissionMode::Default | PermissionMode::Plan)
    }

    /// Check if this mode allows automatic edit acceptance.
    pub fn auto_accept_edits(&self) -> bool {
        matches!(self, PermissionMode::AcceptEdits | PermissionMode::Bypass)
    }

    /// Check if this mode bypasses all permission checks.
    pub fn is_bypass(&self) -> bool {
        matches!(self, PermissionMode::Bypass)
    }

    /// Cycle to the next permission mode.
    ///
    /// Claude Code cycles: Default → AcceptEdits → Plan → Default.
    /// Bypass and DontAsk don't participate in the cycle (they stay as-is).
    pub fn next_cycle(&self) -> Self {
        match self {
            PermissionMode::Default => PermissionMode::AcceptEdits,
            PermissionMode::AcceptEdits => PermissionMode::Plan,
            PermissionMode::Plan => PermissionMode::Default,
            // Bypass and DontAsk are sticky — they don't participate in the cycle
            other => *other,
        }
    }

    /// Get the mode as a string.
    pub fn as_str(&self) -> &'static str {
        (*self).into()
    }
}

/// Result of a permission check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum PermissionResult {
    /// Operation is allowed to proceed.
    Allowed,
    /// Operation is denied.
    Denied {
        /// Reason for denial.
        reason: String,
    },
    /// Operation needs user approval before proceeding.
    NeedsApproval {
        /// The approval request to present to the user.
        request: ApprovalRequest,
    },
    /// No rule matched — fall through to defaults.
    ///
    /// Tools return this from `check_permission()` when they have no
    /// opinion, letting the pipeline apply default behavior
    /// (reads → Allow, writes → NeedsApproval).
    Passthrough,
}

impl PermissionResult {
    /// Check if the operation is allowed.
    pub fn is_allowed(&self) -> bool {
        matches!(self, PermissionResult::Allowed)
    }

    /// Check if the operation is denied.
    pub fn is_denied(&self) -> bool {
        matches!(self, PermissionResult::Denied { .. })
    }

    /// Check if the operation needs approval.
    pub fn needs_approval(&self) -> bool {
        matches!(self, PermissionResult::NeedsApproval { .. })
    }

    /// Check if no rule matched (passthrough to defaults).
    pub fn is_passthrough(&self) -> bool {
        matches!(self, PermissionResult::Passthrough)
    }
}

/// A permission decision with additional context about why the decision was made.
///
/// This wraps `PermissionResult` with metadata about which rule matched
/// and from which source, enabling debugging and audit logging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionDecision {
    /// The permission result.
    pub result: PermissionResult,
    /// Human-readable reason for the decision.
    pub reason: String,
    /// The source of the rule that matched (if applicable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<RuleSource>,
    /// The pattern that matched (if applicable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched_pattern: Option<String>,
}

impl PermissionDecision {
    /// Create an allowed decision with a reason.
    pub fn allowed(reason: impl Into<String>) -> Self {
        Self {
            result: PermissionResult::Allowed,
            reason: reason.into(),
            source: None,
            matched_pattern: None,
        }
    }

    /// Create a denied decision with a reason.
    pub fn denied(reason: impl Into<String>) -> Self {
        let reason = reason.into();
        Self {
            result: PermissionResult::Denied {
                reason: reason.clone(),
            },
            reason,
            source: None,
            matched_pattern: None,
        }
    }

    /// Create a "needs approval" decision for an ask rule.
    pub fn ask(reason: impl Into<String>) -> Self {
        let reason = reason.into();
        Self {
            result: PermissionResult::NeedsApproval {
                request: ApprovalRequest::default(),
            },
            reason,
            source: None,
            matched_pattern: None,
        }
    }

    /// Set the rule source.
    pub fn with_source(mut self, source: RuleSource) -> Self {
        self.source = Some(source);
        self
    }

    /// Set the matched pattern.
    pub fn with_pattern(mut self, pattern: impl Into<String>) -> Self {
        self.matched_pattern = Some(pattern.into());
        self
    }

    /// Check if the operation is allowed.
    pub fn is_allowed(&self) -> bool {
        self.result.is_allowed()
    }
}

/// Source of a permission rule.
///
/// Ordering: smaller value = higher priority (checked first).
/// Persistent file-based sources are checked first so that configured
/// deny rules cannot be bypassed by session-level overrides. Within
/// each behavior step (deny/ask/allow), the first matching source wins.
///
/// Priority: User > Project > Local > Flag > Policy > Cli > Command > Session.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, IntoStaticStr,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum RuleSource {
    /// User-level settings (~/.cocode/settings.json) — highest priority.
    User,
    /// Project-level settings (.cocode/settings.json in project).
    Project,
    /// Local settings (.cocode/settings.local.json).
    Local,
    /// CLI flag overrides.
    Flag,
    /// Enterprise managed policy (read-only).
    Policy,
    /// CLI argument overrides.
    Cli,
    /// Per-command overrides.
    Command,
    /// Session-level approvals (runtime grants) — lowest priority.
    Session,
}

impl RuleSource {
    /// Returns a numeric priority value. Lower = higher priority (checked first).
    ///
    /// Persistent file-based sources (User, Project, Local) are checked before
    /// runtime sources (Cli, Command, Session) so that configured deny rules
    /// cannot be bypassed by session-level overrides.
    fn priority(self) -> i32 {
        match self {
            RuleSource::User => 0,
            RuleSource::Project => 1,
            RuleSource::Local => 2,
            RuleSource::Flag => 3,
            RuleSource::Policy => 4,
            RuleSource::Cli => 5,
            RuleSource::Command => 6,
            RuleSource::Session => 7,
        }
    }
}

impl PartialOrd for RuleSource {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RuleSource {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.priority().cmp(&other.priority())
    }
}

impl RuleSource {
    /// Whether this source represents a persistent (file-based) rule.
    pub fn is_persistent(&self) -> bool {
        matches!(
            self,
            RuleSource::User | RuleSource::Project | RuleSource::Local | RuleSource::Policy
        )
    }

    /// Get the source as a string.
    pub fn as_str(&self) -> &'static str {
        (*self).into()
    }
}

/// A prompt-based permission declared in a plan's `allowedPrompts`.
///
/// When the user approves a plan via `ExitPlanMode`, these pre-declared
/// permissions are injected into the session's approval store so the
/// corresponding tool invocations proceed without further prompting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllowedPrompt {
    /// The tool this permission applies to (e.g. "Bash").
    pub tool: String,
    /// Semantic description of the permitted action (e.g. "run tests").
    pub prompt: String,
}

/// Result of a user approval interaction.
///
/// Returned from `PermissionRequester::request_permission()` to convey
/// the user's three-way choice: approve once, approve similar, or deny.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ApprovalDecision {
    /// Approve this one execution only.
    Approved,
    /// Approve and remember a prefix pattern for similar future commands.
    ApprovedWithPrefix {
        /// The prefix pattern to remember, e.g. "git *".
        prefix_pattern: String,
    },
    /// Deny the operation.
    Denied,
}

/// Plan exit option for the multi-choice ExitPlanMode approval dialog.
///
/// Matches Claude Code's 5-way ExitPlanMode dialog:
/// 1. Clear context + auto-accept edits (most common)
/// 2. Clear context + bypass permissions
/// 3. Keep context + elevate to accept-edits
/// 4. Keep context + manual approve (restore pre-plan mode)
/// 5. Keep planning (deny)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlanExitOption {
    /// Approve: clear conversation context, switch to AcceptEdits mode.
    ///
    /// The plan content is injected as the first message in the fresh conversation.
    ClearAndAcceptEdits,
    /// Approve: clear conversation context, switch to Bypass mode.
    ///
    /// For advanced users who want full auto-approval.
    ClearAndBypass,
    /// Approve: keep current context, elevate to AcceptEdits mode.
    ///
    /// Useful when context is small and doesn't need clearing.
    KeepAndElevate,
    /// Approve: keep current context, restore pre-plan mode (manual approve).
    ///
    /// Returns to Default mode so every tool call still requires manual approval.
    KeepAndDefault,
    /// Deny: stay in plan mode and continue planning.
    KeepPlanning,
}

impl PlanExitOption {
    /// Whether this option approves the plan (vs continuing to plan).
    pub fn is_approved(&self) -> bool {
        !matches!(self, PlanExitOption::KeepPlanning)
    }

    /// Whether this option requires clearing the conversation context.
    pub fn should_clear_context(&self) -> bool {
        matches!(
            self,
            PlanExitOption::ClearAndAcceptEdits | PlanExitOption::ClearAndBypass
        )
    }

    /// Whether this option keeps the current conversation context.
    pub fn should_keep_context(&self) -> bool {
        matches!(
            self,
            PlanExitOption::KeepAndElevate | PlanExitOption::KeepAndDefault
        )
    }

    /// The target permission mode after exiting plan mode.
    pub fn target_mode(&self) -> Option<PermissionMode> {
        match self {
            PlanExitOption::ClearAndAcceptEdits | PlanExitOption::KeepAndElevate => {
                Some(PermissionMode::AcceptEdits)
            }
            PlanExitOption::ClearAndBypass => Some(PermissionMode::Bypass),
            PlanExitOption::KeepAndDefault => Some(PermissionMode::Default),
            PlanExitOption::KeepPlanning => None,
        }
    }
}

/// Request for user approval of an operation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    /// Unique identifier for this request.
    pub request_id: String,
    /// The tool requesting approval.
    pub tool_name: String,
    /// Human-readable description of what will happen.
    pub description: String,
    /// Security risks associated with this operation.
    #[serde(default)]
    pub risks: Vec<SecurityRisk>,
    /// Whether this can be auto-approved for similar future operations.
    #[serde(default)]
    pub allow_remember: bool,
    /// Proposed command prefix pattern for "allow similar" option.
    /// E.g. "git *" for command "git push origin main".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposed_prefix_pattern: Option<String>,
    /// Tool input parameters (for SDK clients to make informed decisions).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<serde_json::Value>,
}

/// A security risk associated with an operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityRisk {
    /// Type of risk.
    pub risk_type: RiskType,
    /// Severity of the risk.
    pub severity: RiskSeverity,
    /// Human-readable description of the risk.
    pub message: String,
}

/// Type of security risk.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, IntoStaticStr,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum RiskType {
    /// Operation could destroy or modify data.
    Destructive,
    /// Operation involves network access.
    Network,
    /// Operation modifies system configuration.
    SystemConfig,
    /// Operation accesses sensitive files.
    SensitiveFile,
    /// Operation requires elevated privileges.
    Elevated,
    /// Unknown or unclassified risk.
    Unknown,
}

impl RiskType {
    /// Get the risk type as a string.
    pub fn as_str(&self) -> &'static str {
        (*self).into()
    }
}

/// Severity level of a security risk.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    Display,
    IntoStaticStr,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum RiskSeverity {
    /// Low severity - minor impact.
    Low,
    /// Medium severity - moderate impact.
    Medium,
    /// High severity - significant impact.
    High,
    /// Critical severity - severe impact.
    Critical,
}

impl RiskSeverity {
    /// Get the severity as a string.
    pub fn as_str(&self) -> &'static str {
        (*self).into()
    }

    /// Check if this severity is at least the given level.
    pub fn at_least(&self, other: RiskSeverity) -> bool {
        *self >= other
    }
}

#[cfg(test)]
#[path = "permission.test.rs"]
mod tests;
