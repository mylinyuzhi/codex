//! UI state — local TUI state, never sent to the agent.
//!
//! Overlay types live in `state::overlay`. This file keeps only the UiState
//! plus the pieces tightly coupled to it: input + history, focus, streaming,
//! and toasts.

use std::collections::HashSet;
use std::collections::VecDeque;
use std::time::Duration;
use std::time::Instant;

use crate::constants;
use crate::display_settings::DisplaySettings;
use crate::double_press::DoublePressTracker;
use crate::keybinding_resolver::KeybindingHandle;
use crate::state::overlay::Overlay;
use crate::theme::Theme;
use crate::theme::ThemeRuntimeState;
use crate::theme::ThemeSetting;
use crate::widgets::suggestion_popup::SuggestionItem;

/// Exit keys subject to double-press confirmation. Mirrors TS
/// `ExitState::keyName` (`hooks/useExitOnCtrlCD.ts`).
///
/// The variant labels (`"Ctrl-C"` / `"Ctrl-D"`) match the TS string
/// values verbatim so footer copy and i18n substitution line up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitKey {
    /// Ctrl+C — interrupt the running task on the first press and exit
    /// on a second press within the window.
    CtrlC,
    /// Ctrl+D — arm-only on the first press, exit on the second.
    CtrlD,
}

impl ExitKey {
    /// Human-readable label used in the "Press X again to exit" hint.
    /// Matches the TS string values in `useExitOnCtrlCD.ts:8`.
    pub fn label(self) -> &'static str {
        match self {
            Self::CtrlC => "Ctrl-C",
            Self::CtrlD => "Ctrl-D",
        }
    }
}

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
    overlay: Option<Overlay>,
    /// Queued overlays awaiting display.
    overlay_queue: VecDeque<Overlay>,
    /// Monotonic identity for the currently active overlay surface.
    ///
    /// Incremented only when the active overlay instance changes, not when a
    /// renderer mutates selection/filter state inside the same overlay.
    overlay_generation: u64,
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
    /// Runtime theme registry + persisted setting snapshot.
    pub theme_state: ThemeRuntimeState,
    /// Display preferences derived from settings.json.
    pub display_settings: DisplaySettings,
    /// Active toast notifications.
    pub toasts: VecDeque<Toast>,
    /// Status-bar warning for terminal compatibility downgrades.
    pub terminal_compatibility_warning: Option<String>,
    /// IDs of collapsed tool calls.
    pub collapsed_tools: HashSet<String>,
    /// Help overlay scroll position.
    pub help_scroll: i32,
    /// Double-press tracker for Ctrl+C → exit. Independent from
    /// [`ctrl_d_tracker`] so a "Ctrl+C, Ctrl+D, Ctrl+C" sequence within
    /// the window still completes the Ctrl+C double-press — mirrors
    /// TS `useExitOnCtrlCD.ts` (two parallel `useDoublePress` hooks).
    pub ctrl_c_tracker: DoublePressTracker<()>,
    /// Double-press tracker for Ctrl+D → exit. See [`ctrl_c_tracker`].
    pub ctrl_d_tracker: DoublePressTracker<()>,
    /// Double-press tracker for Esc → Rewind overlay. The Esc keystroke
    /// itself fires `TuiCommand::Cancel` on every press; this tracker
    /// only controls whether the second Esc opens the rewind picker.
    /// TS: `useDoublePress` inside `PromptInput.tsx`.
    pub esc_tracker: DoublePressTracker<()>,
    /// Whether the terminal window currently has focus. Used to gate
    /// turn-complete notifications so they only fire when the user has
    /// switched away — matches TS `ink::focus` semantics.
    pub terminal_focused: bool,
    /// Last time the retained native surface was known to be visible.
    ///
    /// This intentionally excludes generic lifecycle timestamps like turn
    /// completion. Native scrollback visibility is only considered known after
    /// an app-directed key/paste interaction or after focus gain is followed by
    /// a successful retained-surface draw.
    pub surface_visibility_known_at: Option<Instant>,
    surface_visibility_confirmation_pending: bool,
    /// Platform clipboard lease held alive for the lifetime of the TUI. On
    /// Linux/X11 and some Wayland setups the clipboard is served by the
    /// process that wrote it, so dropping the `arboard::Clipboard` handle
    /// would wipe the copied text. The lease is `None` on other platforms
    /// and on the OSC 52 path where no in-process ownership is required.
    pub clipboard_lease: Option<crate::clipboard_copy::ClipboardLease>,
    /// Active autocomplete suggestions (slash commands, @-mentions, etc.).
    /// `Some(_)` drives the keybinding bridge into `Autocomplete` context
    /// and renders a popup above the input. Recomputed after every input
    /// mutation in `autocomplete::refresh_suggestions`.
    pub active_suggestions: Option<ActiveSuggestions>,
    /// Keybinding resolver + warnings + display platform. Cheap to clone
    /// (`Arc` internally). Defaults to a from-defaults handle; the
    /// CLI bootstrap (`tui_runner`) replaces it with a watcher-backed
    /// handle so `~/.coco/keybindings.json` customizations + hot reload
    /// take effect.
    ///
    /// Lives in state (not a process-wide global) so each test gets
    /// its own handle and `cargo test --lib` runs without
    /// `serial_test` guards.
    pub kb_handle: KeybindingHandle,
    /// Whether teammate spinner lines show recent message preview.
    /// TS `AppStateStore.ts::showTeammateMessagePreview` (default
    /// false). Toggled via `app:toggleTeammatePreview` (Ctrl+Shift+O).
    pub show_teammate_message_preview: bool,
    /// Whether subagent activity renders the coordinator task view.
    /// Resolved by the CLI runner from runtime feature gates and env
    /// before rendering, so the view remains deterministic from state.
    pub coordinator_mode_active: bool,
    /// Stashed input draft from `chat:stash` (Ctrl+S in defaults).
    ///
    /// Mirrors TS `PromptInput.tsx::handleStash` (single-slot push/pop
    /// semantics). Three cases:
    /// * empty input + stash present → pop stash into input
    /// * non-empty input → push to stash (overwriting any prior),
    ///   clear input
    /// * empty input + empty stash → silent no-op
    pub stashed_input: Option<StashedInput>,
}

