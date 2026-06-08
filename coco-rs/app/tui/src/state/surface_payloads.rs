//! Prompt and modal payload types.
//!
//! The active surface state lives in `InteractionPaneState` and `ModalState`.
//! This module keeps the concrete payload structs shared by those enums and
//! their render/update code.

use super::session::ProviderUnavailableReason;

/// Permission approval state with tool-specific detail.
///
/// TS: src/components/permissions/ (51 files, 12K LOC)
/// Each tool type has a specialized review UI.
#[derive(Debug, Clone)]
pub struct PermissionPromptState {
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
    /// choice-list mode (mirrors `QuestionPromptState`): Up/Down moves
    /// `selected_choice`, Enter (approve) echoes the picked value back
    /// to the tool via `UserCommand::ApprovalResponse.updated_input`.
    ///
    /// TS parity: `ExitPlanModePermissionRequest.tsx:691-704` option
    /// grid, gated on `settings.showClearContextOnPlanAccept`.
    pub choices: Option<Vec<coco_types::PermissionAskChoice>>,
    /// Cursor position within `choices`, or within the classic
    /// approve / always-allow / deny action list when `choices.is_none()`.
    pub selected_choice: usize,
    /// Bounded, sanitized display projection of the tool input.
    /// Raw input must stay in `original_input` and must not be read by
    /// presentation code.
    pub display_input: coco_types::PermissionDisplayInput,
    /// Raw tool input captured at dialog-open time. Choice dialogs splice
    /// `user_choice` into it; classic read dialogs use it to build
    /// path-scoped "always allow" updates.
    pub original_input: Option<serde_json::Value>,
    /// Permission updates suggested by core for "always allow".
    /// Prefer these over UI-side inference.
    pub permission_suggestions: Vec<coco_types::PermissionUpdate>,
    /// Identity badge of the requesting cross-process teammate, if any.
    /// Rendered in the prompt title so the leader sees who is asking;
    /// `None` for the leader's own in-process requests.
    pub worker_badge: Option<coco_types::WorkerBadge>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PermissionAction {
    ApproveOnce,
    AlwaysAllow,
    Deny,
}

impl PermissionPromptState {
    pub(crate) fn classic_action_count(&self) -> usize {
        if self.show_always_allow { 3 } else { 2 }
    }

    pub(crate) fn classic_action_at(&self, index: usize) -> PermissionAction {
        match (self.show_always_allow, index) {
            (_, 0) => PermissionAction::ApproveOnce,
            (true, 1) => PermissionAction::AlwaysAllow,
            _ => PermissionAction::Deny,
        }
    }

