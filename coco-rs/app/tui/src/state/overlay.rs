//! Modal overlay types.
//!
//! Split from `state/ui.rs` to keep that file under the 800-LoC module-size
//! guidance. See `crate-coco-tui.md` §Overlay System for the taxonomy and
//! `state/ui.rs::UiState::set_overlay` for displacement semantics.

use super::session::ProviderUnavailableReason;

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
    /// `/memory` file-picker — list managed/user/project/local CLAUDE.md
    /// entries and open the chosen file in `$VISUAL || $EDITOR`.
    /// TS: `commands/memory/memory.tsx::Dialog<MemoryFileSelector>`.
    MemoryDialog(MemoryDialogOverlay),
    /// Verbose, scrollable view of the entire conversation including
    /// hidden meta messages. TS `app:toggleTranscript` (`screen ===
    /// 'transcript'` branch in `screens/REPL.tsx:4392`). coco-rs ports
    /// the read-only essentials: full message dump with no truncation,
    /// scrollable, dismissed via Esc / Ctrl+O.
    Transcript(TranscriptOverlay),
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
            | Self::Settings(_)
            | Self::Transcript(_)
            | Self::MemoryDialog(_) => 7,
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
    /// Optional multi-choice payload. `None` means render the classic
    /// yes/no/always dialog. `Some` switches the renderer into a
    /// choice-list mode (mirrors `QuestionOverlay`): Up/Down moves
    /// `selected_choice`, Enter (approve) echoes the picked value back
    /// to the tool via `UserCommand::ApprovalResponse.updated_input`.
    ///
    /// TS parity: `ExitPlanModePermissionRequest.tsx:691-704` option
    /// grid, gated on `settings.showClearContextOnPlanAccept`.
    pub choices: Option<Vec<coco_types::PermissionAskChoice>>,
    /// Cursor position within `choices`. Meaningless when
    /// `choices.is_none()`.
    pub selected_choice: usize,
    /// Raw tool input captured at dialog-open time. Used by the
    /// approve path to splice `user_choice` in without losing other
    /// fields the model originally supplied. Carried only when
    /// `choices.is_some()`.
    pub original_input: Option<serde_json::Value>,
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

/// Model picker overlay — provider-grouped list of `(provider, model_id)`
/// candidates plus an inline thinking-effort selector. Tab cycles the
/// target role (Main / Fast / Plan / …); the confirm path persists
/// to that role's slot in `~/.coco.json::model_roles`.
///
/// TS parity reference: `components/ModelPicker.tsx`. coco-rs extends
/// the TS shape with a role pill so multi-provider users can configure
/// every `ModelRole` from the same surface — TS only ever drives the
/// `main` model.
#[derive(Debug, Clone)]
pub struct ModelPickerOverlay {
    /// Which role we're configuring. Defaults to `Main` when launched
    /// by `Ctrl+M` / `/model`; Tab cycles forward, Shift+Tab back.
    pub role: coco_types::ModelRole,
    /// Available model entries, pre-sorted by `(provider, display_name)`
    /// so the rendered list is stable and provider headers fall
    /// naturally between consecutive same-provider rows.
    pub entries: Vec<ModelEntry>,
    /// Substring filter, lowercased — matches `display_name` and
    /// `provider_display`.
    pub filter: String,
    /// Index into the *filtered* entries list (0-based, headers skipped
    /// because they aren't selectable rows).
    pub selected: i32,
    /// Currently-chosen effort for the focused model. Re-derived from
    /// `default_effort` on every selection change (see `update::overlay`).
    /// `None` when the focused model declares no thinking levels.
    pub effort: Option<coco_types::ReasoningEffort>,
}

