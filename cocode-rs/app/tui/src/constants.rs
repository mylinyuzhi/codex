//! Named constants for the TUI.
//!
//! Centralizes magic numbers used across TUI modules so they are
//! discoverable and easy to tune. Values that are `i32` should be
//! cast to `u16` (or other target type) at the call site when passed
//! to ratatui.

use std::time::Duration;

// ========== Layout ==========

/// Minimum terminal width (columns) before the side panel is shown.
pub const SIDE_PANEL_MIN_WIDTH: i32 = 100;

/// Terminal width at which we switch to a wider main/side split ratio.
pub const WIDE_TERMINAL_WIDTH: i32 = 160;

/// Main area percentage in wide terminals (>= 160 columns).
pub const WIDE_TERMINAL_MAIN_PCT: i32 = 75;

/// Side area percentage in wide terminals.
pub const WIDE_TERMINAL_SIDE_PCT: i32 = 25;

/// Main area percentage in normal-width terminals.
pub const NORMAL_TERMINAL_MAIN_PCT: i32 = 70;

/// Side area percentage in normal-width terminals.
pub const NORMAL_TERMINAL_SIDE_PCT: i32 = 30;

/// Maximum height (rows) for the multi-line input box, including borders.
pub const MAX_INPUT_HEIGHT: i32 = 10;

// ========== Display Limits ==========

/// Maximum number of tool executions shown in the side panel.
pub const MAX_TOOL_PANEL_DISPLAY: i32 = 8;

/// Maximum number of subagents shown in the side panel.
pub const MAX_SUBAGENT_PANEL_DISPLAY: i32 = 5;

/// Maximum active toast notifications at once.
pub const MAX_TOASTS: i32 = 5;

/// Maximum queued overlays awaiting display.
pub const MAX_OVERLAY_QUEUE: i32 = 16;

/// Maximum command history entries retained.
pub const MAX_HISTORY_ENTRIES: i32 = 100;

/// Maximum number of inline paste cache entries before LRU eviction.
pub const MAX_PASTE_INLINE_ENTRIES: i32 = 100;

// ========== Scrolling ==========

/// Lines to scroll per arrow-key scroll step.
pub const SCROLL_LINE_STEP: i32 = 3;

/// Lines to scroll per page-scroll step.
pub const SCROLL_PAGE_STEP: i32 = 20;

// ========== Timing ==========

/// Window within which two consecutive Esc presses count as double-Esc.
pub const DOUBLE_ESC_WINDOW: Duration = Duration::from_millis(800);

/// Elapsed query time after which a "slow query" toast is shown.
pub const SLOW_QUERY_THRESHOLD: Duration = Duration::from_secs(30);

/// Idle time before showing a "waiting for input" notification.
pub const IDLE_NOTIFICATION_TIMEOUT: Duration = Duration::from_secs(120);

// ========== Toast Durations ==========

/// Default display duration for info toasts.
pub const TOAST_DURATION_INFO: Duration = Duration::from_secs(3);

/// Default display duration for success toasts.
pub const TOAST_DURATION_SUCCESS: Duration = Duration::from_secs(3);

/// Default display duration for warning toasts.
pub const TOAST_DURATION_WARNING: Duration = Duration::from_secs(5);

/// Default display duration for error toasts.
pub const TOAST_DURATION_ERROR: Duration = Duration::from_secs(8);

// ========== Status Bar ==========

/// Truncate model names longer than this many characters.
pub const MODEL_NAME_MAX_LEN: i32 = 24;

/// Number of filled/empty blocks in the context gauge bar.
pub const CONTEXT_GAUGE_BAR_COUNT: i32 = 6;

/// Context usage percentage below which the gauge is green (success).
pub const CONTEXT_WARNING_THRESHOLD: i32 = 60;

/// Context usage percentage at or above which the gauge turns red (error).
pub const CONTEXT_ERROR_THRESHOLD: i32 = 80;

/// Truncate the working directory display beyond this many characters.
pub const WORKING_DIR_MAX_LEN: i32 = 25;

/// Token count at or above which we format as "N.NM".
pub const TOKEN_FORMAT_MILLIONS: i64 = 1_000_000;

/// Token count at or above which we format as "N.Nk".
pub const TOKEN_FORMAT_THOUSANDS: i64 = 1_000;