    pub(crate) fn selected_classic_action(&self) -> PermissionAction {
        let index = self
            .selected_choice
            .min(self.classic_action_count().saturating_sub(1));
        self.classic_action_at(index)
    }
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

/// Plan mode exit state.
#[derive(Debug, Clone, Default)]
pub struct PlanExitPromptState {
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

/// Cost warning state.
#[derive(Debug, Clone)]
pub struct CostWarningPromptState {
    pub current_cost_cents: i64,
    pub threshold_cents: i64,
}

/// Model picker state — provider-grouped list of `(provider, model_id)`
/// candidates plus an inline thinking-effort selector. Tab cycles the
/// target role (Main / Fast / Plan / …); the confirm path persists
/// to that role's slot in `~/.coco.json::model_roles`.
///
/// TS parity reference: `components/ModelPicker.tsx`. coco-rs extends
/// the TS shape with a role pill so multi-provider users can configure
/// every `ModelRole` from the same surface — TS only ever drives the
/// `main` model.
#[derive(Debug, Clone)]
pub struct ModelPickerState {
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
    /// `default_effort` on every selection change (see `update::interaction`).
    /// `None` when the focused model declares no thinking levels.
    pub effort: Option<coco_types::ReasoningEffort>,
}

/// Teams roster picker — lets the leader cycle a teammate's permission
/// mode. TS parity: `components/teams/TeamsDialog.tsx` (per-member mode
/// cycling). Members come from `session.subagents` (kind == Teammate); each
/// member's CURRENT mode is seeded fresh from `team.json` so the picker shows
/// and cycles from the live mode (not a hardcoded default). Left/Right cycles
/// the focused member's mode in place; Enter dispatches it via
/// `UserCommand::SetTeammateMode`.
#[derive(Debug, Clone)]
pub struct TeamRosterState {
    /// Active team name (header).
    pub team_name: String,
    /// Running teammates, in roster order.
    pub members: Vec<TeamRosterMember>,
    /// Index of the focused teammate row.
    pub selected: usize,
}

/// One teammate row in the roster picker.
#[derive(Debug, Clone)]
pub struct TeamRosterMember {
    /// Display name (the `name` half of `name@team`), used as the
    /// `set_teammate_mode` target.
    pub name: String,
    /// Agent type label (e.g. `"explore"`), shown as a dim suffix.
    pub agent_type: String,
    /// Assigned palette color, if any.
    pub color: Option<coco_types::AgentColorName>,
    /// This teammate's CURRENT permission mode, seeded from `team.json` and
    /// cycled in place by Left/Right when focused. Per-member so divergent
    /// teammate modes are each shown and edited correctly (a single shared
    /// field could not represent that). TS: `teammate.mode` per row.
    pub mode: coco_types::PermissionMode,
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

/// Session browser state (list of saved sessions).
#[derive(Debug, Clone)]
pub struct SessionBrowserState {
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

/// Question state (AskUserQuestion tool).
#[derive(Debug, Clone)]
pub struct QuestionPromptState {
    pub request_id: String,
    /// Original tool input dict, stored verbatim so the answer payload
    /// can re-emit fields the model supplied that the TUI doesn't render
    /// (e.g. `metadata.source`). Stored AND re-emitted because the
    /// splice protocol in `update/state.rs` rebuilds the input as
    /// `{...original_input, answers, annotations}` — dropping the
    /// `original_input` spread would silently strip those fields.
    pub original_input: serde_json::Value,
    pub questions: Vec<QuestionItem>,
    /// Currently visible question page, or the multi-question submit page.
    pub current_question: QuestionPage,
    /// Focus within the currently visible page. This never encodes which page
    /// is visible, so footer/submit focus can no longer fall back to question 0.
    pub focus_target: QuestionFocusTarget,
    /// Plan-mode gate for the Skip-interview footer item. Set from
    /// `state.session.permission_mode == PermissionMode::Plan` when the
    /// state is constructed.
    pub is_in_plan_mode: bool,
}

/// Page shown by the AskUserQuestion prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestionPage {
    Question(usize),
    Submit,
}

/// What the user is focused on inside the active question page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestionFocusTarget {
    QuestionOption(usize),
    OtherInput,
    QuestionFooter(QuestionFooterAction),
    SubmitAction(SubmitAction),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestionFooterAction {
    ChatAboutThis,
    SkipInterview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitAction {
    SubmitAnswers,
    Cancel,
}

/// One question in the AskUserQuestion state.
#[derive(Debug, Clone)]
pub struct QuestionItem {
    /// Short label rendered as a chip — e.g. "Auth method".
    pub header: String,
    /// Full question text — typically ends with "?".
    pub question: String,
    pub options: Vec<QuestionOption>,
    /// `true` allows checkbox-style multi-selection, `false` is radio.
    pub multi_select: bool,
    /// Committed option index for radio-style questions. `None` means the user
    /// has focused a row but has not pressed Enter / a number shortcut yet.
    pub selected: Option<usize>,
    /// Indices toggled on for multi-select. Empty in single-select mode
    /// (the Enter handler then falls back to `selected`).
    pub checked: Vec<usize>,
    /// Free-form answer state rendered as the final `Type something.` row.
    pub other_input: OtherInputState,
}

impl QuestionItem {
    /// True when typed characters should edit [`Self::other_input`].
    pub fn is_editing(&self) -> bool {
        self.other_input.focused
    }
}

#[derive(Debug, Clone, Default)]
pub struct OtherInputState {
    pub focused: bool,
    pub value: String,
    pub committed: bool,
}

/// One choice within a [`QuestionItem`].
#[derive(Debug, Clone)]
pub struct QuestionOption {
    /// 1-5 word label shown in the option list.
    pub label: String,
    /// Longer explanation rendered under the label.
    pub description: String,
    /// Optional preview content (Markdown / monospace) shown side-by-side
    /// when this option is focused. `None` for plain options.
    pub preview: Option<String>,
}

impl QuestionPromptState {
    pub(crate) fn current_question_index(&self) -> Option<usize> {
        let QuestionPage::Question(idx) = self.current_question else {
            return None;
        };
        (idx < self.questions.len()).then_some(idx)
    }

    pub(crate) fn current_question_item(&self) -> Option<&QuestionItem> {
        self.current_question_index()
            .and_then(|idx| self.questions.get(idx))
    }

    pub(crate) fn current_question_item_mut(&mut self) -> Option<&mut QuestionItem> {
        let idx = self.current_question_index()?;
        self.questions.get_mut(idx)
    }

    pub(crate) fn set_question_page(&mut self, idx: usize) {
        if self.questions.is_empty() {
            return;
        }
        let idx = idx.min(self.questions.len() - 1);
        self.current_question = QuestionPage::Question(idx);
        self.focus_target = if self.questions[idx].options.is_empty() {
            QuestionFocusTarget::OtherInput
        } else {
            QuestionFocusTarget::QuestionOption(
                self.questions[idx]
                    .selected
                    .unwrap_or(0)
                    .min(self.questions[idx].options.len().saturating_sub(1)),
            )
        };
        self.sync_other_focus();
    }

    pub(crate) fn set_submit_page(&mut self) {
        self.current_question = QuestionPage::Submit;
        self.focus_target = QuestionFocusTarget::SubmitAction(SubmitAction::SubmitAnswers);
        self.sync_other_focus();
    }

    pub(crate) fn advance_question_or_submit(&mut self) -> bool {
        let Some(idx) = self.current_question_index() else {
            return false;
        };
        if idx + 1 < self.questions.len() {
            self.set_question_page(idx + 1);
            return true;
        }
        if self.questions.len() > 1 {
            self.set_submit_page();
            return true;
        }
        false
    }

    pub(crate) fn switch_page(&mut self, delta: i32) {
        if self.questions.len() <= 1 {
            return;
        }
        let len = self.questions.len() + 1;
        let current = match self.current_question {
            QuestionPage::Question(idx) => idx.min(self.questions.len() - 1),
            QuestionPage::Submit => self.questions.len(),
        };
        let next = (current as i32 + delta).rem_euclid(len as i32) as usize;
        if next == self.questions.len() {
            self.set_submit_page();
        } else {
            self.set_question_page(next);
        }
    }

    pub(crate) fn cycle_focus(&mut self, delta: i32) {
        use QuestionFocusTarget as Target;
        use QuestionFooterAction as Footer;
        use QuestionPage as Page;
        use SubmitAction as Action;

        let order = match self.current_question {
            Page::Question(idx) => {
                let Some(item) = self.questions.get(idx) else {
                    return;
                };
                let mut order: Vec<Target> = (0..item.options.len())
                    .map(Target::QuestionOption)
                    .collect();
                order.push(Target::OtherInput);
                order.push(Target::QuestionFooter(Footer::ChatAboutThis));
                if self.is_in_plan_mode {
                    order.push(Target::QuestionFooter(Footer::SkipInterview));
                }
                order
            }
            Page::Submit => vec![
                Target::SubmitAction(Action::SubmitAnswers),
                Target::SubmitAction(Action::Cancel),
            ],
        };
        if order.is_empty() {
            return;
        }
        let idx = order
            .iter()
            .position(|target| *target == self.focus_target)
            .unwrap_or(0) as i32;
        self.focus_target = order[(idx + delta).rem_euclid(order.len() as i32) as usize];
        self.sync_other_focus();
    }

    pub(crate) fn commit_focused_answer(&mut self) {
        let QuestionPage::Question(qidx) = self.current_question else {
            return;
        };
        let Some(item) = self.questions.get_mut(qidx) else {
            return;
        };
        match self.focus_target {
            QuestionFocusTarget::QuestionOption(oidx)
                if !item.multi_select && oidx < item.options.len() =>
            {
                item.selected = Some(oidx);
                item.other_input.committed = false;
            }
            QuestionFocusTarget::OtherInput if !item.other_input.value.trim().is_empty() => {
                item.other_input.committed = true;
                if !item.multi_select {
                    item.selected = None;
                }
            }
            _ => {}
        }
    }

    pub(crate) fn sync_other_focus(&mut self) {
        let focused_idx = match (self.current_question, self.focus_target) {
            (QuestionPage::Question(idx), QuestionFocusTarget::OtherInput) => Some(idx),
            _ => None,
        };
        for (idx, item) in self.questions.iter_mut().enumerate() {
            item.other_input.focused = focused_idx == Some(idx);
        }
    }

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
                let answer = self.answer_for(q, /*include_uncommitted_other=*/ true);
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
    /// Single-select picks the selected option label unless the free-text input
    /// is focused with content; multi-select joins checked labels plus typed
    /// free text when present.
    pub(crate) fn committed_answer_for(&self, q: &QuestionItem) -> String {
        self.answer_for(q, /*include_uncommitted_other=*/ false)
    }

    fn answer_for(&self, q: &QuestionItem, include_uncommitted_other: bool) -> String {
        let typed = q.other_input.value.trim();
        if include_uncommitted_other && q.other_input.focused {
            return typed.to_string();
        }
        if include_uncommitted_other && !typed.is_empty() {
            return typed.to_string();
        }
        if q.other_input.committed && !typed.is_empty() {
            return typed.to_string();
        }

        let label_for = |idx: usize| -> Option<&str> {
            let opt = q.options.get(idx)?;
            Some(opt.label.as_str())
        };
        if q.multi_select {
            let mut labels = q
                .checked
                .iter()
                .filter_map(|i| label_for(*i))
                .map(str::to_string)
                .collect::<Vec<_>>();
            if (q.other_input.committed || include_uncommitted_other) && !typed.is_empty() {
                labels.push(typed.to_string());
            }
            labels.join(", ")
        } else {
            q.selected.and_then(label_for).unwrap_or("").to_string()
        }
    }

    /// Whether `q` currently resolves to a non-empty answer. Drives the
    /// multi-question nav strip's ☒/☐ checkbox (TS `figures.checkboxOn/Off`
    /// keyed on `answers[q.question]`). Single-select questions pre-select the
    /// first option, so they read as answered unless "Other" is focused with an
    /// empty buffer; multi-select reads unanswered until something is checked.
    pub(crate) fn question_has_answer(&self, q: &QuestionItem) -> bool {
        !self.committed_answer_for(q).trim().is_empty()
    }

    /// True when every question resolves to an answer — drives the Submit tab's
    /// ✔/☐ marker and the "ready to submit" hint.
    pub(crate) fn all_answered(&self) -> bool {
        self.questions.iter().all(|q| self.question_has_answer(q))
    }
}

#[cfg(test)]
#[path = "surface_payloads.test.rs"]
mod surface_payload_tests;

/// Sandbox permission state.
#[derive(Debug, Clone)]
pub struct SandboxPermissionPromptState {
    pub request_id: String,
    pub description: String,
}

/// Plan mode entry state.
#[derive(Debug, Clone)]
pub struct PlanEntryPromptState {
    pub description: String,
}

/// Global search state (ripgrep streaming).
#[derive(Debug, Clone)]
pub struct GlobalSearchState {
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

/// Quick file open state.
#[derive(Debug, Clone)]
pub struct QuickOpenState {
    pub filter: String,
    pub files: Vec<String>,
    pub selected: i32,
}

/// `/copy` picker state. The picker is mounted when the chosen
/// assistant message contains code blocks AND the user has not opted
/// into "always copy full response" via config — TS mirror at
/// `commands/copy/copy.tsx::CopyPicker`.
#[derive(Debug, Clone)]
pub struct CopyPickerState {
    /// The full markdown source of the picked assistant message.
    pub full_text: String,
    /// Fenced code blocks extracted from `full_text`. Empty when the
    /// user opens the picker via the "always" path; otherwise at least
    /// one entry (the no-blocks case skips the picker entirely).
    pub code_blocks: Vec<CopyPickerCodeBlock>,
    /// 0 = latest, 1 = second-to-latest, …  Surfaced in the picker
    /// header so the user knows which turn they're copying.
    pub message_age: usize,
    /// Currently highlighted option.
    pub selected: CopyPickerSelection,
}

/// Identity of a code block inside the picker — owned copy so the
/// picker survives transcript mutation while it's open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyPickerCodeBlock {
    pub code: String,
    pub lang: Option<String>,
}

/// What the picker currently has selected. `Always` is the trailing
/// "Always copy full response" option that flips the
/// `copy_full_response` setting on confirm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyPickerSelection {
    Full,
    CodeBlock(usize),
    Always,
}

impl CopyPickerState {
    pub fn option_count(&self) -> usize {
        // Full + each code block + Always
        2 + self.code_blocks.len()
    }