/// One row in the picker — pre-resolved against the registry so the
/// renderer never has to reach back into config.
#[derive(Debug, Clone)]
pub struct ModelEntry {
    /// Canonical provider id (e.g. `"anthropic"`). Used as the persistence key.
    pub provider: String,
    /// Display label rendered in the section header (e.g. `"Anthropic"`).
    pub provider_display: String,
    pub model_id: String,
    /// Pretty name; falls back to `model_id` when registry lacks one.
    pub display_name: String,
    /// Context window in tokens — rendered as `"1M"` / `"200K"`.
    pub context_window: Option<i64>,
    /// Efforts the model supports; drives the inline footer cycle.
    /// Empty when the model has no thinking capability — the effort
    /// footer is then hidden.
    pub supported_efforts: Vec<coco_types::ReasoningEffort>,
    /// Model's preferred effort when none chosen explicitly.
    pub default_effort: Option<coco_types::ReasoningEffort>,
    /// `true` when this entry is the role's currently-applied selection.
    /// Rendered with a `[current]` badge.
    pub is_current_for_role: bool,
    /// Provider config issues that prevent this row from being selected.
    pub unavailable_reasons: Vec<ProviderUnavailableReason>,
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
///
/// Mirrors the TS `AskUserQuestionPermissionRequest.tsx` data model:
/// up to 4 questions per call, each with 2-4 options, optional preview
/// content per option, and optional per-option notes captured by the
/// user. Supports both single-select (radio) and multi-select (checkbox)
/// modes per question, plus the two TS footer affordances:
/// "Chat about this" (always shown) and "Skip interview and plan
/// immediately" (plan-mode only).
///
/// Submit semantics:
/// - Enter on [`QuestionFocus::Question`] when on the LAST question →
///   ship `UserCommand::ApprovalResponse { approved: true, updated_input:
///   Some({...original_input, answers, annotations}) }`. TS:
///   `submitAnswers` (`AskUserQuestionPermissionRequest.tsx:407`).
/// - Enter on `QuestionFocus::Question` (not last) → advance focus to
///   next question. TS: `nextQuestion` / `Submit` button on intermediate
///   questions.
/// - Enter on [`QuestionFocus::ChatAboutThis`] → ship
///   `ApprovalResponse { approved: false, feedback: Some(<synthesized>) }`
///   with the TS-mirrored clarification prose. TS:
///   `handleRespondToClaude`.
/// - Enter on [`QuestionFocus::SkipInterview`] → same with skip-interview
///   prose. TS: `handleFinishPlanInterview`. Only reachable when
///   `is_in_plan_mode`.
#[derive(Debug, Clone)]
pub struct QuestionOverlay {
    pub request_id: String,
    /// Original tool input dict, stored verbatim so the answer payload
    /// can re-emit fields the model supplied that the TUI doesn't render
    /// (e.g. `metadata.source`). Stored AND re-emitted because the
    /// splice protocol in `update/overlay.rs` rebuilds the input as
    /// `{...original_input, answers, annotations}` — dropping the
    /// `original_input` spread would silently strip those fields.
    pub original_input: serde_json::Value,
    pub questions: Vec<QuestionItem>,
    /// Currently focused element (question index OR a footer item).
    /// Tab cycles forward, Shift+Tab cycles backward.
    pub focus: QuestionFocus,
    /// Plan-mode gate for the Skip-interview footer item. Set from
    /// `state.session.permission_mode == PermissionMode::Plan` when the
    /// overlay is constructed.
    pub is_in_plan_mode: bool,
}

/// What the user is currently focused on in the question overlay.
///
/// TS reference: `AskUserQuestionPermissionRequest.tsx` tracks
/// `currentQuestionIndex` + `isFooterFocused` + `footerIndex`. Coco
/// collapses these into a single enum so the focus state machine is
/// linearizable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestionFocus {
    /// On the Nth question (0-indexed). `selected` within that question
    /// drives radio/checkbox selection.
    Question(i32),
    /// Footer "Chat about this" item — always available.
    ChatAboutThis,
    /// Footer "Skip interview and plan immediately" item — only
    /// reachable when `QuestionOverlay.is_in_plan_mode`.
    SkipInterview,
}

