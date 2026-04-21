//! Modal overlay types.
//!
//! Split from `state/ui.rs` to keep that file under the 800-LoC module-size
//! guidance. See `crate-coco-tui.md` §Overlay System for the taxonomy and
//! `state/ui.rs::UiState::set_overlay` for displacement semantics.

/// Modal overlay variants.
#[derive(Debug, Clone)]
pub enum Overlay {
    /// Tool permission approval (Y/N/A).
    Permission(PermissionOverlay),
    /// Help display (keyboard shortcuts).
    Help,
    /// Error message.
    Error(String),
    /// Plan mode exit approval.
    PlanExit(PlanExitOverlay),
    /// Plan mode entry approval.
    PlanEntry(PlanEntryOverlay),
    /// Cost warning.
    CostWarning(CostWarningOverlay),
    /// Model picker (Ctrl+M).
    ModelPicker(ModelPickerOverlay),
    /// Command palette (Ctrl+P).
    CommandPalette(CommandPaletteOverlay),
    /// Session browser (Ctrl+S).
    SessionBrowser(SessionBrowserOverlay),
    /// Question from agent (AskUserQuestion tool).
    Question(QuestionOverlay),
    /// MCP elicitation form.
    Elicitation(ElicitationOverlay),
    /// Sandbox permission.
    SandboxPermission(SandboxPermissionOverlay),
    /// Global search (ripgrep streaming).
    GlobalSearch(GlobalSearchOverlay),
    /// Quick file open (Ctrl+O).
    QuickOpen(QuickOpenOverlay),
    /// Transcript export.
    Export(ExportOverlay),
    /// Full-screen diff view.
    DiffView(DiffViewOverlay),
    /// MCP server approval.
    McpServerApproval(McpServerApprovalOverlay),
    /// Worktree exit confirmation.
    WorktreeExit(WorktreeExitOverlay),
    /// Doctor/diagnostics.
    Doctor(DoctorOverlay),
    /// Bridge dialog (IDE/REPL).
    Bridge(BridgeOverlay),
    /// Invalid config warning.
    InvalidConfig(InvalidConfigOverlay),
    /// Idle return confirmation.
    IdleReturn(IdleReturnOverlay),
    /// Trust dialog.
    Trust(TrustOverlay),
    /// Auto mode opt-in.
    AutoModeOptIn(AutoModeOptInOverlay),
    /// Bypass permissions confirmation.
    BypassPermissions(BypassPermissionsOverlay),
    /// Background task detail.
    TaskDetail(TaskDetailOverlay),
    /// Feedback survey.
    Feedback(FeedbackOverlay),
    /// MCP server multi-select.
    McpServerSelect(McpServerSelectOverlay),
    /// Context window visualization.
    ContextVisualization,
    /// Rewind overlay (message selector + restore options).
    /// TS: MessageSelector component.
    Rewind(crate::state::rewind::RewindOverlay),
    /// Tabbed settings panel (theme, output style, permissions, about).
    /// TS: src/components/Settings/.
    Settings(crate::widgets::settings_panel::SettingsPanelState),
    /// Team lead approval for a teammate's plan (received via mailbox).
    /// TS: `planApprovalOverlay` + `PlanApprovalRequest` flow in
    /// `tools/ExitPlanModeTool/ExitPlanModeV2Tool.ts`.
    PlanApproval(PlanApprovalOverlay),
}

impl Overlay {
    /// Priority ranking — lower number wins. See `crate-coco-tui.md` §Overlay
    /// Priority System. Used by `UiState::set_overlay` for displacement and
    /// queue ordering. Agent-driven overlays arriving while a lower-priority
    /// one is active will displace it; user-triggered overlays on top of a
    /// higher-priority agent overlay queue behind it.
    pub fn priority(&self) -> i32 {
        match self {
            // 0 — security-critical
            Self::SandboxPermission(_) => 0,
            // 1 — blocks agent execution (needs approval to continue)
            Self::Permission(_) | Self::PlanExit(_) | Self::PlanEntry(_) => 1,
            // 2 — tool or agent awaiting structured input
            Self::Question(_)
            | Self::Elicitation(_)
            | Self::McpServerApproval(_)
            | Self::IdleReturn(_)
            | Self::PlanApproval(_) => 2,
            // 3 — high-stakes confirmation
            Self::CostWarning(_) | Self::BypassPermissions(_) | Self::WorktreeExit(_) => 3,
            // 4 — error surface
            Self::Error(_) | Self::InvalidConfig(_) => 4,
            // 5 — content review
            Self::Rewind(_) | Self::DiffView(_) => 5,
            // 6 — settings confirmation
            Self::AutoModeOptIn(_)
            | Self::Trust(_)
            | Self::Bridge(_)
            | Self::McpServerSelect(_) => 6,
            // 7 — user-triggered pickers, visualizations, settings
            Self::ModelPicker(_)
            | Self::CommandPalette(_)
            | Self::SessionBrowser(_)
            | Self::GlobalSearch(_)
            | Self::QuickOpen(_)
            | Self::Export(_)
            | Self::Feedback(_)
            | Self::TaskDetail(_)
            | Self::Doctor(_)
            | Self::ContextVisualization
            | Self::Settings(_) => 7,
            // 8 — help (read-only reference)
            Self::Help => 8,
        }
    }
}