    pub fn move_up(&mut self) {
        self.selected = match self.selected {
            CopyPickerSelection::Full if self.code_blocks.is_empty() => CopyPickerSelection::Always,
            CopyPickerSelection::Full => CopyPickerSelection::Always,
            CopyPickerSelection::CodeBlock(0) => CopyPickerSelection::Full,
            CopyPickerSelection::CodeBlock(n) => CopyPickerSelection::CodeBlock(n - 1),
            CopyPickerSelection::Always if self.code_blocks.is_empty() => CopyPickerSelection::Full,
            CopyPickerSelection::Always => {
                CopyPickerSelection::CodeBlock(self.code_blocks.len() - 1)
            }
        };
    }

    pub fn move_down(&mut self) {
        self.selected = match self.selected {
            CopyPickerSelection::Full if self.code_blocks.is_empty() => CopyPickerSelection::Always,
            CopyPickerSelection::Full => CopyPickerSelection::CodeBlock(0),
            CopyPickerSelection::CodeBlock(n) if n + 1 < self.code_blocks.len() => {
                CopyPickerSelection::CodeBlock(n + 1)
            }
            CopyPickerSelection::CodeBlock(_) => CopyPickerSelection::Always,
            CopyPickerSelection::Always => CopyPickerSelection::Full,
        };
    }
}

/// Export dialog state.
#[derive(Debug, Clone)]
pub struct ExportState {
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

/// Full-screen diff view state.
#[derive(Debug, Clone)]
pub struct DiffViewState {
    pub path: String,
    pub diff: String,
    pub scroll: i32,
}

/// MCP server approval state.
#[derive(Debug, Clone)]
pub struct McpServerApprovalPromptState {
    pub server_name: String,
    pub server_url: Option<String>,
    pub tools: Vec<String>,
    pub request_id: String,
}

/// Worktree exit confirmation state.
#[derive(Debug, Clone)]
pub struct WorktreeExitState {
    pub branch: String,
    pub has_uncommitted: bool,
    pub changed_files: Vec<String>,
}

/// Doctor/diagnostics state.
#[derive(Debug, Clone)]
pub struct DoctorState {
    pub checks: Vec<DoctorCheck>,
}

/// A single doctor check result.
#[derive(Debug, Clone)]
pub struct DoctorCheck {
    pub name: String,
    pub passed: bool,
    pub message: String,
}

/// Bridge dialog state (IDE/REPL).
#[derive(Debug, Clone)]
pub struct BridgeState {
    pub bridge_type: String,
    pub status: String,
    pub details: String,
}

/// Invalid config warning state.
#[derive(Debug, Clone)]
pub struct InvalidConfigState {
    pub errors: Vec<String>,
}

/// Idle return confirmation state.
#[derive(Debug, Clone)]
pub struct IdleReturnState {
    pub idle_duration_secs: i64,
}

/// Trust dialog state.
#[derive(Debug, Clone)]
pub struct TrustState {
    pub path: String,
    pub description: String,
}

/// Auto mode opt-in state.
#[derive(Debug, Clone)]
pub struct AutoModeOptInState {
    pub description: String,
}

/// Bypass permissions confirmation state.
#[derive(Debug, Clone)]
pub struct BypassPermissionsState {
    pub current_mode: String,
}

/// Plugin-hint recommendation dialog state.
///
/// Surfaced when a CLI/SDK emits a `<claude-code-hint />` tag referencing a
/// plugin and the pre-store gate passed. Show-once per plugin. The user
/// picks install / dismiss / disable-all.
///
/// TS: `PluginHintMenu.tsx` + `useClaudeCodeHintRecommendation.tsx`.
#[derive(Debug, Clone)]
pub struct PluginHintState {
    /// Fully-qualified plugin ID (`name@marketplace`).
    pub plugin_id: String,
    /// Human-readable plugin name.
    pub plugin_name: String,
    /// The marketplace that hosts the plugin.
    pub marketplace_name: String,
    /// Short description from the marketplace entry.
    pub plugin_description: Option<String>,
    /// First token of the command that emitted the hint.
    pub source_command: String,
    /// Selected option index: 0 = install, 1 = dismiss, 2 = disable-all.
    pub selected: i32,
}

impl PluginHintState {
    /// Number of selectable options.
    pub const OPTION_COUNT: i32 = 3;

