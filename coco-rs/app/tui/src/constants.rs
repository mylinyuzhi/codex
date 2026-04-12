//! Named constants for the TUI.
//!
//! Centralizes magic numbers used across TUI modules. Values are `i32`
//! and should be cast to `u16` at the call site when passed to ratatui.

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

// ========== Scrolling ==========

/// Lines to scroll per arrow-key scroll step.
pub const SCROLL_LINE_STEP: i32 = 3;

/// Lines to scroll per page-scroll step.
pub const SCROLL_PAGE_STEP: i32 = 20;

// ========== Timing ==========

/// Interval for status-bar updates, toast expiry, idle detection.
pub const TICK_INTERVAL: Duration = Duration::from_millis(250);

/// Interval for spinner animation frames.
pub const SPINNER_TICK_INTERVAL: Duration = Duration::from_millis(50);

/// Duration before an overlay transition gate drops.
pub const OVERLAY_TRANSITION_DURATION: Duration = Duration::from_millis(150);

// ========== Toast Durations ==========

/// Duration for informational toasts.
pub const TOAST_INFO_DURATION: Duration = Duration::from_secs(3);

/// Duration for success toasts.
pub const TOAST_SUCCESS_DURATION: Duration = Duration::from_secs(3);

/// Duration for warning toasts.
pub const TOAST_WARNING_DURATION: Duration = Duration::from_secs(5);

/// Duration for error toasts.
pub const TOAST_ERROR_DURATION: Duration = Duration::from_secs(8);

// ========== Text ==========

/// Maximum characters for tool description preview in side panel.
pub const TOOL_DESCRIPTION_MAX_CHARS: i32 = 40;

/// Multiplier to estimate token count from word count.
pub const THINKING_TOKEN_MULTIPLIER: f64 = 1.3;

// ========== Virtual Scroll ==========

/// Number of messages to render beyond the visible viewport (buffer).
pub const VIRTUAL_SCROLL_OVERSCAN: i32 = 5;

// ========== Table ==========

/// Maximum column width for markdown tables.
pub const TABLE_MAX_COL_WIDTH: i32 = 40;

/// Minimum column width for markdown tables.
pub const TABLE_MIN_COL_WIDTH: i32 = 5;

// ========== Search ==========

/// Maximum search results shown in global search overlay.
pub const MAX_SEARCH_RESULTS: i32 = 50;

// ========== Rewind ==========

/// Time window for double-Esc detection.
/// TS: useDoublePress() hook in PromptInput.tsx
pub const DOUBLE_ESC_THRESHOLD: Duration = Duration::from_millis(400);

/// Maximum visible messages in the rewind message selector.
/// TS: MAX_VISIBLE_MESSAGES = 7 in MessageSelector.tsx
pub const REWIND_MAX_VISIBLE: i32 = 7;

// ========== Mouse ==========

/// Lines per mouse scroll wheel tick.
pub const MOUSE_SCROLL_LINES: i32 = 3;