/// Permission approval overlay with tool-specific detail.
///
/// TS: src/components/permissions/ (51 files, 12K LOC)
/// Each tool type has a specialized review UI.
#[derive(Debug, Clone)]
pub struct PermissionOverlay {
    pub request_id: String,
    pub tool_name: String,
    pub description: String,
    pub detail: PermissionDetail,
    /// Risk level badge from the permission explainer.
    /// TS: PermissionRequestTitle shows color-coded LOW/MEDIUM/HIGH badge.
    pub risk_level: Option<RiskLevel>,
    /// Whether "Always Allow" option should be shown (gated by policy).
    /// TS: shouldShowAlwaysAllowOptions() in permissionsLoader.ts
    pub show_always_allow: bool,
    /// Whether a background classifier check is in progress.
    /// TS: `classifierCheckInProgress` in ToolUseConfirm.
    pub classifier_checking: bool,
    /// Set when classifier auto-approved; shows checkmark before dismissal.
    /// TS: `classifierAutoApproved` + `classifierMatchedRule` in ToolUseConfirm.
    pub classifier_auto_approved: Option<String>,
}

/// Risk level for permission explainer badge.
///
/// TS: types/permissions.ts — RiskLevel = 'LOW' | 'MEDIUM' | 'HIGH'
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

/// Tool-specific permission review content.
///
/// Matches TS's 12 specialized permission request components.
#[derive(Debug, Clone)]
pub enum PermissionDetail {
    /// Bash command — show command, risk level, working directory.
    /// TS: BashPermissionRequest/ (108KB)
    Bash {
        command: String,
        risk_description: Option<String>,
        working_dir: Option<String>,
    },
    /// File edit — show path and unified diff.
    /// TS: FileEditPermissionRequest/ (16KB)
    FileEdit { path: String, diff: String },
    /// File write — show path and content preview.
    /// TS: FileWritePermissionRequest/ (40KB)
    FileWrite {
        path: String,
        content_preview: String,
        is_new_file: bool,
    },
    /// Filesystem operation (mkdir, rm, mv, cp).
    /// TS: FilesystemPermissionRequest/ (13KB)
    Filesystem { operation: String, path: String },
    /// Web fetch — show URL.
    /// TS: WebFetchPermissionRequest/ (32KB)
    WebFetch { url: String, method: String },
    /// Skill execution — show skill name and description.
    /// TS: SkillPermissionRequest/ (36KB)
    Skill {
        skill_name: String,
        skill_description: Option<String>,
    },
    /// Sed in-place edit — show pattern and replacement.
    /// TS: SedEditPermissionRequest/ (32KB)
    SedEdit {
        path: String,
        pattern: String,
        replacement: String,
    },
    /// Notebook cell edit — show path, cell, and change.
    /// TS: NotebookEditPermissionRequest/ (56KB)
    NotebookEdit {
        path: String,
        cell_id: String,
        change_preview: String,
    },
    /// MCP tool call — show server and tool.
    McpTool {
        server_name: String,
        tool_name: String,
        input_preview: String,
    },
    /// PowerShell command approval.
    PowerShell {
        command: String,
        risk_description: Option<String>,
        working_dir: Option<String>,
    },
    /// Computer use (screen/mouse/keyboard) approval.
    ComputerUse { action: String, description: String },
    /// Generic fallback — plain text description.
    Generic { input_preview: String },
}

/// Plan mode exit overlay.
#[derive(Debug, Clone, Default)]
pub struct PlanExitOverlay {
    pub plan_content: Option<String>,
    /// The mode to switch INTO after ExitPlanMode is approved. Set by
    /// the user via the approval options (TS parity: "Yes, Accept Edits"
    /// / "Yes, Bypass" / "Yes, keep default"). On `RestorePrePlan`,
    /// plan-mode restoration falls back to `ctx.pre_plan_mode`.
    pub next_mode: PlanExitTarget,
}