    /// The response keyed by the current selection.
    pub fn selected_response(&self) -> PluginHintResponse {
        match self.selected {
            0 => PluginHintResponse::Install,
            2 => PluginHintResponse::Disable,
            _ => PluginHintResponse::Dismiss,
        }
    }
}

/// User decision on a plugin-hint dialog. TS: `'yes' | 'no' | 'disable'`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginHintResponse {
    /// Install the recommended plugin.
    Install,
    /// Dismiss without installing.
    Dismiss,
    /// Dismiss and never show plugin-install hints again.
    Disable,
}

/// Background task detail state.
#[derive(Debug, Clone)]
pub struct TaskDetailState {
    pub task_id: String,
    pub task_type: String,
    pub description: String,
    pub output: String,
    pub status: String,
    pub scroll: i32,
}

/// Feedback survey state.
#[derive(Debug, Clone)]
pub struct FeedbackState {
    pub prompt: String,
    pub options: Vec<String>,
    pub selected: i32,
}

/// MCP server multi-select state.
#[derive(Debug, Clone)]
pub struct McpServerSelectState {
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

/// Plan-approval state shown to the team lead when a teammate sends
/// a `plan_approval_request` via mailbox. The leader picks approve /
/// deny (+ optional feedback); the TUI dispatches
/// `UserCommand::PlanApprovalResponse` which the engine writes back to
/// the teammate's inbox.
///
/// TS source: `tools/ExitPlanModeTool/ExitPlanModeV2Tool.ts:137-141`
/// builds the request; leader side surfaces via an ink modal.
#[derive(Debug, Clone)]
pub struct PlanApprovalPromptState {
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

impl PlanApprovalPromptState {
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

/// `/memory` file-picker state state. Built from the
/// `TuiOnlyEvent::OpenMemoryDialog` payload; entries are pre-resolved
/// paths plus a label and scope tag. Selection is a simple index — there
/// is no filter (the entry count is small and fixed per session).
#[derive(Debug, Clone)]
pub struct MemoryDialogState {
    pub entries: Vec<MemoryDialogEntry>,
    pub selected: i32,
}

/// A single row in the memory picker — TUI-side mirror of
/// `coco_types::MemoryDialogEntry` so the state struct stays free of
/// the coco-types dependency at the field level.
#[derive(Debug, Clone)]
pub struct MemoryDialogEntry {
    pub path: std::path::PathBuf,
    pub label: String,
    pub scope: MemoryDialogScope,
    pub row_kind: MemoryDialogRowKind,
}

/// Scope tag for a memory file picker entry. Mirrors
/// `coco_types::MemoryDialogScope`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryDialogScope {
    Managed,
    User,
    Project,
    ProjectLocal,
    ProjectConfig,
    Subdir,
    Imported,
    AutoMemFolder,
    TeamMemFolder,
    AgentMemFolder,
}

impl MemoryDialogScope {
    /// Build from the wire payload variant.
    pub fn from_wire(s: coco_types::MemoryDialogScope) -> Self {
        match s {
            coco_types::MemoryDialogScope::Managed => Self::Managed,
            coco_types::MemoryDialogScope::User => Self::User,
            coco_types::MemoryDialogScope::Project => Self::Project,
            coco_types::MemoryDialogScope::ProjectLocal => Self::ProjectLocal,
            coco_types::MemoryDialogScope::ProjectConfig => Self::ProjectConfig,
            coco_types::MemoryDialogScope::Subdir => Self::Subdir,
            coco_types::MemoryDialogScope::Imported => Self::Imported,
            coco_types::MemoryDialogScope::AutoMemFolder => Self::AutoMemFolder,
            coco_types::MemoryDialogScope::TeamMemFolder => Self::TeamMemFolder,
            coco_types::MemoryDialogScope::AgentMemFolder => Self::AgentMemFolder,
        }
    }
}

/// Semantic row kind for memory picker entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryDialogRowKind {
    File { exists: bool, read_only: bool },
    Folder { enabled: bool },
    Toggle { enabled: bool },
}

impl MemoryDialogRowKind {
    pub fn from_wire(kind: coco_types::MemoryDialogRowKind) -> Self {
        match kind {
            coco_types::MemoryDialogRowKind::File { exists, read_only } => {
                Self::File { exists, read_only }
            }
            coco_types::MemoryDialogRowKind::Folder { enabled } => Self::Folder { enabled },
            coco_types::MemoryDialogRowKind::Toggle { enabled } => Self::Toggle { enabled },
        }
    }

