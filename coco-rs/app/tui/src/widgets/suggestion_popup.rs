//! Shared suggestion popup widget for autocomplete + the slash command
//! palette.
//!
//! Renders an inline, borderless list directly above the input area —
//! mirrors the TS Claude Code style from
//! `components/PromptInput/PromptInputFooterSuggestions.tsx`. Each row is
//! a single text line: a leading `▸ ` / `  ` marker, then a fixed-width
//! name column padded with spaces, then a single-line description
//! truncated to the remaining width. Selected rows use the theme's
//! primary color (bold); unselected rows are rendered with `text_dim`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use unicode_width::UnicodeWidthStr;

use crate::presentation::layout::truncate_to_width;
use crate::theme::Theme;

/// A suggestion item for the popup.
#[derive(Debug, Clone)]
pub struct SuggestionItem {
    /// Display text — for slash commands this already includes the
    /// leading `/`. The widget renders it verbatim.
    pub label: String,
    /// Optional single-line description; whitespace runs are collapsed
    /// to a single space before truncation.
    pub description: Option<String>,
    /// Optional kind-specific metadata (e.g. directory flag for path
    /// completions). `None` for legacy / context-free items.
    pub metadata: Option<SuggestionMeta>,
}

/// Per-kind metadata carried alongside a suggestion. Used by the input
/// handler to format insertion correctly (directory `/` suffix vs file
/// trailing-space, MCP resource server prefix, etc.).
#[derive(Debug, Clone)]
pub enum SuggestionMeta {
    /// Path completion (file or directory). `is_directory` lets the
    /// insertion path append `/` and keep the popup open for drilling.
    Path { is_directory: bool },
}

/// Suggestion popup widget.
pub struct SuggestionPopup<'a> {
    items: &'a [SuggestionItem],
    selected: usize,
    theme: &'a Theme,
    max_visible: usize,
}

impl<'a> SuggestionPopup<'a> {
    /// Default cap on visible rows — matches TS `OVERLAY_MAX_ITEMS = 5`
    /// doubled to allow more breathing room when chat is tall enough.
    /// Callers that drive their own row reservation (e.g. the TUI's
    /// vertical layout) should override via `max_visible` so the widget
    /// can't overflow the slot.
    pub const DEFAULT_MAX_VISIBLE: u16 = 10;

    pub fn new(items: &'a [SuggestionItem], theme: &'a Theme) -> Self {
        Self {
            items,
            selected: 0,
            theme,
            max_visible: Self::DEFAULT_MAX_VISIBLE as usize,
        }
    }

    pub fn selected(mut self, index: usize) -> Self {
        self.selected = index;
        self
    }

    pub fn max_visible(mut self, max: usize) -> Self {
        self.max_visible = max;
        self
    }
}

/// Width of the leading marker (`▸ ` / `  `).
const MARKER_WIDTH: usize = 2;
/// Trailing padding between the name column and the description so the
/// description never abuts the longest name in the list.
const NAME_COLUMN_PADDING: usize = 2;
/// Cap on the name column as a percentage of the popup's total width.
/// Matches TS `maxNameWidth = Math.floor(columns * 0.4)`.
const NAME_COLUMN_CAP_PCT: usize = 40;
/// Floor on the name column when items are extremely short so the
/// description still has a stable starting column.
const NAME_COLUMN_FLOOR: usize = 10;

impl Widget for SuggestionPopup<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.items.is_empty() {
            return;
        }
        let popup_width = area.width;
        if popup_width == 0 {
            return;
        }

        let visible_count = self.items.len().min(self.max_visible).max(1);
        let popup_height = visible_count as u16;

        // Walk up from the anchor (input area top) by `popup_height` so
        // the popup floats directly above the input. `Clear` blanks the
        // chat tail underneath so it can't bleed through.
        let y = area.y.saturating_sub(popup_height);
        let popup_area = Rect::new(area.x, y, popup_width, popup_height);

        Clear.render(popup_area, buf);

        // Fixed name column = longest label + padding, capped at 40% of
        // popup width and floored so very-short items still leave room
        // for the description.
        let max_label_width = self
            .items
            .iter()
            .map(|item| UnicodeWidthStr::width(item.label.as_str()))
            .max()
            .unwrap_or(0);
        let column_cap =
            ((popup_width as usize) * NAME_COLUMN_CAP_PCT / 100).max(NAME_COLUMN_FLOOR);
        let name_col_width = (max_label_width + NAME_COLUMN_PADDING)
            .min(column_cap)
            .max(NAME_COLUMN_FLOOR.min(column_cap));

        // Center the selected row in the visible window so the user
        // sees context above and below as they navigate.
        let total = self.items.len();
        let half = visible_count / 2;
        let max_start = total.saturating_sub(visible_count);
        let start = self.selected.saturating_sub(half).min(max_start);
        let end = (start + visible_count).min(self.items.len());

        let mut lines: Vec<Line> = Vec::with_capacity(end - start);
        for (i, item) in self.items[start..end].iter().enumerate() {
            let actual_idx = start + i;
            let is_selected = actual_idx == self.selected;
            lines.push(build_row(
                item,
                is_selected,
                name_col_width,
                popup_width as usize,
                self.theme,
            ));
        }

        Paragraph::new(lines).render(popup_area, buf);
    }
}

fn build_row(
    item: &SuggestionItem,
    is_selected: bool,
    name_col_width: usize,
    popup_width: usize,
    theme: &Theme,
) -> Line<'static> {
    let marker = if is_selected { "▸ " } else { "  " };
    let label_style = if is_selected {
        Style::default().fg(theme.primary).bold()
    } else {
        Style::default().fg(theme.text_dim)
    };
    let desc_style = if is_selected {
        Style::default().fg(theme.text)
    } else {
        Style::default().fg(theme.text_dim)
    };

    // Truncate label to fit the column (minus the inter-column padding)
    // so even the longest label leaves at least one space before the
    // description starts.
    let label_target = name_col_width.saturating_sub(NAME_COLUMN_PADDING);
    let label_text = if UnicodeWidthStr::width(item.label.as_str()) > label_target {
        truncate_to_width(&item.label, label_target)
    } else {
        item.label.clone()
    };
    let label_used = UnicodeWidthStr::width(label_text.as_str());
    let pad = " ".repeat(name_col_width.saturating_sub(label_used));

    let mut spans: Vec<Span<'static>> = vec![
        Span::styled(marker, label_style),
        Span::styled(label_text, label_style),
        Span::raw(pad),
    ];

    if let Some(desc) = item.description.as_ref() {
        let remaining = popup_width.saturating_sub(MARKER_WIDTH + name_col_width);
        if remaining > 0 {
            let normalized = normalize_whitespace(desc);
            let truncated = truncate_to_width(&normalized, remaining);
            spans.push(Span::styled(truncated, desc_style));
        }
    }

    Line::from(spans)
}

/// Collapse runs of whitespace in a description down to a single space
/// — matches TS `description.replace(/\s+/g, " ")` so multi-line help
/// text renders on one inline row.
fn normalize_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out
}

#[cfg(test)]
#[path = "suggestion_popup.test.rs"]
mod tests;