/// One slot of stashed input. Mirrors TS `StashedPrompt` shape
/// (`PromptInput.tsx:1359-1365`): text + cursor + paste-manager state.
#[derive(Debug, Clone)]
pub struct StashedInput {
    /// Stashed text content.
    pub text: String,
    /// Cursor byte offset at stash time. Restored alongside `text` on pop.
    /// In-memory only (no on-disk persistence), so the encoding change
    /// from char-index → byte-offset doesn't require migration.
    pub cursor_byte: usize,
    /// Snapshot of paste-pill entries (TS `pastedContents`) at stash
    /// time. Restored on pop so pill labels in the stashed `text`
    /// (e.g. `[Pasted text #1]`) still resolve to the original
    /// content. Empty `Vec` when the user hadn't pasted anything.
    pub paste_entries: Vec<crate::paste::PasteEntry>,
}

impl UiState {
    /// Create a new default UI state.
    pub fn new() -> Self {
        let theme_state = ThemeRuntimeState::default();
        Self {
            input: InputState::new(),
            paste_manager: crate::paste::PasteManager::new(),
            scroll_offset: 0,
            focus: FocusTarget::Input,
            overlay: None,
            overlay_queue: VecDeque::new(),
            overlay_generation: 0,
            streaming: None,
            show_thinking: true,
            show_system_reminders: false,
            user_scrolled: false,
            theme: theme_state.theme.clone(),
            theme_state,
            display_settings: DisplaySettings::default(),
            toasts: VecDeque::new(),
            terminal_compatibility_warning: None,
            collapsed_tools: HashSet::new(),
            help_scroll: 0,
            ctrl_c_tracker: DoublePressTracker::new(constants::DOUBLE_PRESS_TIMEOUT),
            ctrl_d_tracker: DoublePressTracker::new(constants::DOUBLE_PRESS_TIMEOUT),
            esc_tracker: DoublePressTracker::new(constants::DOUBLE_PRESS_TIMEOUT),
            terminal_focused: true,
            surface_visibility_known_at: None,
            surface_visibility_confirmation_pending: false,
            clipboard_lease: None,
            active_suggestions: None,
            kb_handle: KeybindingHandle::from_defaults(),
            stashed_input: None,
            show_teammate_message_preview: false,
            coordinator_mode_active: false,
        }
    }