/// One question in the AskUserQuestion overlay.
#[derive(Debug, Clone)]
pub struct QuestionItem {
    /// Short label rendered as a chip — e.g. "Auth method".
    pub header: String,
    /// Full question text — typically ends with "?".
    pub question: String,
    pub options: Vec<QuestionOption>,
    /// `true` allows checkbox-style multi-selection, `false` is radio.
    pub multi_select: bool,
    /// Currently focused option index (drives navigation + radio selection).
    pub selected: i32,
    /// Indices toggled on for multi-select. Empty in single-select mode
    /// (the Enter handler then falls back to `selected`).
    pub checked: Vec<i32>,
    /// Free-form text typed by the user. Used both as "notes" annotation
    /// (TS `questionStates[q].textInputValue`) AND as the answer body
    /// when the focused option is the injected "Other" option. The
    /// answer-build logic in `update/overlay.rs` differentiates by
    /// inspecting the focused option's label.
    pub notes: String,
    /// `true` while typed characters route to `notes` instead of moving
    /// focus between options. Set automatically when focus moves to the
    /// "Other" option (`__other__` label) — TS:
    /// `QuestionView.tsx:85-87` `isOtherFocused`.
    pub editing_notes: bool,
}

/// One choice within a [`QuestionItem`].
#[derive(Debug, Clone)]
pub struct QuestionOption {
    /// 1-5 word label shown in the option list. The injected
    /// "Other" option uses the sentinel label [`OTHER_OPTION_LABEL`]
    /// (TS `__other__`) — the answer-build logic detects this and
    /// substitutes the user's typed `notes` for the label.
    pub label: String,
    /// Longer explanation rendered under the label.
    pub description: String,
    /// Optional preview content (Markdown / monospace) shown side-by-side
    /// when this option is focused. `None` for plain options.
    pub preview: Option<String>,
}

/// Sentinel label injected as the last option of every question so the
/// user can type a free-form answer instead of picking. Mirrors TS
/// `QuestionView.tsx:85` `value === "__other__"`.
pub const OTHER_OPTION_LABEL: &str = "__other__";

/// Visible label used by the renderer when displaying the "Other"
/// sentinel — TS shows "Other" in the dropdown.
pub const OTHER_OPTION_DISPLAY: &str = "Other";

impl QuestionOverlay {
    /// Build the "Chat about this" rejection-feedback prose.
    ///
    /// Byte-for-byte mirror of TS `handleRespondToClaude` at
    /// `claude-code/src/components/permissions/AskUserQuestionPermissionRequest/AskUserQuestionPermissionRequest.tsx:300-316`.
    /// The leading-whitespace lines are intentional — TS uses an
    /// indented template literal and ships the literal indentation.
    pub fn chat_about_this_feedback(&self) -> String {
        let questions_with_answers = self.format_questions_with_answers(/*concise=*/ false);
        format!(
            "The user wants to clarify these questions.\n    \
             This means they may have additional information, context or questions for you.\n    \
             Take their response into account and then reformulate the questions if appropriate.\n    \
             Start by asking them what they would like to clarify.\n\n    \
             Questions asked:\n{questions_with_answers}"
        )
    }

    /// Build the "Skip interview and plan immediately" rejection-feedback
    /// prose. Byte-for-byte mirror of TS `handleFinishPlanInterview`
    /// (`AskUserQuestionPermissionRequest.tsx:340-356`). Caller is
    /// responsible for gating on `is_in_plan_mode` — this fn is pure.
    pub fn skip_interview_feedback(&self) -> String {
        let questions_with_answers = self.format_questions_with_answers(/*concise=*/ false);
        format!(
            "The user has indicated they have provided enough answers for the plan interview.\n\
             Stop asking clarifying questions and proceed to finish the plan with the information you have.\n\n\
             Questions asked and answers provided:\n{questions_with_answers}"
        )
    }