    pub fn is_file(self) -> bool {
        matches!(self, Self::File { .. })
    }
}

impl MemoryDialogState {
    /// Build from the wire payload (`TuiOnlyEvent::OpenMemoryDialog`).
    pub fn from_wire(entries: Vec<coco_types::MemoryDialogEntry>) -> Self {
        Self {
            entries: entries
                .into_iter()
                .map(|e| MemoryDialogEntry {
                    path: std::path::PathBuf::from(e.path),
                    label: e.label,
                    scope: MemoryDialogScope::from_wire(e.scope),
                    row_kind: MemoryDialogRowKind::from_wire(e.row_kind),
                })
                .collect(),
            selected: 0,
        }
    }
}

/// `/skills` editable overlay state — flat list of [`SkillRow`]s
/// with filter + sort + selection state plus in-memory `pending`
/// override on each row. TS parity: 2.1.142 `uJ4` (`cli_inner_pretty
/// .js:476909`) — the read-only 2.1.88 grouped variant is retired.
#[derive(Debug, Clone)]
pub struct SkillsDialogState {
    /// All rows, stable insertion order (the renderer applies the
    /// current sort each frame; mutation order matters only for
    /// pending-state retention).
    pub rows: Vec<SkillRow>,
    /// Current filter query (lowercased on insert so the matcher
    /// can do byte-exact substring lookup). Empty = no filter.
    pub filter_query: String,
    /// Whether the inline filter input box is the active key
    /// target. `true` ⇒ printable characters append to the query;
    /// `false` ⇒ Space/Enter/Esc/`/`/`t` drive selection mode.
    pub filter_focused: bool,
    /// Whether the user toggled `t` to sort by descending token
    /// cost. Default (false) sorts by source-string lex + name.
    /// Not persisted — each `/skills` invocation starts at false.
    pub sort_by_tokens: bool,
    /// Index into the **filtered + sorted** view (not into
    /// [`Self::rows`]). The renderer recomputes the view each
    /// frame; this is clamped to `0..=view_len-1` on filter/sort
    /// change.
    pub selected_filtered_idx: usize,
    /// Bytes-per-token ratio for the token column. Comes from
    /// `SkillsDialogPayload.bytes_per_token`; the dialog divides
    /// [`SkillRow::frontmatter_bytes`] by this to render `~N tok`.
    pub bytes_per_token: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginDialogTab {
    Installed,
    Marketplaces,
    Errors,
}

impl PluginDialogTab {
    pub const ALL: [Self; 3] = [Self::Installed, Self::Marketplaces, Self::Errors];