    pub fn apply_theme_runtime(&mut self, theme_state: ThemeRuntimeState) {
        self.theme = theme_state.theme.clone();
        if let Some(Overlay::Settings(settings)) = self.overlay.as_mut() {
            settings.set_themes(theme_state.choices.clone(), theme_state.setting.clone());
        }
        self.theme_state = theme_state;
    }

    pub fn apply_theme_setting(&mut self, setting: ThemeSetting) -> anyhow::Result<()> {
        let theme_state = self.theme_state.with_setting(setting)?;
        self.apply_theme_runtime(theme_state);
        Ok(())
    }

    pub fn apply_display_settings(&mut self, display_settings: DisplaySettings) {
        self.display_settings = display_settings;
        if let Some(Overlay::Settings(settings)) = self.overlay.as_mut() {
            settings.set_display_settings(display_settings);
        }
    }

    pub fn active_overlay(&self) -> Option<&Overlay> {
        self.overlay.as_ref()
    }

    pub fn active_overlay_mut(&mut self) -> Option<&mut Overlay> {
        self.overlay.as_mut()
    }

    pub fn has_overlay(&self) -> bool {
        self.overlay.is_some()
    }

    pub fn overlay_generation(&self) -> u64 {
        self.overlay_generation
    }

    #[cfg(test)]
    pub fn overlay_queue_len(&self) -> usize {
        self.overlay_queue.len()
    }

    #[cfg(test)]
    pub fn overlay_queue_front(&self) -> Option<&Overlay> {
        self.overlay_queue.front()
    }

    /// Set the active overlay using the [`Overlay::priority`] ranking.
    ///
    /// Rules (see `crate-coco-tui.md` §Overlay Priority):
    /// - No active overlay: install directly.
    /// - New overlay has strictly higher priority (lower number): displace
    ///   the current overlay back into the queue and install the new one.
    /// - Otherwise: insert into the queue at its priority position. Same
    ///   priority keeps insertion order (stable within a tier).
    ///
    /// Queue overflow drops the lowest-priority tail entry to make room so a
    /// security-critical overlay can still enqueue.
    pub fn set_overlay(&mut self, overlay: Overlay) {
        match self.overlay.take() {
            None => {
                self.bump_overlay_generation();
                self.overlay = Some(overlay);
            }
            Some(current) => {
                if overlay.priority() < current.priority() {
                    // New overlay has higher priority — displace current.
                    self.bump_overlay_generation();
                    self.overlay = Some(overlay);
                    self.enqueue_overlay(current);
                } else {
                    // Same-or-lower priority: keep current, queue the new one.
                    self.overlay = Some(current);
                    self.enqueue_overlay(overlay);
                }
            }
        }
    }

    /// Insert `overlay` into the priority-ordered queue. Drops the
    /// lowest-priority entry on overflow to keep more important overlays in.
    fn enqueue_overlay(&mut self, overlay: Overlay) {
        let max = constants::MAX_OVERLAY_QUEUE as usize;
        let prio = overlay.priority();
        let pos = self
            .overlay_queue
            .iter()
            .position(|o| o.priority() > prio)
            .unwrap_or(self.overlay_queue.len());
        self.overlay_queue.insert(pos, overlay);
        while self.overlay_queue.len() > max {
            self.overlay_queue.pop_back();
        }
    }

