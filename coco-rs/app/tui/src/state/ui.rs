//! UI state — local TUI state, never sent to the agent.

use std::collections::HashSet;
use std::collections::VecDeque;
use std::time::Duration;
use std::time::Instant;

use crate::constants;
use crate::theme::Theme;

/// UI-only local state.
#[derive(Debug)]
pub struct UiState {
    /// Multi-line input state.
    pub input: InputState,
    /// Paste pill manager for tracking pasted content (text and images).
    pub paste_manager: crate::paste::PasteManager,
    /// Chat scroll offset (lines from bottom).
    pub scroll_offset: i32,
    /// Current focus target.
    pub focus: FocusTarget,
    /// Active modal overlay.
    pub overlay: Option<Overlay>,
    /// Queued overlays awaiting display.
    pub overlay_queue: VecDeque<Overlay>,
    /// Active streaming content.
    pub streaming: Option<StreamingState>,
    /// Whether thinking content is visible.
    pub show_thinking: bool,
    /// Whether system reminders are visible (debug).
    pub show_system_reminders: bool,
    /// Whether user has manually scrolled.
    pub user_scrolled: bool,
    /// Current theme.
    pub theme: Theme,
    /// Active toast notifications.
    pub toasts: VecDeque<Toast>,
    /// IDs of collapsed tool calls.
    pub collapsed_tools: HashSet<String>,
    /// Help overlay scroll position.
    pub help_scroll: i32,
    /// Kill ring for Ctrl+K / Ctrl+Y.
    pub kill_ring: String,
    /// Timestamp of last Esc press (for double-Esc rewind detection).
    pub last_esc_time: Option<Instant>,
}

impl UiState {
    /// Create a new default UI state.
    pub fn new() -> Self {
        Self {
            input: InputState::new(),
            paste_manager: crate::paste::PasteManager::new(),
            scroll_offset: 0,
            focus: FocusTarget::Input,
            overlay: None,
            overlay_queue: VecDeque::new(),
            streaming: None,
            show_thinking: true,
            show_system_reminders: false,
            user_scrolled: false,
            theme: Theme::default(),
            toasts: VecDeque::new(),
            collapsed_tools: HashSet::new(),
            help_scroll: 0,
            kill_ring: String::new(),
            last_esc_time: None,
        }
    }

    /// Set the active overlay, queueing if one is already active.
    pub fn set_overlay(&mut self, overlay: Overlay) {
        if self.overlay.is_some() {
            if self.overlay_queue.len() < constants::MAX_OVERLAY_QUEUE as usize {
                self.overlay_queue.push_back(overlay);
            }
        } else {
            self.overlay = Some(overlay);
        }
    }

    /// Dismiss the current overlay and show the next queued one.
    pub fn dismiss_overlay(&mut self) {
        self.overlay = self.overlay_queue.pop_front();
    }

    /// Whether there are active toasts.
    pub fn has_toasts(&self) -> bool {
        !self.toasts.is_empty()
    }

    /// Add a toast notification.
    pub fn add_toast(&mut self, toast: Toast) {
        if self.toasts.len() >= constants::MAX_TOASTS as usize {
            self.toasts.pop_front();
        }
        self.toasts.push_back(toast);
    }

    /// Remove expired toasts.
    pub fn expire_toasts(&mut self) {
        self.toasts.retain(|t| !t.is_expired());
    }
}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}

/// Current focus target in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FocusTarget {
    #[default]
    Input,
    Chat,
}

/// Multi-line input state.
#[derive(Debug)]
pub struct InputState {
    /// Current text content.
    pub text: String,
    /// Cursor position (character index, NOT byte).
    pub cursor: i32,
    /// Command history.
    pub history: Vec<String>,
    /// Current history navigation index.
    pub history_index: Option<i32>,
}