    /// Helper used by both feedback builders. TS source has identical
    /// loop bodies in both handlers — extracted here to keep the prose
    /// constants the only place that diverges.
    fn format_questions_with_answers(&self, _concise: bool) -> String {
        self.questions
            .iter()
            .map(|q| {
                let answer = self.peek_answer_for(q);
                if answer.is_empty() {
                    format!("- \"{}\"\n  (No answer provided)", q.question)
                } else {
                    format!("- \"{}\"\n  Answer: {}", q.question, answer)
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Peek the would-be answer for `q` without committing. Used by the
    /// feedback synthesizers — they show what the user partially answered
    /// before deciding to bail out via Chat-about-this / Skip-interview.
    /// Single-select picks the focused option label (or the typed `notes`
    /// when "Other" is focused); multi-select joins all checked labels.
    fn peek_answer_for(&self, q: &QuestionItem) -> String {
        let label_for = |idx: i32| -> Option<&str> {
            let opt = q.options.get(idx as usize)?;
            if opt.label == OTHER_OPTION_LABEL {
                let trimmed = q.notes.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                }
            } else {
                Some(opt.label.as_str())
            }
        };
        if q.multi_select && !q.checked.is_empty() {
            q.checked
                .iter()
                .filter_map(|i| label_for(*i))
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            label_for(q.selected).unwrap_or("").to_string()
        }
    }
}

#[cfg(test)]
#[path = "overlay.test.rs"]
mod overlay_tests;

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

/// `/memory` file-picker overlay state. Built from the
/// `TuiOnlyEvent::OpenMemoryDialog` payload; entries are pre-resolved
/// paths plus a label and scope tag. Selection is a simple index — there
/// is no filter (the entry count is small and fixed per session).
#[derive(Debug, Clone)]
pub struct MemoryDialogOverlay {
    pub entries: Vec<MemoryDialogEntry>,
    pub selected: i32,
}

/// A single row in the memory picker — TUI-side mirror of
/// `coco_types::MemoryDialogEntry` so the overlay struct stays free of
/// the coco-types dependency at the field level.
#[derive(Debug, Clone)]
pub struct MemoryDialogEntry {
    pub path: std::path::PathBuf,
    pub label: String,
    pub scope: MemoryDialogScope,
}

/// Scope tag for a memory file picker entry. Mirrors
/// `coco_types::MemoryDialogScope`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryDialogScope {
    Managed,
    User,
    Project,
    ProjectLocal,
    Subdir,
}

impl MemoryDialogScope {
    /// Build from the wire payload variant.
    pub fn from_wire(s: coco_types::MemoryDialogScope) -> Self {
        match s {
            coco_types::MemoryDialogScope::Managed => Self::Managed,
            coco_types::MemoryDialogScope::User => Self::User,
            coco_types::MemoryDialogScope::Project => Self::Project,
            coco_types::MemoryDialogScope::ProjectLocal => Self::ProjectLocal,
            coco_types::MemoryDialogScope::Subdir => Self::Subdir,
        }
    }
}

impl MemoryDialogOverlay {
    /// Build from the wire payload (`TuiOnlyEvent::OpenMemoryDialog`).
    pub fn from_wire(entries: Vec<coco_types::MemoryDialogEntry>) -> Self {
        Self {
            entries: entries
                .into_iter()
                .map(|e| MemoryDialogEntry {
                    path: std::path::PathBuf::from(e.path),
                    label: e.label,
                    scope: MemoryDialogScope::from_wire(e.scope),
                })
                .collect(),
            selected: 0,
        }
    }
}

/// Transcript overlay — verbose, all-messages dump for `Ctrl+O`.
///
/// Mirrors TS `screen === 'transcript'` (`screens/REPL.tsx:4392`)
/// minus the alt-screen virtual-scroll + search-bar layer (those
/// require ratatui infra coco-rs hasn't built; can be added as a
/// follow-up). Read-only: scroll position is the only state.
///
/// `show_all` mirrors TS `showAllInTranscript` — when true, hidden
/// meta messages (`isVisibleInTranscriptOnly`) are included in the
/// dump. Toggled inside the overlay via `transcript:toggleShowAll`
/// → [`crate::events::TuiCommand::ToggleTranscriptShowAll`] →
/// [`crate::update::transcript::toggle_show_all`]. coco-rs defaults
/// to `true` because the popup is opened explicitly to inspect; TS
/// defaults to `false` because its full-screen takeover doubles as a
/// reading mode where meta noise would distract.
#[derive(Debug, Clone, Default)]
pub struct TranscriptOverlay {
    /// Vertical scroll offset (line-count from top). Reset to 0 on
    /// open so the dump starts from the first message.
    pub scroll: i32,
    /// Whether to render meta / system-reminder messages too. TS:
    /// `showAllInTranscript`. Default `true` since the whole point of
    /// transcript mode is "show me everything".
    pub show_all: bool,
}

impl TranscriptOverlay {
    /// Open with default state — scrolled to top, `show_all` on.
    pub fn new() -> Self {
        Self {
            scroll: 0,
            show_all: true,
        }
    }
}
