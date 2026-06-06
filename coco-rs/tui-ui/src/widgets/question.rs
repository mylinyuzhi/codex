//! Pure, domain-free AskUserQuestion widget.
//!
//! Area-based on purpose: it owns its `Rect` and splits it with a horizontal
//! layout so a focused-option preview can sit side-by-side — something the flat
//! `Vec<Line>` + single `Paragraph` path structurally cannot do. The `coco-tui`
//! shell projects its `QuestionPromptState` into [`QuestionView`] (doing all
//! i18n + chip/Other construction) and hands it here, so this crate stays free
//! of `AppState` and translated strings ("view-model in, ratatui out").

use ratatui::prelude::*;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Widget;
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

use crate::style::UiStyles;

/// Below this inner width a focused-option preview stacks under the options
/// instead of sitting in its own column.
const TWO_COL_MIN_WIDTH: u16 = 72;
/// Fixed width of the option column when the preview is side-by-side.
const OPTION_COL_WIDTH: u16 = 34;
/// Gap between the option column and the preview panel.
const COL_GAP: u16 = 2;

const CURSOR: &str = "❯ ";
const NO_CURSOR: &str = "  ";

/// Rows the multi-question nav strip reserves at the top of the inner area:
/// the tab row plus a trailing blank separator.
const NAV_ROWS: u16 = 2;

/// Selection marker for one option row.
pub enum RowMark {
    /// Single-select radio row.
    Radio { focused: bool },
    /// Multi-select checkbox row.
    Check { checked: bool, focused: bool },
}

impl RowMark {
    fn focused(&self) -> bool {
        match self {
            Self::Radio { focused } | Self::Check { focused, .. } => *focused,
        }
    }
}

/// One option row in the projected view.
pub struct OptionRow {
    /// 1-based number shown as a `n.` prefix and reachable by digit shortcut.
    pub number: usize,
    pub label: String,
    /// Rendered on its own indented line(s) below the label. Empty = omitted.
    pub description: String,
    pub mark: RowMark,
}

/// A footer action ("Chat about this" / "Skip interview").
pub struct FooterAction {
    pub label: String,
    pub focused: bool,
}

/// One tab in the multi-question navigation strip.
pub struct NavTab {
    /// Header chip, already truncated by the shell.
    pub header: String,
    /// Whether this question already resolves to an answer — drives the ☒/☐
    /// checkbox (mirrors TS `figures.checkboxOn/Off`).
    pub answered: bool,
}

/// Trailing "✔ Submit" tab in the navigation strip.
pub struct SubmitNavTab {
    /// Whether the Submit tab currently holds focus.
    pub focused: bool,
    /// Whether every question is answered (drives ✔ vs ☐).
    pub ready: bool,
}

/// Multi-question navigation strip, rendered above the prompt when an
/// AskUserQuestion call carries more than one question. Mirrors the TS
/// `QuestionNavigationBar` tab row (`← ☒ Nav  ☐ Scope  ✔ Submit →`), current
/// tab highlighted.
pub struct QuestionNav {
    pub tabs: Vec<NavTab>,
    /// Index of the focused question (ignored when [`Self::submit`] is focused).
    pub current: usize,
    /// Trailing Submit tab; `Some` whenever the strip is shown (>1 question).
    pub submit: Option<SubmitNavTab>,
}

/// Domain-free projection of an AskUserQuestion prompt.
pub struct QuestionView {
    /// Block title, e.g. " Question ".
    pub title: String,
    /// Header chip for a single-question prompt, already truncated by the
    /// shell. `None` when [`Self::nav`] is set (the strip shows every header).
    pub chip: Option<String>,
    /// Multi-question navigation strip; `Some` only when there is >1 question.
    pub nav: Option<QuestionNav>,
    pub prompt: String,
    pub rows: Vec<OptionRow>,
    /// Focused option's preview content (raw markdown/monospace), if any.
    pub preview: Option<String>,
    /// `Some(buffer)` only when the Other composer is focused — rendered as a
    /// `your answer: …▌` line with a caret.
    pub composer: Option<String>,
    pub footer: Vec<FooterAction>,
    /// Pre-joined hint line (dim), already localized.
    pub hints: String,
}

/// Result of laying out the left (options) column.
struct Composed {
    lines: Vec<Line<'static>>,
    /// Index of the focused line, so a scroll window can keep it visible.
    focused: Option<usize>,
}

impl QuestionView {
    /// Full desired height (incl. border) for an outer `width`. The caller
    /// clamps this to the available space; the widget scrolls the option rows
    /// to fit whatever area it is finally given.
    pub fn desired_height(&self, width: u16, styles: UiStyles<'_>) -> u16 {
        let inner = width.saturating_sub(2).max(1);
        // Nav strip occupies its own row plus a trailing blank, full-width.
        let nav_rows = if self.nav.is_some() { NAV_ROWS } else { 0 };
        let height = if self.two_col(inner) {
            let left_w = self.option_col_width(inner);
            let preview_w = inner.saturating_sub(left_w + COL_GAP).max(1);
            let left = self
                .compose(styles, left_w, /*with_preview=*/ false)
                .lines
                .len();
            let preview = self.preview_lines(styles, preview_w).len();
            left.max(preview)
        } else {
            self.compose(styles, inner, /*with_preview=*/ true)
                .lines
                .len()
        };
        (height as u16).saturating_add(nav_rows).saturating_add(2)
    }