impl InputState {
    /// Create empty input.
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_index: None,
        }
    }

    /// Insert a character at cursor.
    pub fn insert_char(&mut self, c: char) {
        let byte_pos = self.char_to_byte(self.cursor);
        self.text.insert(byte_pos, c);
        self.cursor += 1;
    }

    /// Delete character before cursor.
    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            let byte_pos = self.char_to_byte(self.cursor);
            let next_byte = self.char_to_byte(self.cursor + 1);
            self.text.replace_range(byte_pos..next_byte, "");
        }
    }

    /// Delete character at cursor.
    pub fn delete_forward(&mut self) {
        let len = self.text.chars().count() as i32;
        if self.cursor < len {
            let byte_pos = self.char_to_byte(self.cursor);
            let next_byte = self.char_to_byte(self.cursor + 1);
            self.text.replace_range(byte_pos..next_byte, "");
        }
    }

    /// Take the current input, clearing the buffer.
    pub fn take_input(&mut self) -> String {
        self.cursor = 0;
        self.history_index = None;
        std::mem::take(&mut self.text)
    }

    /// Move cursor left.
    pub fn cursor_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    /// Move cursor right.
    pub fn cursor_right(&mut self) {
        let len = self.text.chars().count() as i32;
        if self.cursor < len {
            self.cursor += 1;
        }
    }

    /// Move cursor to start of line.
    pub fn cursor_home(&mut self) {
        self.cursor = 0;
    }

    /// Move cursor to end of line.
    pub fn cursor_end(&mut self) {
        self.cursor = self.text.chars().count() as i32;
    }

    /// Whether the input is empty.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Add text to history.
    pub fn add_to_history(&mut self, text: String) {
        if !text.is_empty() {
            // Remove duplicate if exists
            self.history.retain(|h| h != &text);
            self.history.push(text);
            if self.history.len() > constants::MAX_HISTORY_ENTRIES as usize {
                self.history.remove(0);
            }
        }
    }

    /// Convert character index to byte index.
    fn char_to_byte(&self, char_idx: i32) -> usize {
        self.text
            .char_indices()
            .nth(char_idx as usize)
            .map(|(i, _)| i)
            .unwrap_or(self.text.len())
    }
}

impl Default for InputState {
    fn default() -> Self {
        Self::new()
    }
}

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
#[derive(Debug, Clone)]
pub struct PlanExitOverlay {
    pub plan_content: Option<String>,
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

/// Streaming display state.
#[derive(Debug, Clone)]
pub struct StreamingState {
    /// Accumulated text content.
    pub content: String,
    /// Accumulated thinking content.
    pub thinking: String,
    /// Current streaming mode.
    pub mode: StreamMode,
    /// Display cursor position for adaptive pacing.
    pub display_cursor: usize,
}

/// Current streaming content type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamMode {
    Text,
    Thinking,
    ToolUse,
}

impl StreamingState {
    /// Create a new streaming state.
    pub fn new() -> Self {
        Self {
            content: String::new(),
            thinking: String::new(),
            mode: StreamMode::Text,
            display_cursor: 0,
        }
    }

    /// Append text delta.
    pub fn append_text(&mut self, delta: &str) {
        self.content.push_str(delta);
        self.mode = StreamMode::Text;
    }

    /// Append thinking delta.
    pub fn append_thinking(&mut self, delta: &str) {
        self.thinking.push_str(delta);
        self.mode = StreamMode::Thinking;
    }

    /// Get visible content up to display cursor.
    pub fn visible_content(&self) -> &str {
        let end = self.display_cursor.min(self.content.len());
        &self.content[..end]
    }

    /// Advance display cursor (returns true if changed).
    pub fn advance_display(&mut self) -> bool {
        if self.display_cursor < self.content.len() {
            // Advance by one line or to end
            let remaining = &self.content[self.display_cursor..];
            let advance = remaining
                .find('\n')
                .map(|i| i + 1)
                .unwrap_or(remaining.len());
            self.display_cursor += advance;
            true
        } else {
            false
        }
    }

    /// Reveal all content immediately.
    pub fn reveal_all(&mut self) {
        self.display_cursor = self.content.len();
    }
}

impl Default for StreamingState {
    fn default() -> Self {
        Self::new()
    }
}

/// Toast notification.
#[derive(Debug, Clone)]
pub struct Toast {
    pub message: String,
    pub severity: ToastSeverity,
    pub created_at: Instant,
    pub duration: Duration,
}

/// Toast severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastSeverity {
    Info,
    Success,
    Warning,
    Error,
}

impl Toast {
    /// Create an info toast.
    pub fn info(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            severity: ToastSeverity::Info,
            created_at: Instant::now(),
            duration: constants::TOAST_INFO_DURATION,
        }
    }

    /// Create a success toast.
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            severity: ToastSeverity::Success,
            created_at: Instant::now(),
            duration: constants::TOAST_SUCCESS_DURATION,
        }
    }

    /// Create a warning toast.
    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            severity: ToastSeverity::Warning,
            created_at: Instant::now(),
            duration: constants::TOAST_WARNING_DURATION,
        }
    }

    /// Create an error toast.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            severity: ToastSeverity::Error,
            created_at: Instant::now(),
            duration: constants::TOAST_ERROR_DURATION,
        }
    }

    /// Whether the toast has expired.
    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed() >= self.duration
    }

    /// Remaining percentage (1.0 = full, 0.0 = expired).
    pub fn remaining_pct(&self) -> f64 {
        let elapsed = self.created_at.elapsed().as_secs_f64();
        let total = self.duration.as_secs_f64();
        (1.0 - elapsed / total).max(0.0)
    }
}