    /// Dismiss the current overlay and show the next queued one.
    pub fn dismiss_overlay(&mut self) {
        if self.overlay.is_some() || !self.overlay_queue.is_empty() {
            self.overlay = self.overlay_queue.pop_front();
            self.bump_overlay_generation();
        }
    }

    /// Take the active overlay for in-place handling.
    ///
    /// Call [`Self::restore_active_overlay`] when putting back the same overlay,
    /// [`Self::install_active_overlay`] when replacing it with a different
    /// surface, or [`Self::finish_taken_overlay`] when dismissing it.
    pub fn take_active_overlay(&mut self) -> Option<Overlay> {
        self.overlay.take()
    }

    /// Restore the same active overlay after mutating its internal state.
    pub fn restore_active_overlay(&mut self, overlay: Overlay) {
        self.overlay = Some(overlay);
    }

    /// Install a different active overlay surface.
    pub fn install_active_overlay(&mut self, overlay: Overlay) {
        self.overlay = Some(overlay);
        self.bump_overlay_generation();
    }

    /// Complete a handler that took and dismissed the previous active overlay.
    pub fn finish_taken_overlay(&mut self) {
        self.overlay = self.overlay_queue.pop_front();
        self.bump_overlay_generation();
    }

    /// Clear active and queued overlays.
    pub fn clear_overlays(&mut self) {
        let had_overlay = self.overlay.is_some() || !self.overlay_queue.is_empty();
        self.overlay = None;
        self.overlay_queue.clear();
        if had_overlay {
            self.bump_overlay_generation();
        }
    }

    /// Record a key/paste interaction routed to the retained surface.
    pub fn record_surface_interaction(&mut self, now: Instant) {
        self.surface_visibility_known_at = Some(now);
        self.surface_visibility_confirmation_pending = false;
    }

    /// Focus gain only proves visibility after a successful retained draw.
    pub fn request_surface_visibility_confirmation(&mut self) {
        self.surface_visibility_confirmation_pending = true;
    }

    pub fn clear_surface_visibility_confirmation(&mut self) {
        self.surface_visibility_confirmation_pending = false;
    }

    pub fn confirm_surface_visibility_after_draw(&mut self, now: Instant) {
        if self.surface_visibility_confirmation_pending {
            self.surface_visibility_known_at = Some(now);
            self.surface_visibility_confirmation_pending = false;
        }
    }