    fn two_col(&self, inner_width: u16) -> bool {
        self.preview.is_some() && inner_width >= TWO_COL_MIN_WIDTH
    }

    fn option_col_width(&self, inner_width: u16) -> u16 {
        OPTION_COL_WIDTH.min(inner_width.saturating_sub(10).max(1))
    }

    /// Build the option-column lines. When `with_preview` is set (narrow
    /// layout) the preview is stacked at the bottom; otherwise it is omitted
    /// (the side panel renders it).
    fn compose(&self, styles: UiStyles<'_>, width: u16, with_preview: bool) -> Composed {
        let w = width.max(1) as usize;
        let mut lines: Vec<Line<'static>> = Vec::new();
        let mut focused: Option<usize> = None;

        if let Some(chip) = &self.chip {
            lines.push(Line::from(Span::styled(
                format!("[{chip}]"),
                Style::default().fg(styles.accent()),
            )));
        }
        for l in wrap(&self.prompt, w) {
            lines.push(Line::from(l));
        }
        lines.push(Line::from(""));

        let number_width = self.rows.len().to_string().len();
        for row in &self.rows {
            if row.mark.focused() {
                focused = Some(lines.len());
            }
            lines.push(self.option_line(row, styles, number_width));
            for d in wrap(&row.description, w.saturating_sub(4)) {
                if d.is_empty() {
                    continue;
                }
                lines.push(Line::from(Span::styled(
                    format!("    {d}"),
                    Style::default().fg(styles.dim()),
                )));
            }
        }

        if let Some(buffer) = &self.composer {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("your answer: ", Style::default().fg(styles.dim())),
                Span::raw(buffer.clone()),
                Span::styled("▌", Style::default().fg(styles.accent())),
            ]));
        }

        if with_preview && let Some(preview) = &self.preview {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "— preview —",
                Style::default().fg(styles.dim()),
            )));
            for l in wrap(preview, w) {
                lines.push(Line::from(l));
            }
        }

        if !self.footer.is_empty() {
            lines.push(Line::from(""));
            for action in &self.footer {
                if action.focused {
                    focused = Some(lines.len());
                }
                let cursor = if action.focused { CURSOR } else { NO_CURSOR };
                let color = if action.focused {
                    styles.accent()
                } else {
                    styles.text()
                };
                lines.push(Line::from(vec![
                    Span::styled(cursor, Style::default().fg(styles.accent())),
                    Span::styled(action.label.clone(), Style::default().fg(color)),
                ]));
            }
        }

        if !self.hints.is_empty() {
            lines.push(Line::from(""));
            for l in wrap(&self.hints, w) {
                lines.push(Line::from(Span::styled(
                    l,
                    Style::default().fg(styles.dim()),
                )));
            }
        }

        Composed { lines, focused }
    }

    fn option_line(
        &self,
        row: &OptionRow,
        styles: UiStyles<'_>,
        number_width: usize,
    ) -> Line<'static> {
        let focused = row.mark.focused();
        let cursor = if focused { CURSOR } else { NO_CURSOR };
        let label_color = if focused {
            styles.accent()
        } else {
            styles.text()
        };
        let mut spans = vec![
            Span::styled(cursor, Style::default().fg(styles.accent())),
            Span::styled(
                format!("{:>number_width$}. ", row.number),
                Style::default().fg(styles.dim()),
            ),
        ];
        if let RowMark::Check { checked, .. } = row.mark {
            let (glyph, color) = if checked {
                ("[x] ", styles.success())
            } else {
                ("[ ] ", styles.dim())
            };
            spans.push(Span::styled(glyph, Style::default().fg(color)));
        }
        spans.push(Span::styled(
            row.label.clone(),
            Style::default().fg(label_color),
        ));
        Line::from(spans)
    }

    fn preview_lines(&self, styles: UiStyles<'_>, width: u16) -> Vec<Line<'static>> {
        let Some(preview) = &self.preview else {
            return Vec::new();
        };
        let w = width.max(1) as usize;
        let mut lines = vec![Line::from(Span::styled(
            "preview",
            Style::default().fg(styles.dim()),
        ))];
        for l in wrap(preview, w) {
            lines.push(Line::from(l));
        }
        lines
    }
}

/// Renders a [`QuestionView`] into its area.
pub struct QuestionWidget<'a> {
    view: &'a QuestionView,
    styles: UiStyles<'a>,
}

impl<'a> QuestionWidget<'a> {
    pub fn new(view: &'a QuestionView, styles: UiStyles<'a>) -> Self {
        Self { view, styles }
    }
}