/// Mode to switch into after the user approves ExitPlanMode.
///
/// TS: `buildPlanApprovalOptions()` — the approval dropdown lets the
/// user pick how much elevation they want on exit. We keep a compact
/// three-way choice; the full TS matrix (clear-context, Ultraplan, etc.)
/// is Anthropic-specific or feature-gated.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanExitTarget {
    /// Restore whichever mode the session had before entering plan mode
    /// (via `ToolPermissionContext.pre_plan_mode`). TS: `'yes-default-keep-context'`.
    #[default]
    RestorePrePlan,
    /// Switch to `AcceptEdits` so file writes don't re-prompt.
    /// TS: `'yes-accept-edits'`.
    AcceptEdits,
    /// Switch to `BypassPermissions`. Requires the session to have the
    /// bypass gate enabled. TS: `'yes-bypass-permissions'`.
    BypassPermissions,
}

impl PlanExitTarget {
    /// The permission mode this target resolves to. `RestorePrePlan`
    /// returns `None` to signal the engine should use `pre_plan_mode`.
    pub fn resolve(self) -> Option<coco_types::PermissionMode> {
        match self {
            Self::RestorePrePlan => None,
            Self::AcceptEdits => Some(coco_types::PermissionMode::AcceptEdits),
            Self::BypassPermissions => Some(coco_types::PermissionMode::BypassPermissions),
        }
    }

    /// Ordered list of exit targets offered to the user for a given
    /// capability gate. `BypassPermissions` is only included when the
    /// session was authorized to reach it at startup — matching TS
    /// `buildPlanApprovalOptions()` which conditionally renders the
    /// "Yes, and bypass permissions" entry on
    /// `isBypassPermissionsModeAvailable`.
    pub fn available(bypass_permissions_available: bool) -> Vec<Self> {
        let mut out = vec![Self::RestorePrePlan, Self::AcceptEdits];
        if bypass_permissions_available {
            out.push(Self::BypassPermissions);
        }
        out
    }
}

/// Cost warning overlay.
#[derive(Debug, Clone)]
pub struct CostWarningOverlay {
    pub current_cost_cents: i64,
    pub threshold_cents: i64,
}

/// Model picker overlay (filterable list).
#[derive(Debug, Clone)]
pub struct ModelPickerOverlay {
    pub models: Vec<ModelOption>,
    pub filter: String,
    pub selected: i32,
}

/// A selectable model option.
#[derive(Debug, Clone)]
pub struct ModelOption {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
}

/// Command palette overlay (filterable list of /commands).
#[derive(Debug, Clone)]
pub struct CommandPaletteOverlay {
    pub commands: Vec<CommandOption>,
    pub filter: String,
    pub selected: i32,
}

/// A selectable command option.
#[derive(Debug, Clone)]
pub struct CommandOption {
    pub name: String,
    pub description: Option<String>,
}

/// Session browser overlay (list of saved sessions).
#[derive(Debug, Clone)]
pub struct SessionBrowserOverlay {
    pub sessions: Vec<SessionOption>,
    pub filter: String,
    pub selected: i32,
}

/// A selectable session option.
#[derive(Debug, Clone)]
pub struct SessionOption {
    pub id: String,
    pub label: String,
    pub message_count: i32,
    pub created_at: String,
}

/// Question overlay (AskUserQuestion tool).
#[derive(Debug, Clone)]
pub struct QuestionOverlay {
    pub request_id: String,
    pub question: String,
    pub options: Vec<String>,
    pub selected: i32,
}

/// MCP elicitation form overlay.
#[derive(Debug, Clone)]
pub struct ElicitationOverlay {
    pub request_id: String,
    pub server_name: String,
    pub message: String,
    pub fields: Vec<ElicitationField>,
}

/// A field in an elicitation form.
#[derive(Debug, Clone)]
pub struct ElicitationField {
    pub name: String,
    pub description: Option<String>,
    pub value: String,
}

/// Sandbox permission overlay.
#[derive(Debug, Clone)]
pub struct SandboxPermissionOverlay {
    pub request_id: String,
    pub description: String,
}

/// Plan mode entry overlay.
#[derive(Debug, Clone)]
pub struct PlanEntryOverlay {
    pub description: String,
}

/// Global search overlay (ripgrep streaming).
#[derive(Debug, Clone)]
pub struct GlobalSearchOverlay {
    pub query: String,
    pub results: Vec<SearchResult>,
    pub selected: i32,
    pub is_searching: bool,
}

/// A global search result entry.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub file: String,
    pub line_number: i32,
    pub content: String,
}

/// Quick file open overlay.
#[derive(Debug, Clone)]
pub struct QuickOpenOverlay {
    pub filter: String,
    pub files: Vec<String>,
    pub selected: i32,
}

/// Export dialog overlay.
#[derive(Debug, Clone)]
pub struct ExportOverlay {
    pub formats: Vec<ExportFormat>,
    pub selected: i32,
}