    pub fn label(self) -> &'static str {
        match self {
            Self::Installed => "Installed",
            Self::Marketplaces => "Marketplaces",
            Self::Errors => "Errors",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Installed => Self::Marketplaces,
            Self::Marketplaces => Self::Errors,
            Self::Errors => Self::Installed,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Installed => Self::Errors,
            Self::Marketplaces => Self::Installed,
            Self::Errors => Self::Marketplaces,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PluginDialogState {
    pub installed: Vec<coco_types::PluginDialogInstalledRow>,
    pub marketplaces: Vec<coco_types::PluginDialogMarketplaceRow>,
    pub errors: Vec<coco_types::PluginDialogErrorRow>,
    pub selected_tab: PluginDialogTab,
    pub selected_idx: usize,
    pub filter_query: String,
    pub filter_focused: bool,
}

impl PluginDialogState {
    pub fn from_wire(payload: coco_types::PluginDialogPayload) -> Self {
        Self {
            installed: payload.installed,
            marketplaces: payload.marketplaces,
            errors: payload.errors,
            selected_tab: PluginDialogTab::Installed,
            selected_idx: 0,
            filter_query: String::new(),
            filter_focused: false,
        }
    }

    pub fn selected_len(&self) -> usize {
        match self.selected_tab {
            PluginDialogTab::Installed => self.filtered_installed_indices().len(),
            PluginDialogTab::Marketplaces => self.filtered_marketplace_indices().len(),
            PluginDialogTab::Errors => self.filtered_error_indices().len(),
        }
    }

    pub fn move_down(&mut self) {
        let len = self.selected_len();
        if len > 0 {
            self.selected_idx = (self.selected_idx + 1).min(len - 1);
        }
    }

    pub fn move_up(&mut self) {
        self.selected_idx = self.selected_idx.saturating_sub(1);
    }

    pub fn cycle_tab_next(&mut self) {
        self.selected_tab = self.selected_tab.next();
        self.selected_idx = 0;
    }

    pub fn cycle_tab_prev(&mut self) {
        self.selected_tab = self.selected_tab.prev();
        self.selected_idx = 0;
    }

    pub fn apply_filter_char(&mut self, c: char) {
        if c == '\n' || c == '\r' {
            return;
        }
        if c == '/' && self.filter_query.is_empty() {
            return;
        }
        self.filter_query.push(c.to_ascii_lowercase());
        self.selected_idx = 0;
    }

    pub fn backspace_filter(&mut self) -> bool {
        let changed = self.filter_query.pop().is_some();
        if changed {
            self.selected_idx = 0;
        }
        changed
    }

    pub fn clear_filter(&mut self) {
        self.filter_query.clear();
        self.filter_focused = false;
        self.selected_idx = 0;
    }

    pub fn filtered_installed_indices(&self) -> Vec<usize> {
        self.installed
            .iter()
            .enumerate()
            .filter_map(|(idx, row)| {
                self.matches_filter(&[&row.id, &row.name, row.description.as_deref().unwrap_or("")])
                    .then_some(idx)
            })
            .collect()
    }

    pub fn filtered_marketplace_indices(&self) -> Vec<usize> {
        self.marketplaces
            .iter()
            .enumerate()
            .filter_map(|(idx, row)| {
                self.matches_filter(&[&row.name, row.source.as_deref().unwrap_or("")])
                    .then_some(idx)
            })
            .collect()
    }

    pub fn filtered_error_indices(&self) -> Vec<usize> {
        self.errors
            .iter()
            .enumerate()
            .filter_map(|(idx, row)| {
                self.matches_filter(&[&row.plugin_id, &row.message])
                    .then_some(idx)
            })
            .collect()
    }

    pub fn focused_action(&self) -> Option<coco_types::PluginDialogAction> {
        match self.selected_tab {
            PluginDialogTab::Installed => {
                let idx = *self.filtered_installed_indices().get(self.selected_idx)?;
                self.installed.get(idx)?.actions.first().cloned()
            }
            PluginDialogTab::Marketplaces => {
                let idx = *self.filtered_marketplace_indices().get(self.selected_idx)?;
                self.marketplaces.get(idx)?.actions.first().cloned()
            }
            PluginDialogTab::Errors => None,
        }
    }

    fn matches_filter(&self, fields: &[&str]) -> bool {
        if self.filter_query.is_empty() {
            return true;
        }
        fields
            .iter()
            .any(|field| field.to_ascii_lowercase().contains(&self.filter_query))
    }
}

/// One row in the editable `/skills` dialog. Carries everything
/// the renderer + save algorithm need — no round-trip to the
/// handler.
#[derive(Debug, Clone)]
pub struct SkillRow {
    pub name: String,
    pub source: SkillsDialogSource,
    /// Pre-built source label in lowercase for the filter matcher
    /// (`/` search hits name OR description OR source label).
    pub source_label_lower: String,
    pub plugin_name: Option<String>,
    pub frontmatter_bytes: i64,
    /// Lowercase haystack `name \u{1} description \u{1} source_label`
    /// — pre-computed so the filter matcher is one `contains` call
    /// per row.
    pub search_haystack: String,
    /// Value in `<cwd>/.claude/settings.local.json` right now.
    /// `None` ⇒ key absent.
    pub current_local: Option<SkillOverrideState>,
    /// Project-or-user resolution (without local / policy / flag).
    /// What the dialog reverts to when the user clears their local
    /// override.
    pub baseline: SkillOverrideState,
    /// User's in-memory pending edit. Initialized from
    /// `lock.forced_value` if locked, else from `current_local ??
    /// baseline`. Mutates on Space (lock rows are no-op).
    pub pending: SkillOverrideState,
    /// Optional lock — when set, the row renders `🔒 <label>`
    /// and refuses to cycle. The lock's `forced_value` is also
    /// surfaced as `pending` so save-diff never tries to persist
    /// a different value.
    pub lock: Option<SkillLock>,
}

/// TUI-side mirror of `coco_types::SkillsDialogSource`. Pinned to
/// the state crate so [`crate::state::ModalState`] doesn't import
/// `coco-types` directly for this field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SkillsDialogSource {
    BuiltIn,
    Project,
    User,
    Policy,
    Plugin,
    Mcp,
}

impl SkillsDialogSource {
    pub fn from_wire(s: coco_types::SkillsDialogSource) -> Self {
        match s {
            coco_types::SkillsDialogSource::BuiltIn => Self::BuiltIn,
            coco_types::SkillsDialogSource::Project => Self::Project,
            coco_types::SkillsDialogSource::User => Self::User,
            coco_types::SkillsDialogSource::Policy => Self::Policy,
            coco_types::SkillsDialogSource::Plugin => Self::Plugin,
            coco_types::SkillsDialogSource::Mcp => Self::Mcp,
        }
    }