    fn bump_overlay_generation(&mut self) {
        self.overlay_generation = self.overlay_generation.wrapping_add(1);
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

    /// Which exit key is currently armed for double-press confirmation.
    /// When both trackers are armed (uncommon but possible if the user
    /// alternates Ctrl+C/Ctrl+D), the most recently armed key wins so
    /// the hint reflects the latest keystroke. Mirrors TS
    /// `ExitState { pending, keyName }` — only one prompt visible.
    pub fn pending_exit_hint(&self) -> Option<ExitKey> {
        match (
            self.ctrl_c_tracker.pending_until(),
            self.ctrl_d_tracker.pending_until(),
        ) {
            (Some(cu), Some(du)) => Some(if du > cu {
                ExitKey::CtrlD
            } else {
                ExitKey::CtrlC
            }),
            (Some(_), None) => Some(ExitKey::CtrlC),
            (None, Some(_)) => Some(ExitKey::CtrlD),
            (None, None) => None,
        }
    }

    /// Advance both exit trackers and the Esc tracker. Returns `true`
    /// if any tracker just expired (the caller should request a redraw
    /// so the "press again" hint disappears).
    pub fn tick_double_press(&mut self, now: Instant) -> bool {
        let a = self.ctrl_c_tracker.tick(now);
        let b = self.ctrl_d_tracker.tick(now);
        let c = self.esc_tracker.tick(now);
        a || b || c
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

/// Which autocomplete trigger produced the active suggestions.
///
/// Determines the popup title, the source of suggestion items, and how the
/// accepted item is substituted back into the input. Kinds map to TS's four
/// mention triggers plus the leading `/` slash-command trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionKind {
    /// Leading `/` — populated from `session.available_commands`.
    SlashCommand,
    /// `@path` — populated asynchronously by `FileSearchManager`.
    File,
    /// `@agent-name` — populated from the session's agent registry.
    Agent,
    /// `@#symbol` — populated asynchronously by `SymbolSearchManager` (LSP).
    Symbol,
}

/// Active autocomplete session: popup rendered above input, intercepts
/// `Up/Down/Tab/Esc` while letting regular typing pass through.
#[derive(Debug, Clone)]
pub struct ActiveSuggestions {
    pub kind: SuggestionKind,
    /// Items to show in the popup — filtered by `query` before display.
    pub items: Vec<SuggestionItem>,
    /// Currently selected index into the filtered list.
    pub selected: usize,
    /// The filter text the user has typed after the trigger.
    pub query: String,
    /// Byte offset in `input.text` where the trigger started (the `/`
    /// or `@`). Used when accepting a suggestion to splice the selection
    /// back into the input via `textarea.replace_range`.
    pub trigger_pos: usize,
}

/// A single history entry with frecency metadata.
///
/// TS: `frequencyMap` in PromptInput.tsx — each entry tracks how often it was
/// used and when it was last entered. Navigation sorts by a frecency score
/// (`ln(frequency) * recency_factor`) rather than raw insertion order, so a
/// command typed ten times last week floats above a one-off from yesterday.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub text: String,
    /// How many times this exact text has been submitted.
    pub frequency: i32,
    /// Unix-epoch seconds of the most recent submission.
    pub last_used_secs: i64,
}

impl HistoryEntry {
    /// Frecency score — higher is more relevant. Returns `f64` so the caller
    /// can sort_by on the raw value without losing ordering granularity.
    ///
    /// Formula: `ln(frequency + 1) * recency_factor`, where
    /// `recency_factor = 1.0` for entries less than 24h old and decays
    /// exponentially with 7-day half-life for older entries. This matches
    /// the TS weighting and guards `ln(0)` by adding 1 to frequency.
    pub fn frecency(&self, now_secs: i64) -> f64 {
        let freq = ((self.frequency.max(0) + 1) as f64).ln();
        let age = (now_secs - self.last_used_secs).max(0) as f64;
        let day = 86_400.0_f64;
        let recency = if age < day {
            1.0
        } else {
            let weeks = (age - day) / (7.0 * day);
            0.5_f64.powf(weeks)
        };
        freq * recency
    }
}

/// Input prefix mode — derived from the leading character of [`InputState::text`].
///
/// TS parity:
/// * `Bash` mirrors TS `PromptInputMode = 'bash'` (typed `!` prefix in
///   `components/PromptInput/inputModes.ts`). Submit bypasses the model
///   loop and runs the shell directly, like TS's `LocalShellTask`.
/// * Memory capture uses the `/memory` slash command and file picker.
///   Leading `#` is ordinary chat text, matching TS input-mode behavior.
///
/// The mode is computed on the fly so backspacing past the prefix
/// character returns to `Normal` automatically — no separate state to
/// keep in sync.
///
/// Plan mode is *not* a prompt mode; it's a permission mode set via
/// `Shift+Tab` cycle and shown in the input title, not via prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PromptMode {
    /// Standard chat input.
    #[default]
    Normal,
    /// Leading `!` — submit runs as a shell command. TS: `LocalShellTask`.
    Bash,
}

impl PromptMode {
    /// Compute the mode from the leading character of `text`.
    ///
    /// Returns `Normal` for empty input and for anything that doesn't
    /// match a known prefix. Whitespace before the prefix disqualifies
    /// the mode (matches TS: `getModeFromInput` checks `startsWith`).
    pub fn from_text(text: &str) -> Self {
        match text.as_bytes().first() {
            Some(b'!') => Self::Bash,
            _ => Self::Normal,
        }
    }