/// Supported export formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Markdown,
    Json,
    Text,
}

impl ExportFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Markdown => "Markdown (.md)",
            Self::Json => "JSON (.json)",
            Self::Text => "Plain Text (.txt)",
        }
    }
}

/// Full-screen diff view overlay.
#[derive(Debug, Clone)]
pub struct DiffViewOverlay {
    pub path: String,
    pub diff: String,
    pub scroll: i32,
}

/// MCP server approval overlay.
#[derive(Debug, Clone)]
pub struct McpServerApprovalOverlay {
    pub server_name: String,
    pub server_url: Option<String>,
    pub tools: Vec<String>,
    pub request_id: String,
}

/// Worktree exit confirmation overlay.
#[derive(Debug, Clone)]
pub struct WorktreeExitOverlay {
    pub branch: String,
    pub has_uncommitted: bool,
    pub changed_files: Vec<String>,
}

/// Doctor/diagnostics overlay.
#[derive(Debug, Clone)]
pub struct DoctorOverlay {
    pub checks: Vec<DoctorCheck>,
}

/// A single doctor check result.
#[derive(Debug, Clone)]
pub struct DoctorCheck {
    pub name: String,
    pub passed: bool,
    pub message: String,
}

/// Bridge dialog overlay (IDE/REPL).
#[derive(Debug, Clone)]
pub struct BridgeOverlay {
    pub bridge_type: String,
    pub status: String,
    pub details: String,
}

/// Invalid config warning overlay.
#[derive(Debug, Clone)]
pub struct InvalidConfigOverlay {
    pub errors: Vec<String>,
}

/// Idle return confirmation overlay.
#[derive(Debug, Clone)]
pub struct IdleReturnOverlay {
    pub idle_duration_secs: i64,
}

/// Trust dialog overlay.
#[derive(Debug, Clone)]
pub struct TrustOverlay {
    pub path: String,
    pub description: String,
}

/// Auto mode opt-in overlay.
#[derive(Debug, Clone)]
pub struct AutoModeOptInOverlay {
    pub description: String,
}

/// Bypass permissions confirmation overlay.
#[derive(Debug, Clone)]
pub struct BypassPermissionsOverlay {
    pub current_mode: String,
}

/// Background task detail overlay.
#[derive(Debug, Clone)]
pub struct TaskDetailOverlay {
    pub task_id: String,
    pub task_type: String,
    pub description: String,
    pub output: String,
    pub status: String,
    pub scroll: i32,
}

/// Feedback survey overlay.
#[derive(Debug, Clone)]
pub struct FeedbackOverlay {
    pub prompt: String,
    pub options: Vec<String>,
    pub selected: i32,
}

/// MCP server multi-select overlay.
#[derive(Debug, Clone)]
pub struct McpServerSelectOverlay {
    pub servers: Vec<McpServerOption>,
    pub filter: String,
}

/// Selectable MCP server option.
#[derive(Debug, Clone)]
pub struct McpServerOption {
    pub name: String,
    pub selected: bool,
    pub tool_count: i32,
}

/// Plan-approval overlay shown to the team lead when a teammate sends
/// a `plan_approval_request` via mailbox. The leader picks approve /
/// deny (+ optional feedback); the TUI dispatches
/// `UserCommand::PlanApprovalResponse` which the engine writes back to
/// the teammate's inbox.
///
/// TS source: `tools/ExitPlanModeTool/ExitPlanModeV2Tool.ts:137-141`
/// builds the request; leader side surfaces via an ink modal.
#[derive(Debug, Clone)]
pub struct PlanApprovalOverlay {
    /// Correlation id that will travel back in the response.
    pub request_id: String,
    /// Teammate agent name (who sent the request).
    pub from: String,
    /// Optional plan-file path on disk (.claude/plans/...). `None` when
    /// the request embeds the content inline instead.
    pub plan_file_path: Option<String>,
    /// The plan text itself (rendered markdown) — always present so the
    /// leader can review without opening a file.
    pub plan_content: String,
    /// Focused button index: 0 = Approve, 1 = Deny.
    pub focused: u8,
}

impl PlanApprovalOverlay {
    pub fn new(
        request_id: String,
        from: String,
        plan_file_path: Option<String>,
        plan_content: String,
    ) -> Self {
        Self {
            request_id,
            from,
            plan_file_path,
            plan_content,
            focused: 0,
        }
    }

    pub fn toggle_focus(&mut self) {
        self.focused = if self.focused == 0 { 1 } else { 0 };
    }

    pub fn is_approve_focused(&self) -> bool {
        self.focused == 0
    }
}

#[cfg(test)]
#[path = "overlay.test.rs"]
mod tests;