    /// Lowercased label used by the inline source column and the
    /// filter haystack. TS `xJ4` (`cli_inner_pretty.js:476897-
    /// 476907`) collapses `bundled`/`builtin` → display label
    /// `"built-in"`; the others use the snake-cased source name.
    pub fn label_lower(&self) -> &'static str {
        match self {
            Self::BuiltIn => "built-in",
            Self::Project => "project",
            Self::User => "user",
            Self::Policy => "policy",
            Self::Plugin => "plugin",
            Self::Mcp => "mcp",
        }
    }
}

/// Type alias for the wire skill-lock — keeps the state layer
/// free of `coco_types` imports outside this struct.
pub type SkillLock = coco_types::SkillLock;
pub use coco_types::SkillLockSource;
pub use coco_types::SkillOverrideState;

impl SkillsDialogState {
    /// Build from the wire payload. The renderer applies the
    /// 2.1.142 sort (source-string lex + name; or token desc when
    /// `sort_by_tokens` is on) each frame, so we don't pre-sort.
    pub fn from_wire(payload: coco_types::SkillsDialogPayload) -> Self {
        let rows = payload
            .entries
            .into_iter()
            .map(|e| {
                let source = SkillsDialogSource::from_wire(e.source);
                let source_label_lower = source.label_lower().to_string();
                // pending starts at lock-forced-value when locked,
                // else current_local ?? baseline. The dialog never
                // surfaces a different `pending` on a locked row.
                let pending = e
                    .lock
                    .as_ref()
                    .map(|l| l.forced_value)
                    .or(e.current_local)
                    .unwrap_or(e.baseline);
                let mut haystack = String::with_capacity(
                    e.name.len() + e.description.len() + source_label_lower.len() + 2,
                );
                haystack.push_str(&e.name.to_lowercase());
                haystack.push('\u{1}');
                haystack.push_str(&e.description.to_lowercase());
                haystack.push('\u{1}');
                haystack.push_str(&source_label_lower);
                SkillRow {
                    name: e.name,
                    source,
                    source_label_lower,
                    plugin_name: e.plugin_name,
                    frontmatter_bytes: e.frontmatter_bytes,
                    search_haystack: haystack,
                    current_local: e.current_local,
                    baseline: e.baseline,
                    pending,
                    lock: e.lock,
                }
            })
            .collect();
        Self {
            rows,
            filter_query: String::new(),
            filter_focused: false,
            sort_by_tokens: false,
            selected_filtered_idx: 0,
            // Defensive fallback if a producer sets 0 — the
            // ~4-bytes/token English rule-of-thumb keeps the token
            // column non-zero.
            bytes_per_token: if payload.bytes_per_token > 0 {
                payload.bytes_per_token
            } else {
                4
            },
        }
    }

    /// Total entry count (drives the `{N} skills` subtitle).
    pub fn total(&self) -> usize {
        self.rows.len()
    }

    /// Whether any row carries a plugin source — drives the
    /// "Plugin skills are managed via /plugin" footer.
    pub fn has_plugin_rows(&self) -> bool {
        self.rows
            .iter()
            .any(|r| r.source == SkillsDialogSource::Plugin)
    }

    /// Indices into [`Self::rows`] for the currently-filtered +
    /// sorted view. Recomputed every call; the dialog renderer is
    /// expected to call this once per frame.
    pub fn filtered_view(&self) -> Vec<usize> {
        let mut indices: Vec<usize> = if self.filter_query.is_empty() {
            (0..self.rows.len()).collect()
        } else {
            self.rows
                .iter()
                .enumerate()
                .filter(|(_, r)| r.search_haystack.contains(&self.filter_query))
                .map(|(i, _)| i)
                .collect()
        };
        if self.sort_by_tokens {
            indices.sort_by(|a, b| {
                self.rows[*b]
                    .frontmatter_bytes
                    .cmp(&self.rows[*a].frontmatter_bytes)
                    .then_with(|| self.rows[*a].name.cmp(&self.rows[*b].name))
            });
        } else {
            indices.sort_by(|a, b| {
                self.rows[*a]
                    .source_label_lower
                    .cmp(&self.rows[*b].source_label_lower)
                    .then_with(|| self.rows[*a].name.cmp(&self.rows[*b].name))
            });
        }
        indices
    }

    /// Resolve the currently-focused row index in [`Self::rows`].
    /// Returns `None` when the filtered view is empty.
    pub fn focused_row(&self) -> Option<usize> {
        let view = self.filtered_view();
        view.get(self.selected_filtered_idx).copied()
    }

    /// Cycle the focused row's `pending` state through the 4-state
    /// ladder. **No-op on locked rows** (TS `oT5`-locked rows
    /// silently swallow Space at `cli_inner_pretty.js:476984` —
    /// the cycle handler returns early before mutating state).
    pub fn cycle_focused(&mut self) {
        let Some(idx) = self.focused_row() else {
            return;
        };
        if self.rows[idx].lock.is_some() {
            return;
        }
        self.rows[idx].pending = self.rows[idx].pending.next();
    }

    /// Compute the diff to write to `localSettings.skill_overrides`.
    /// Mirrors TS `C` (`cli_inner_pretty.js:476991-477016`):
    ///
    /// - For each row, compare `pending` to `baseline`. If equal,
    ///   write `null` (delete the local key); else write `pending`.
    /// - Skip the row entirely when `pending` already matches the
    ///   on-disk local value (no-op).
    /// - Locked rows are skipped (their `pending` is forced and
    ///   never written by the dialog).
    pub fn compute_save_diff(&self) -> SaveDiff {
        let mut diff = std::collections::BTreeMap::new();
        let mut total_edits = 0usize;
        for row in &self.rows {
            if row.lock.is_some() {
                continue;
            }
            let value_to_write: Option<SkillOverrideState> = if row.pending == row.baseline {
                None
            } else {
                Some(row.pending)
            };
            let effective_before = row.current_local.unwrap_or(row.baseline);
            if row.pending != effective_before {
                total_edits += 1;
            }
            if value_to_write != row.current_local {
                diff.insert(row.name.clone(), value_to_write);
            }
        }
        SaveDiff { diff, total_edits }
    }