    /// Strip the mode prefix from `text` (including one optional space
    /// after it). Returns `text` unchanged for `Normal`.
    ///
    /// Used at submit time so `!ls -la` becomes the command `ls -la`.
    pub fn strip_prefix(self, text: &str) -> &str {
        match self {
            Self::Normal => text,
            Self::Bash => {
                let stripped = &text[1..];
                stripped.strip_prefix(' ').unwrap_or(stripped)
            }
        }
    }

    /// i18n key for the input title shown when this mode is active.
    pub fn title_i18n_key(self) -> &'static str {
        match self {
            Self::Normal => "input.title",
            Self::Bash => "input.title_bash_mode",
        }
    }
}

/// Multi-line input state.
///
/// Backed by a [`TextArea`] (byte-offset cursor, multi-line wrapped,
/// grapheme + display-width aware) so the cursor renders correctly over
/// CJK / wide characters and multi-line input wraps. Frecency-ranked
/// history + vim runtime live alongside it.
#[derive(Debug)]
pub struct InputState {
    /// The editable buffer + cursor. Edit it directly for byte-offset
    /// access; the surrounding `InputState` API only owns history + vim.
    pub textarea: crate::widgets::TextArea,
    /// Command history ordered by frecency (most-relevant first).
    pub history: Vec<HistoryEntry>,
    /// Current history navigation index into `history` (None = live draft).
    pub history_index: Option<usize>,
    /// Vim modal-editing runtime: state machine + persistent register.
    pub vim: crate::vim::VimRuntime,
}

impl InputState {
    /// Create empty input.
    pub fn new() -> Self {
        Self {
            textarea: crate::widgets::TextArea::new(),
            history: Vec::new(),
            history_index: None,
            vim: crate::vim::VimRuntime::new(),
        }
    }

    /// Current text content.
    pub fn text(&self) -> &str {
        self.textarea.text()
    }

    /// Replace the entire input. Resets `history_index` so subsequent
    /// Up/Down navigation restarts from the live draft.
    pub fn set_text(&mut self, text: &str) {
        self.textarea.set_text(text);
        self.history_index = None;
    }

    /// Take the current input, clearing the buffer.
    pub fn take_input(&mut self) -> String {
        self.history_index = None;
        self.textarea.take_text()
    }

    /// Whether the input is empty.
    pub fn is_empty(&self) -> bool {
        self.textarea.is_empty()
    }

    /// Current prompt-prefix mode (derived from leading character).
    pub fn prompt_mode(&self) -> PromptMode {
        PromptMode::from_text(self.textarea.text())
    }

    /// Record a submitted text into history using frecency scoring.
    ///
    /// If the text already exists, bump its frequency and update `last_used`.
    /// Otherwise append a new entry. After insertion the vector is sorted
    /// descending by [`HistoryEntry::frecency`] so that up-arrow navigation
    /// walks the most relevant entries first. Capped at
    /// `constants::MAX_HISTORY_ENTRIES` by dropping the lowest-scoring tail.
    pub fn add_to_history(&mut self, text: String) {
        if text.is_empty() {
            return;
        }
        let now = now_unix_secs();
        if let Some(entry) = self.history.iter_mut().find(|h| h.text == text) {
            entry.frequency = entry.frequency.saturating_add(1);
            entry.last_used_secs = now;
        } else {
            self.history.push(HistoryEntry {
                text,
                frequency: 1,
                last_used_secs: now,
            });
        }
        // Sort by frecency desc; ties keep recent-first (stable sort on
        // original order where the most-recent append naturally sits last).
        self.history
            .sort_by(|a, b| b.frecency(now).total_cmp(&a.frecency(now)));
        let max = constants::MAX_HISTORY_ENTRIES as usize;
        if self.history.len() > max {
            self.history.truncate(max);
        }
    }
}

/// Unix-epoch seconds for the frecency timestamp. Clock skew or a pre-epoch
/// system date falls back to 0, which dampens the recency factor but keeps
/// the history useful rather than panicking.
fn now_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Default for InputState {
    fn default() -> Self {
        Self::new()
    }
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

#[cfg(test)]
#[path = "ui.test.rs"]
mod tests;