impl Widget for QuestionWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(self.view.title.clone())
            .border_style(Style::default().fg(self.styles.primary()));
        let inner = block.inner(area);
        block.render(area, buf);
        if inner.width == 0 || inner.height == 0 {
            return;
        }

        // The nav strip spans the full inner width above the columns, so carve
        // it off before the one-/two-column split.
        let mut content = inner;
        if let Some(nav) = &self.view.nav {
            buf.set_line(
                content.x,
                content.y,
                &nav_line(nav, self.styles),
                content.width,
            );
            let used = NAV_ROWS.min(content.height);
            content = Rect {
                y: content.y + used,
                height: content.height - used,
                ..content
            };
        }
        if content.height == 0 {
            return;
        }

        if self.view.two_col(content.width) {
            let left_w = self.view.option_col_width(content.width);
            let left = Rect {
                width: left_w,
                ..content
            };
            let preview = Rect {
                x: content.x + left_w + COL_GAP,
                width: content.width.saturating_sub(left_w + COL_GAP),
                ..content
            };
            let composed = self.view.compose(self.styles, left.width, false);
            render_scrolled(&composed, left, buf);
            let preview_lines = self.view.preview_lines(self.styles, preview.width);
            render_top(&preview_lines, preview, buf);
        } else {
            let composed = self.view.compose(self.styles, content.width, true);
            render_scrolled(&composed, content, buf);
        }
    }
}

/// One-row navigation strip: `← ☒ Nav  ☐ Scope  ✔ Submit →`, focused tab
/// highlighted with the theme selection colors, end arrows dimmed at the ring
/// boundaries (TS `QuestionNavigationBar`). The trailing `✔/☐ Submit` tab is
/// focusable; ✔ once every question is answered, ☐ otherwise.
fn nav_line(nav: &QuestionNav, styles: UiStyles<'_>) -> Line<'static> {
    let submit_focused = nav.submit.as_ref().is_some_and(|s| s.focused);
    // Index of the focused tab across [questions…, submit] for the arrow dimming.
    let focused_idx = if submit_focused {
        nav.tabs.len()
    } else {
        nav.current
    };
    let last = nav.tabs.len(); // questions occupy 0..len; submit is `len`.
    let arrow_color = |dim: bool| if dim { styles.dim() } else { styles.text() };
    let selected = || {
        Style::default()
            .bg(styles.selection_bg())
            .fg(styles.selection_fg())
    };
    let mut spans = vec![Span::styled(
        "← ",
        Style::default().fg(arrow_color(focused_idx == 0)),
    )];
    for (i, tab) in nav.tabs.iter().enumerate() {
        let checkbox = if tab.answered { "☒" } else { "☐" };
        let text = format!(" {checkbox} {} ", tab.header);
        let style = if !submit_focused && i == nav.current {
            selected()
        } else {
            Style::default().fg(styles.text())
        };
        spans.push(Span::styled(text, style));
    }
    if let Some(submit) = &nav.submit {
        let mark = if submit.ready { "✔" } else { "☐" };
        let style = if submit.focused {
            selected()
        } else {
            Style::default().fg(styles.text())
        };
        spans.push(Span::styled(format!(" {mark} Submit "), style));
    }
    spans.push(Span::styled(
        " →",
        Style::default().fg(arrow_color(focused_idx == last)),
    ));
    Line::from(spans)
}

/// Render `composed` into `area`, scrolling the lines so the focused line stays
/// visible when the content is taller than the area.
fn render_scrolled(composed: &Composed, area: Rect, buf: &mut Buffer) {
    let height = area.height as usize;
    let start = if composed.lines.len() <= height {
        0
    } else {
        let focused = composed.focused.unwrap_or(0);
        focused
            .saturating_sub(height / 2)
            .min(composed.lines.len() - height)
    };
    for (i, line) in composed.lines[start..].iter().take(height).enumerate() {
        buf.set_line(area.x, area.y + i as u16, line, area.width);
    }
}

fn render_top(lines: &[Line<'static>], area: Rect, buf: &mut Buffer) {
    for (i, line) in lines.iter().take(area.height as usize).enumerate() {
        buf.set_line(area.x, area.y + i as u16, line, area.width);
    }
}

/// Word-wrap `text` to `width` display columns, hard-breaking runs (e.g. CJK or
/// a single long token) that exceed the width. Preserves explicit newlines.
fn wrap(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    for raw in text.split('\n') {
        let mut line = String::new();
        let mut line_w = 0usize;
        for word in raw.split_whitespace() {
            let word_w = UnicodeWidthStr::width(word);
            let sep = usize::from(line_w > 0);
            if line_w > 0 && line_w + sep + word_w > width {
                out.push(std::mem::take(&mut line));
                line_w = 0;
            }
            if word_w > width {
                // Hard-break an over-wide token by display columns.
                if line_w > 0 {
                    out.push(std::mem::take(&mut line));
                    line_w = 0;
                }
                for ch in word.chars() {
                    let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
                    if line_w + cw > width {
                        out.push(std::mem::take(&mut line));
                        line_w = 0;
                    }
                    line.push(ch);
                    line_w += cw;
                }
                continue;
            }
            if line_w > 0 {
                line.push(' ');
                line_w += 1;
            }
            line.push_str(word);
            line_w += word_w;
        }
        out.push(line);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

#[cfg(test)]
#[path = "question.test.rs"]
mod tests;