    /// Apply a single printable character to the filter query.
    /// Mirrors TS `cli_inner_pretty.js:477038-477045`: if the char
    /// is `/`, the literal slash is stripped (so typing `/` to
    /// enter filter mode doesn't push a literal `/` into the
    /// query). All other characters append.
    ///
    /// The caller should set `filter_focused = true` before calling
    /// this — the function itself only mutates the query string.
    pub fn apply_filter_char(&mut self, ch: char) {
        if ch == '/' {
            // Strip leading slash; if it's at the very start of an
            // empty query, this is the activation case and nothing
            // changes.
            return;
        }
        self.filter_query.push(ch.to_ascii_lowercase());
        self.clamp_selection();
    }

    /// Pop one character off the filter query. Returns whether
    /// the query was non-empty (TS swallows the keystroke when the
    /// query is empty so the dialog stays in select mode).
    pub fn backspace_filter(&mut self) -> bool {
        if self.filter_query.is_empty() {
            return false;
        }
        self.filter_query.pop();
        self.clamp_selection();
        true
    }

    /// Clear the filter query and exit filter focus.
    pub fn clear_filter(&mut self) {
        self.filter_query.clear();
        self.filter_focused = false;
        self.clamp_selection();
    }

    /// Toggle source-vs-token-cost sort. Mirrors TS `t` key
    /// (`cli_inner_pretty.js:477018-477023`). Resets the
    /// selection index because the view order changed under it.
    pub fn toggle_sort(&mut self) {
        self.sort_by_tokens = !self.sort_by_tokens;
        self.selected_filtered_idx = 0;
    }

    /// Move selection up by one within the filtered view. No-op
    /// when at the top (TS lets the list controller wrap, but
    /// the dialog itself doesn't — see `o6` dispatch).
    pub fn move_up(&mut self) {
        if self.selected_filtered_idx > 0 {
            self.selected_filtered_idx -= 1;
        }
    }

    /// Move selection down by one within the filtered view.
    pub fn move_down(&mut self) {
        let view_len = self.filtered_view().len();
        if view_len == 0 {
            self.selected_filtered_idx = 0;
            return;
        }
        if self.selected_filtered_idx + 1 < view_len {
            self.selected_filtered_idx += 1;
        }
    }

    /// Clamp the selected index into the current view length so a
    /// filter change doesn't leave the cursor pointing past the
    /// last row.
    fn clamp_selection(&mut self) {
        let view_len = self.filtered_view().len();
        if view_len == 0 {
            self.selected_filtered_idx = 0;
            return;
        }
        if self.selected_filtered_idx >= view_len {
            self.selected_filtered_idx = view_len - 1;
        }
    }
}

/// Glyph + label table for the dialog's per-row state column. TS
/// mirror: `rT5` (`cli_inner_pretty.js:477209-477214`).
///
/// Lives at the TUI state layer (not on `coco_types::SkillOverrideState`)
/// because the glyphs are a display concern — SDK consumers should
/// render their own table from the state enum.
pub fn skill_override_glyph_and_label(state: SkillOverrideState) -> (char, &'static str) {
    match state {
        SkillOverrideState::On => ('\u{2714}', "on"),
        SkillOverrideState::NameOnly => ('\u{2022}', "name-only"),
        SkillOverrideState::UserInvocableOnly => ('\u{25CB}', "user-only"),
        SkillOverrideState::Off => ('\u{2716}', "off"),
    }
}

/// Diff produced by [`SkillsDialogState::compute_save_diff`] —
/// directly serializable as the `skill_overrides` JSON patch the
/// SettingsWriter expects.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaveDiff {
    /// Keys to update in `localSettings.skill_overrides`. `Some` ⇒
    /// write the new state. `None` ⇒ delete the key (TS B6
    /// deletion sentinel). Empty map ⇒ no-op save.
    pub diff: std::collections::BTreeMap<String, Option<SkillOverrideState>>,
    /// Number of rows whose effective state changed (different from
    /// what was effective at dialog-open time). Drives the toast:
    /// `Updated N override(s)` vs `No changes`.
    pub total_edits: usize,
}

impl SaveDiff {
    /// Whether any keys would change on disk.
    pub fn has_disk_changes(&self) -> bool {
        !self.diff.is_empty()
    }

    /// Render the diff as a `serde_json::Value` patch ready for
    /// [`coco_config::SettingsWriter::write_local`]. Each `None`
    /// becomes JSON `null` (the writer's delete sentinel).
    pub fn to_settings_patch(&self) -> serde_json::Value {
        let mut overrides = serde_json::Map::new();
        for (name, value) in &self.diff {
            let v = match value {
                Some(s) => serde_json::to_value(s).unwrap_or(serde_json::Value::Null),
                None => serde_json::Value::Null,
            };
            overrides.insert(name.clone(), v);
        }
        serde_json::json!({ "skill_overrides": serde_json::Value::Object(overrides) })
    }
}

/// Standalone theme picker (TS `components/ThemePicker.tsx`). Opened by
/// `/theme`; navigating live-previews the focused theme via an in-memory
/// `apply_theme_setting`, Enter persists, Esc restores `original_setting` so
/// the preview never sticks.
#[derive(Debug, Clone)]
pub struct ThemePickerState {
    /// Auto + every built-in / custom theme, in display order.
    pub choices: Vec<crate::theme::ThemeChoice>,
    /// Index into `choices` (clamped to range by the renderer / nav).
    pub selected: i32,
    /// Theme setting active when the picker opened, restored on cancel.
    pub original_setting: crate::theme::ThemeSetting,
}
