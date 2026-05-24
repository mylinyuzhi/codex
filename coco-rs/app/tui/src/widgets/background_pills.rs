//! Background pills bar — flat `@name` list for backgrounded subagents.
//!
//! TS source: `components/tasks/BackgroundTaskStatus.tsx:153-156` and
//! `AgentPill` (same file, lines 280-377). The TS shape is a flat
//! `React.Fragment` list of `@{name}` `Text` nodes joined by a single
//! space, with idle/selected/viewed states picking different `Text`
//! styles (dim, inverse, colored). No brackets, no glyph, no elapsed
//! segment, no completion-flash window — backgrounded tasks remain in
//! the row indefinitely (TS `runningTasks` filter is just
//! `isBackgroundTask`, not "running within last 5 s").
//!
//! Width budgeting per pill matches TS `_temp1`:
//! `stringWidth("@" + name) + (i > 0 ? 1 : 0)`.
//!
//! TS-DIVERGE: three TS pieces are deliberately unimplemented and
//! tracked here so a future port can pick them up cleanly:
//! - **Leader pill** (`mainPill = { name: "main", ... }` in
//!   `BackgroundTaskStatus.tsx:64-76`): TS always prepends a leader
//!   row to the pills list. coco-rs has no "main" leader concept on
//!   the pills bar yet — the leader's status surfaces through the
//!   status indicator instead.
//! - **Summary pill** (`getPillLabel(runningTasks)` +
//!   `<SummaryPill>` branch at `BackgroundTaskStatus.tsx:200-233`):
//!   in non-teammate mode TS shows a single aggregated label
//!   ("3 running"). coco-rs always shows the per-task pill list and
//!   relies on overflow tail for compactness.
//! - **Horizontal scrolling** (`calculateHorizontalScrollWindow` +
//!   left/right arrow indicators, `BackgroundTaskStatus.tsx:114-145`):
//!   TS scrolls the pill row when it overflows and tracks a focused
//!   pill index. coco-rs renders `[+N more]` as a static tail
//!   instead — simpler, no focus state, but no way to inspect
//!   overflowed pills without entering the Teammates view.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use unicode_width::UnicodeWidthStr;

use crate::presentation::styles::UiStyles;
use crate::state::AppState;
use crate::state::session::SubagentStatus;

/// Reserve enough columns for a trailing `[+N more]` tail before we
/// commit to painting another pill.
const OVERFLOW_RESERVE: usize = 12;

/// Single-space gap between adjacent pills. TS:
/// `BackgroundTaskStatus.tsx:154` (`needsSeparator && <Text> </Text>`).
const PILL_GAP: &str = " ";

/// Prefix marker. TS renders `@{name}` literally — see `AgentPill`
/// branches (`<Text ...>@{name}</Text>`).
const PILL_PREFIX: &str = "@";

/// Per-pill view-model. `label` is the agent's display name (the
/// `@` prefix is added by the renderer). `is_idle` mirrors TS's
/// `isIdle` predicate — backgrounded subagents whose status has
/// reached a terminal value render dimmed.
#[derive(Debug, Clone)]
pub(crate) struct PillEntry<'a> {
    pub(crate) label: &'a str,
    pub(crate) is_idle: bool,
}

/// Borrowed view-model. Pills follow insertion order from
/// `state.session.subagents` (which is itself FIFO by spawn time).
#[derive(Debug, Clone, Default)]
pub(crate) struct BackgroundPillsView<'a> {
    pub(crate) pills: Vec<PillEntry<'a>>,
}

impl BackgroundPillsView<'_> {
    pub(crate) fn is_empty(&self) -> bool {
        self.pills.is_empty()
    }
}

/// Project `AppState` into the pills view-model.
///
/// Inclusion rule mirrors TS `runningTasks.filter(isBackgroundTask)` +
/// `_temp6 (type === "in_process_teammate")`:
/// every subagent flagged `is_backgrounded` participates, regardless
/// of run status — terminal ones render with `is_idle = true`.
pub(crate) fn build_view(state: &AppState) -> BackgroundPillsView<'_> {
    let pills = state
        .session
        .subagents
        .iter()
        .filter(|a| a.is_backgrounded)
        .map(|a| PillEntry {
            label: a.description.as_str(),
            is_idle: !matches!(a.status, SubagentStatus::Running),
        })
        .collect();
    BackgroundPillsView { pills }
}

/// Render `@a @b @c [+N more]` — single-space-separated, with an
/// overflow tail when the row would exceed the available width.
pub(crate) struct BackgroundPills<'a> {
    view: &'a BackgroundPillsView<'a>,
    styles: UiStyles<'a>,
}

impl<'a> BackgroundPills<'a> {
    pub(crate) fn new(view: &'a BackgroundPillsView<'a>, styles: UiStyles<'a>) -> Self {
        Self { view, styles }
    }
}

impl Widget for BackgroundPills<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 || self.view.is_empty() {
            return;
        }

        let max_w = usize::from(area.width);
        // Worst case is 3 spans per pill (gap + prefix + label) plus
        // 2 spans for the overflow tail.
        let mut spans: Vec<Span<'_>> = Vec::with_capacity(self.view.pills.len() * 3 + 2);
        let mut used = 0usize;
        let mut overflow = 0usize;

        for (i, pill) in self.view.pills.iter().enumerate() {
            let separator_w = if i == 0 { 0 } else { PILL_GAP.width() };
            let pill_w = PILL_PREFIX.width() + pill.label.width();
            let proposed = used + separator_w + pill_w;
            // Reserve room for `[+N more]` if there is at least one
            // more pill we might not be able to paint.
            if proposed + OVERFLOW_RESERVE > max_w && i + 1 < self.view.pills.len() {
                overflow = self.view.pills.len() - i;
                break;
            }
            if proposed > max_w {
                overflow = self.view.pills.len() - i;
                break;
            }
            if i > 0 {
                spans.push(Span::raw(PILL_GAP));
            }
            // TS: idle pills use `<Text dimColor>`; running pills use
            // `<Text color={color}>` with the agent's accent. coco-rs
            // doesn't yet thread agent-specific colors through, so we
            // fall back to `text` for running and `dim` for idle.
            let color = if pill.is_idle {
                self.styles.dim()
            } else {
                self.styles.text()
            };
            spans.push(Span::raw(PILL_PREFIX).fg(color));
            spans.push(Span::raw(pill.label).fg(color));
            used = proposed;
        }
        if overflow > 0 {
            spans.push(Span::raw(PILL_GAP).fg(self.styles.dim()));
            spans.push(Span::raw(format!("[+{overflow} more]")).fg(self.styles.dim()));
        }

        Paragraph::new(Line::from(spans)).render(area, buf);
    }
}

#[cfg(test)]
#[path = "background_pills.test.rs"]
mod tests;