// ========== Side Panel ==========

/// Percentage of side panel height given to the tools panel (when both tools and subagents exist).
pub const SIDE_PANEL_TOOL_PCT: i32 = 50;

/// Percentage of side panel height given to the subagent panel (when both tools and subagents exist).
pub const SIDE_PANEL_SUBAGENT_PCT: i32 = 50;

// ========== Overlay Sizing ==========

/// Default overlay width as a percentage of terminal width.
pub const DEFAULT_OVERLAY_WIDTH_PCT: i32 = 60;

/// Minimum width for standard overlays (percentage-based, clamped).
pub const DEFAULT_OVERLAY_MIN_WIDTH: i32 = 40;

/// Maximum width for standard overlays.
pub const DEFAULT_OVERLAY_MAX_WIDTH: i32 = 80;

/// Width percentage target for the plugin manager overlay.
pub const PLUGIN_MANAGER_OVERLAY_WIDTH_PCT: i32 = 80;

/// Fixed height for the permission approval overlay.
pub const PERMISSION_OVERLAY_HEIGHT: i32 = 12;

/// Fixed height for the sandbox permission overlay.
pub const SANDBOX_PERMISSION_OVERLAY_HEIGHT: i32 = 14;

/// Fixed height for the plan-exit approval overlay (includes space for feedback input).
pub const PLAN_EXIT_OVERLAY_HEIGHT: i32 = 20;

/// Fixed height for the help overlay.
pub const HELP_OVERLAY_HEIGHT: i32 = 30;

/// Fixed height for the error overlay.
pub const ERROR_OVERLAY_HEIGHT: i32 = 8;

/// Maximum height for the model picker list overlay.
pub const MODEL_PICKER_MAX_HEIGHT: i32 = 20;

/// Maximum height for the rewind selector overlay.
pub const REWIND_SELECTOR_MAX_HEIGHT: i32 = 22;

/// Maximum height for the question overlay.
pub const QUESTION_OVERLAY_MAX_HEIGHT: i32 = 22;

/// Percentage of terminal height used for the plugin manager overlay.
pub const PLUGIN_MANAGER_HEIGHT_PCT: i32 = 70;

// ========== Autocomplete ==========

/// Maximum file search suggestions returned.
pub const FILE_SEARCH_MAX_SUGGESTIONS: i32 = 15;

/// Maximum skill search suggestions returned.
pub const SKILL_SEARCH_MAX_SUGGESTIONS: i32 = 10;

/// Maximum agent search suggestions returned.
pub const AGENT_SEARCH_MAX_SUGGESTIONS: i32 = 10;

/// Maximum symbol search suggestions returned.
pub const SYMBOL_SEARCH_MAX_SUGGESTIONS: i32 = 15;

/// Maximum visible rows in any suggestion popup.
pub const SUGGESTION_POPUP_MAX_VISIBLE: i32 = 8;

/// Debounce delay for file search queries.
pub const FILE_SEARCH_DEBOUNCE: Duration = Duration::from_millis(100);

/// Debounce delay for symbol search queries.
pub const SYMBOL_SEARCH_DEBOUNCE: Duration = Duration::from_millis(100);

/// Time-to-live for the cached file index.
pub const FILE_SEARCH_CACHE_TTL: Duration = Duration::from_secs(60);

// ========== Channels ==========

/// Default buffer size for the agent <-> TUI event channels.
pub const AGENT_CHANNEL_BUFFER: i32 = 32;

/// Default buffer size for the command (TUI -> Core) channel.
pub const COMMAND_CHANNEL_BUFFER: i32 = 32;

/// Buffer size for the file search event channel.
pub const FILE_SEARCH_CHANNEL_BUFFER: i32 = 16;

/// Buffer size for the symbol search event channel.
pub const SYMBOL_SEARCH_CHANNEL_BUFFER: i32 = 16;

// ========== Transcript Mode ==========

/// Maximum messages shown in transcript mode (most recent N).
pub const TRANSCRIPT_MODE_MESSAGE_LIMIT: usize = 10;

// ========== Token Estimation ==========

/// Rough multiplier to estimate token count from word count.
pub const THINKING_TOKEN_MULTIPLIER: f64 = 1.3;
