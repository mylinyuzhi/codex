//! Grammar-accurate markdown rendering for the coco TUI.
//!
//! Parses CommonMark + GFM with `pulldown-cmark` and emits owned
//! `Vec<Line<'static>>` for the native-scrollback engine. Code fences are
//! syntax-highlighted with syntect, mapped onto coco's themeable palette (see
//! [`highlight`]). Colors come exclusively from [`UiStyles`]; the lead turn
//! marker is a first-class input (see [`LeadMarker`]) rather than a string the
//! caller post-patches.
//!
//! Output contract matches the prior renderer: logical lines are emitted with a
//! `body_indent`-column left margin and are wrapped downstream at paint time
//! (`Paragraph::wrap`). This crate performs no internal width wrapping.

use coco_tui_ui::display::SyntaxHighlighting;
use coco_tui_ui::style::UiStyles;
use pulldown_cmark::Alignment;
use pulldown_cmark::BlockQuoteKind;
use pulldown_cmark::CodeBlockKind;
use pulldown_cmark::Event;
use pulldown_cmark::HeadingLevel;
use pulldown_cmark::Options;
use pulldown_cmark::Parser;
use pulldown_cmark::Tag;
use pulldown_cmark::TagEnd;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use unicode_width::UnicodeWidthStr;

mod highlight;

/// A turn-boundary marker placed at column 0 of the first rendered line (e.g.
/// the assistant `⏺` dot). The glyph plus a trailing space occupy exactly
/// `body_indent` columns so wrapped continuation lines stay aligned.
#[derive(Debug, Clone)]
pub struct LeadMarker {
    pub glyph: &'static str,
    pub style: Style,
}

impl LeadMarker {
    pub fn new(glyph: &'static str, color: Color) -> Self {
        Self {
            glyph,
            style: Style::default().fg(color),
        }
    }
}

/// Rendering options. `body_indent` replaces the old hard-coded two-space pad.
#[derive(Debug, Clone, Copy)]
pub struct MarkdownOptions<'a> {
    pub styles: UiStyles<'a>,
    pub width: u16,
    pub syntax: SyntaxHighlighting,
    pub body_indent: u16,
    /// True while rendering an in-flight streaming buffer. A mid-stream fence is
    /// not yet closed, so its body is a moving target — laying out a `mermaid`
    /// diagram on every delta makes the block flicker/reflow as it grows. When
    /// set, `mermaid` fences keep their verbatim form and only render as a
    /// diagram once on the finalized (non-streaming) pass.
    pub streaming: bool,
}

impl<'a> MarkdownOptions<'a> {
    /// Defaults matching the legacy renderer (two-space body indent).
    pub fn new(styles: UiStyles<'a>, width: u16, syntax: SyntaxHighlighting) -> Self {
        Self {
            styles,
            width,
            syntax,
            body_indent: 2,
            streaming: false,
        }
    }

    /// Mark this render as an in-flight streaming pass (suppresses per-delta
    /// `mermaid` diagram layout — see [`MarkdownOptions::streaming`]).
    pub fn streaming(mut self) -> Self {
        self.streaming = true;
        self
    }
}

/// Render markdown `text` to owned ratatui lines.
///
/// When `marker` is `Some`, the first emitted line carries the marker glyph at
/// column 0; when `text` is empty a single marker-only line is produced so a
/// turn boundary is still visible.
pub fn render_markdown(
    text: &str,
    opts: MarkdownOptions<'_>,
    marker: Option<&LeadMarker>,
) -> Vec<Line<'static>> {
    let mut writer = Writer::new(opts, marker);
    let mut parser_opts = Options::empty();
    parser_opts.insert(Options::ENABLE_STRIKETHROUGH);
    parser_opts.insert(Options::ENABLE_TABLES);
    parser_opts.insert(Options::ENABLE_TASKLISTS);
    parser_opts.insert(Options::ENABLE_GFM);
    for event in Parser::new_ext(text, parser_opts) {
        writer.event(event);
    }
    writer.finish()
}

/// Highlight raw code outside a Markdown fence.
///
/// Tool-result renderers use this for file-content previews where wrapping the
/// content in a synthetic code fence would add borders and break on embedded
/// fence markers. Returns `None` when highlighting is disabled, unsupported, or
/// too expensive; callers should render plain text in that case.
pub fn highlight_code_lines(
    code: &str,
    lang: &str,
    styles: UiStyles<'_>,
    syntax: SyntaxHighlighting,
) -> Option<std::sync::Arc<Vec<Vec<Span<'static>>>>> {
    highlight::highlight_code(code, lang, styles, syntax)
}

// ─────────────────────────────────────────────────────────────────────────
// Writer
// ─────────────────────────────────────────────────────────────────────────

struct TableBuilder {
    aligns: Vec<Alignment>,
    header: Vec<String>,
    rows: Vec<Vec<String>>,
    cur_row: Vec<String>,
    cur_cell: String,
    in_head: bool,
}

struct Writer<'a> {
    styles: UiStyles<'a>,
    width: u16,
    syntax: SyntaxHighlighting,
    body_indent: usize,
    /// Only read by the `#[cfg(feature = "mermaid")]` branch in
    /// `finish_code_block`; the default (no-mermaid) build never reads it.
    #[cfg_attr(not(feature = "mermaid"), allow(dead_code))]
    streaming: bool,

    lines: Vec<Line<'static>>,
    spans: Vec<Span<'static>>,

    cur_style: Style,
    style_stack: Vec<Style>,

    list_stack: Vec<Option<u64>>,
    pending_marker: Option<Span<'static>>,
    /// Per-open-item `(marker_width, first_line_emitted)`. Continuation lines
    /// (after the item's first line) hang-indent under the item text by the
    /// marker width; the first line (bullet, number, or task checkbox) does not.
    item_hang: Vec<(usize, bool)>,
    quote_gutters: Vec<Style>,

    in_code: bool,
    code_lang: String,
    code_buf: String,

    table: Option<TableBuilder>,

    lead_marker: Option<Span<'static>>,
    /// Display width of `lead_marker` ("{glyph} "), used to align the first
    /// line's padding with continuation lines independent of `body_indent`.
    lead_marker_width: usize,
    first_line_emitted: bool,
    needs_gap: bool,
    empty_marker: Option<Span<'static>>,
}

impl<'a> Writer<'a> {
    fn new(opts: MarkdownOptions<'a>, marker: Option<&LeadMarker>) -> Self {
        let lead_marker = marker.map(|m| Span::styled(format!("{} ", m.glyph), m.style));
        let lead_marker_width = lead_marker
            .as_ref()
            .map_or(0, |s| UnicodeWidthStr::width(s.content.as_ref()));
        let empty_marker = marker.map(|m| Span::styled(m.glyph.to_string(), m.style));
        Self {
            styles: opts.styles,
            width: opts.width,
            syntax: opts.syntax,
            body_indent: opts.body_indent as usize,
            streaming: opts.streaming,
            lines: Vec::new(),
            spans: Vec::new(),
            cur_style: Style::default(),
            style_stack: Vec::new(),
            list_stack: Vec::new(),
            pending_marker: None,
            item_hang: Vec::new(),
            quote_gutters: Vec::new(),
            in_code: false,
            code_lang: String::new(),
            code_buf: String::new(),
            table: None,
            lead_marker,
            lead_marker_width,
            first_line_emitted: false,
            needs_gap: false,
            empty_marker,
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        // Flush any dangling inline content.
        if !self.spans.is_empty() {
            self.flush_line();
        }
        if self.lines.is_empty()
            && let Some(marker) = self.empty_marker.take()
        {
            self.lines.push(Line::from(vec![marker]));
        }
        self.lines
    }

    fn list_depth(&self) -> usize {
        self.list_stack.len()
    }

    fn base_indent_cols(&self) -> usize {
        self.body_indent + self.list_depth().saturating_sub(1) * 2
    }

    /// Columns the line's leading margin consumes (base indent + blockquote
    /// gutters) — the budget that full-width raw lines (rules, code-fence
    /// borders, table grids) must subtract so they don't overflow `width` in
    /// list/quote contexts or at narrow widths.
    fn left_margin_cols(&self) -> usize {
        self.base_indent_cols() + self.quote_gutters.len() * 2
    }

    /// Leading spans for a freshly-finished line: lead marker (first line only)
    /// or indent spaces, blockquote gutters, then a pending list marker.
    fn leading(&mut self) -> Vec<Span<'static>> {
        let mut out: Vec<Span<'static>> = Vec::new();
        let base = self.base_indent_cols();
        // Continuation lines (after an item's first line) hang-indent under the
        // item text by the marker width; the first line carries the marker.
        let hang = match self.item_hang.last() {
            Some(&(w, true)) => w,
            _ => 0,
        };
        let indent = base + hang;
        if !self.first_line_emitted {
            self.first_line_emitted = true;
            if let Some(marker) = self.lead_marker.take() {
                out.push(marker);
                // Pad to `indent` from the marker's true display width, so a
                // width-2 glyph or a non-default body_indent still aligns the
                // first line with hang-indented continuation lines.
                let extra = indent.saturating_sub(self.lead_marker_width);
                if extra > 0 {
                    out.push(Span::raw(" ".repeat(extra)));
                }
            } else if indent > 0 {
                out.push(Span::raw(" ".repeat(indent)));
            }
        } else if indent > 0 {
            out.push(Span::raw(" ".repeat(indent)));
        }
        for gutter in &self.quote_gutters {
            out.push(Span::styled("│ ".to_string(), *gutter));
        }
        if let Some(marker) = self.pending_marker.take() {
            out.push(marker);
        }
        out
    }

    /// Finish the current logical line (content in `self.spans`).
    fn flush_line(&mut self) {
        let mut line_spans = self.leading();
        line_spans.append(&mut self.spans);
        self.lines.push(Line::from(line_spans));
        // The current item has now emitted at least one line; later lines are
        // continuations and hang-indent under the item text.
        if let Some(last) = self.item_hang.last_mut() {
            last.1 = true;
        }
    }

    /// Emit a fully-formed line (used for borders / rules that bypass inline
    /// accumulation), honoring the first-line lead marker + base indent. Any
    /// dangling inline content (e.g. a tight list item's text immediately
    /// followed by a nested block) is flushed first so it is never dropped.
    fn emit_raw_line(&mut self, content: Vec<Span<'static>>) {
        if !self.spans.is_empty() {
            self.flush_line();
        }
        self.spans = content;
        self.flush_line();
    }

    fn blank_line(&mut self) {
        self.lines.push(Line::from(String::new()));
    }

    /// Begin a block: flush any pending inline line (a tight list item's text
    /// before its nested block/sub-list), then insert a separating blank line
    /// when the previous block asked for one.
    fn block_gap(&mut self) {
        if !self.spans.is_empty() {
            self.flush_line();
        }
        if self.needs_gap && !self.lines.is_empty() {
            self.blank_line();
        }
        self.needs_gap = false;
    }

    fn push_style(&mut self, style: Style) {
        self.style_stack.push(self.cur_style);
        self.cur_style = self.cur_style.patch(style);
    }

    fn pop_style(&mut self) {
        if let Some(prev) = self.style_stack.pop() {
            self.cur_style = prev;
        }
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag) => self.end_tag(tag),
            Event::Text(text) => self.on_text(&text),
            Event::Code(code) => self.on_inline_code(&code),
            // Math is not enabled; render literally so nothing is dropped.
            Event::InlineMath(s) | Event::DisplayMath(s) => self.on_text(&s),
            Event::Html(html) => {
                // pulldown emits one HtmlBlock chunk per Event::Html, usually
                // newline-terminated; treat the trailing newline as a line
                // break so multi-line raw HTML keeps its line structure.
                let had_newline = html.ends_with('\n');
                self.on_text(html.trim_end_matches('\n'));
                if had_newline {
                    self.flush_line();
                }
            }
            Event::InlineHtml(html) => self.on_text(html.trim_end_matches('\n')),
            // Footnotes are intentionally not enabled (no ENABLE_FOOTNOTES), so
            // pulldown-cmark never emits this; explicit no-op for exhaustiveness,
            // mirroring the Tag/TagEnd::FootnoteDefinition no-ops.
            Event::FootnoteReference(_) => {}
            Event::SoftBreak | Event::HardBreak => {
                // Preserve authored line structure (matches the prior renderer);
                // downstream `Paragraph::wrap` still reflows over-long lines.
                if self.in_code {
                    self.code_buf.push('\n');
                } else {
                    self.flush_line();
                }
            }
            Event::Rule => self.on_rule(),
            Event::TaskListMarker(checked) => self.on_task_marker(checked),
        }
    }

    fn start_tag(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.block_gap(),
            Tag::Heading { level, .. } => {
                self.block_gap();
                let mut style = Style::default()
                    .fg(self.styles.heading())
                    .add_modifier(Modifier::BOLD);
                if matches!(level, HeadingLevel::H1) {
                    style = style.add_modifier(Modifier::UNDERLINED);
                }
                self.push_style(style);
            }
            Tag::BlockQuote(kind) => {
                self.block_gap();
                self.start_blockquote(kind);
            }
            Tag::CodeBlock(kind) => {
                self.block_gap();
                self.in_code = true;
                self.code_buf.clear();
                self.code_lang = match kind {
                    CodeBlockKind::Fenced(lang) => {
                        lang.split_whitespace().next().unwrap_or("").to_string()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
            }
            Tag::List(start) => {
                // An empty parent item whose first child is a nested list still
                // holds its bullet in `pending_marker`; emit that bare bullet now
                // so the nested item's marker does not overwrite (drop) it.
                if self.pending_marker.is_some() && self.spans.is_empty() {
                    self.flush_line();
                }
                self.block_gap();
                self.list_stack.push(start);
            }
            Tag::Item => {
                let marker = match self.list_stack.last_mut() {
                    Some(Some(n)) => {
                        let label = format!("{n}. ");
                        *n += 1;
                        Span::styled(label, Style::default().fg(self.styles.primary()))
                    }
                    _ => Span::styled("• ".to_string(), Style::default().fg(self.styles.primary())),
                };
                self.item_hang
                    .push((UnicodeWidthStr::width(marker.content.as_ref()), false));
                self.pending_marker = Some(marker);
            }
            Tag::Table(aligns) => {
                self.block_gap();
                self.table = Some(TableBuilder {
                    aligns,
                    header: Vec::new(),
                    rows: Vec::new(),
                    cur_row: Vec::new(),
                    cur_cell: String::new(),
                    in_head: false,
                });
            }
            Tag::TableHead => {
                if let Some(t) = self.table.as_mut() {
                    t.in_head = true;
                }
            }
            Tag::TableRow => {}
            Tag::TableCell => {
                if let Some(t) = self.table.as_mut() {
                    t.cur_cell.clear();
                }
            }
            Tag::Emphasis => self.push_style(Style::default().add_modifier(Modifier::ITALIC)),
            Tag::Strong => self.push_style(Style::default().add_modifier(Modifier::BOLD)),
            Tag::Strikethrough => self.push_style(
                Style::default()
                    .fg(self.styles.strikethrough())
                    .add_modifier(Modifier::CROSSED_OUT),
            ),
            Tag::Link { .. } => self.push_style(
                Style::default()
                    .fg(self.styles.hyperlink())
                    .add_modifier(Modifier::UNDERLINED),
            ),
            // Suppress image markup; alt text (Text events) renders inline.
            Tag::Image { .. } => self.push_style(Style::default()),
            // Not enabled (math/deflist/super-sub/footnote/html-block/metadata);
            // arms exist because pulldown enums are exhaustive.
            Tag::Superscript | Tag::Subscript => self.push_style(Style::default()),
            Tag::HtmlBlock
            | Tag::FootnoteDefinition(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::MetadataBlock(_) => {}
        }
    }

    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush_line();
                self.needs_gap = true;
            }
            TagEnd::Heading(_) => {
                self.pop_style();
                self.flush_line();
                self.needs_gap = true;
            }
            TagEnd::BlockQuote(_) => {
                self.quote_gutters.pop();
                self.needs_gap = true;
            }
            TagEnd::CodeBlock => self.finish_code_block(),
            TagEnd::List(_) => {
                self.list_stack.pop();
                self.needs_gap = true;
            }
            TagEnd::Item => {
                // Flush any tight-list item content that did not close a block.
                if !self.spans.is_empty() || self.pending_marker.is_some() {
                    self.flush_line();
                }
                self.item_hang.pop();
            }
            TagEnd::Table => self.finish_table(),
            TagEnd::TableHead => {
                if let Some(t) = self.table.as_mut() {
                    t.header = std::mem::take(&mut t.cur_row);
                    t.in_head = false;
                }
            }
            TagEnd::TableRow => {
                if let Some(t) = self.table.as_mut()
                    && !t.in_head
                {
                    let row = std::mem::take(&mut t.cur_row);
                    t.rows.push(row);
                }
            }
            TagEnd::TableCell => {
                if let Some(t) = self.table.as_mut() {
                    let cell = std::mem::take(&mut t.cur_cell);
                    t.cur_row.push(cell);
                }
            }
            TagEnd::Emphasis
            | TagEnd::Strong
            | TagEnd::Strikethrough
            | TagEnd::Link
            | TagEnd::Image
            | TagEnd::Superscript
            | TagEnd::Subscript => self.pop_style(),
            TagEnd::HtmlBlock
            | TagEnd::FootnoteDefinition
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::MetadataBlock(_) => {}
        }
    }

    fn on_text(&mut self, text: &str) {
        if self.in_code {
            self.code_buf.push_str(text);
            return;
        }
        if let Some(t) = self.table.as_mut() {
            t.cur_cell.push_str(text);
            return;
        }
        if text.is_empty() {
            return;
        }
        self.spans
            .push(Span::styled(text.to_string(), self.cur_style));
    }

    fn on_inline_code(&mut self, code: &str) {
        if let Some(t) = self.table.as_mut() {
            t.cur_cell.push_str(code);
            return;
        }
        // Inline code keeps the accent foreground but preserves surrounding
        // inline modifiers (bold/italic/strikethrough/link) via patch.
        self.spans.push(Span::styled(
            code.to_string(),
            self.cur_style
                .patch(Style::default().fg(self.styles.accent())),
        ));
    }

    fn on_task_marker(&mut self, checked: bool) {
        // The checkbox IS the list marker for a task item — drop the bullet so
        // it does not render as a redundant "• ☐".
        self.pending_marker = None;
        let (glyph, color) = if checked {
            ("☑ ", self.styles.success())
        } else {
            ("☐ ", self.styles.dim())
        };
        self.spans
            .push(Span::styled(glyph.to_string(), Style::default().fg(color)));
    }

    fn on_rule(&mut self) {
        self.block_gap();
        let dashes = (self.width as usize)
            .saturating_sub(self.left_margin_cols())
            .clamp(1, 80);
        self.emit_raw_line(vec![Span::styled(
            "─".repeat(dashes),
            Style::default().fg(self.styles.hr()),
        )]);
        self.needs_gap = true;
    }

    fn start_blockquote(&mut self, kind: Option<BlockQuoteKind>) {
        let gutter_style = match kind {
            Some(k) => {
                let (label, color) = alert_label(k, self.styles);
                // Alert header line above the quoted body.
                self.emit_raw_line(vec![Span::styled(
                    label.to_string(),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                )]);
                Style::default().fg(color)
            }
            None => Style::default().fg(self.styles.blockquote()),
        };
        self.quote_gutters.push(gutter_style);
    }

    fn finish_code_block(&mut self) {
        self.in_code = false;
        let code = std::mem::take(&mut self.code_buf);
        let lang = std::mem::take(&mut self.code_lang);

        // ```mermaid fences render as box-drawing cells when the `mermaid`
        // feature is on and the diagram is a supported, legible box-and-arrow
        // graph; otherwise fall through to the verbatim code-fence path.
        #[cfg(feature = "mermaid")]
        if !self.streaming
            && lang.eq_ignore_ascii_case("mermaid")
            && let Some(diagram) =
                coco_tui_mermaid::mermaid_to_lines(&code, self.styles, self.width)
        {
            for line in diagram {
                self.emit_raw_line(line.spans);
            }
            self.needs_gap = true;
            return;
        }

        let border_color = Style::default().fg(self.styles.border());
        // Optional themeable background fill behind the fence body. Folded into
        // the gutter, the body text, the highlighted spans, and a right-pad so
        // the block reads as a contiguous rectangle. `None` (the default) is a
        // no-op.
        let bg = self.styles.code_bg();
        let mut gutter = Style::default().fg(self.styles.border());
        let mut body_style = Style::default().fg(self.styles.text());
        if let Some(c) = bg {
            gutter = gutter.bg(c);
            body_style = body_style.bg(c);
        }

        // Budget = width − left margin − 2 box corners, so the border (and the
        // bg-padded body, which fills to the same width) never overflows in
        // list/quote contexts or at narrow widths.
        let border_len = (self.width as usize)
            .saturating_sub(self.left_margin_cols() + 2)
            .clamp(1, 60);
        // Top border with an optional, width-clamped language label so the top
        // dash count always equals the bottom border width.
        let top = if lang.is_empty() {
            format!("┌{}┐", "─".repeat(border_len))
        } else {
            let label_lang =
                coco_tui_ui::truncate::truncate_to_width(&lang, border_len.saturating_sub(3));
            let label = format!("─ {label_lang} ");
            let fill = border_len.saturating_sub(label.width());
            format!("┌{label}{}┐", "─".repeat(fill))
        };
        self.emit_raw_line(vec![Span::styled(top, border_color)]);

        let highlighted = highlight::highlight_code(&code, &lang, self.styles, self.syntax);
        let code_lines: Vec<&str> = code.split('\n').collect();
        // Drop a trailing empty element from the final newline.
        let line_count = if code.ends_with('\n') {
            code_lines.len().saturating_sub(1)
        } else {
            code_lines.len()
        };
        for (i, code_line) in code_lines.iter().take(line_count).enumerate() {
            let mut spans = vec![Span::styled("│ ".to_string(), gutter)];
            match highlighted.as_ref().and_then(|h| h.get(i)) {
                Some(hspans) if !hspans.is_empty() => match bg {
                    Some(c) => spans.extend(
                        hspans
                            .iter()
                            .map(|s| Span::styled(s.content.clone(), s.style.bg(c))),
                    ),
                    None => spans.extend(hspans.iter().cloned()),
                },
                _ => spans.push(Span::styled((*code_line).to_string(), body_style)),
            }
            if let Some(c) = bg {
                let used = coco_tui_ui::truncate::display_width(code_line);
                if used < border_len {
                    spans.push(Span::styled(
                        " ".repeat(border_len - used),
                        Style::default().bg(c),
                    ));
                }
            }
            self.emit_raw_line(spans);
        }
        self.emit_raw_line(vec![Span::styled(
            format!("└{}┘", "─".repeat(border_len)),
            border_color,
        )]);
        self.needs_gap = true;
    }

    fn finish_table(&mut self) {
        let Some(table) = self.table.take() else {
            return;
        };
        let col_count = table
            .header
            .len()
            .max(table.rows.iter().map(Vec::len).max().unwrap_or(0));
        if col_count == 0 {
            return;
        }
        // Column widths capped so the whole grid fits the body width budget.
        let budget = (self.width as usize).saturating_sub(self.left_margin_cols() + col_count + 1);
        let max_col = (budget / col_count).clamp(3, 40);
        let mut widths = vec![0usize; col_count];
        let measure = |cells: &[String], widths: &mut Vec<usize>| {
            for (i, cell) in cells.iter().enumerate() {
                if i < widths.len() {
                    widths[i] = widths[i].max(cell.width().min(max_col));
                }
            }
        };
        measure(&table.header, &mut widths);
        for row in &table.rows {
            measure(row, &mut widths);
        }

        let border = Style::default().fg(self.styles.table_border());
        self.emit_raw_line(vec![Span::styled(
            table_rule(&widths, '┌', '┬', '┐'),
            border,
        )]);
        if !table.header.is_empty() {
            self.emit_table_row(&table.header, &widths, &table.aligns, true);
            self.emit_raw_line(vec![Span::styled(
                table_rule(&widths, '├', '┼', '┤'),
                border,
            )]);
        }
        for row in &table.rows {
            self.emit_table_row(row, &widths, &table.aligns, false);
        }
        self.emit_raw_line(vec![Span::styled(
            table_rule(&widths, '└', '┴', '┘'),
            border,
        )]);
        self.needs_gap = true;
    }

    fn emit_table_row(
        &mut self,
        cells: &[String],
        widths: &[usize],
        aligns: &[Alignment],
        header: bool,
    ) {
        let border = Style::default().fg(self.styles.table_border());
        let text_style = if header {
            Style::default()
                .fg(self.styles.table_header())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let mut spans = vec![Span::styled("│".to_string(), border)];
        for (i, width) in widths.iter().enumerate() {
            let raw = cells.get(i).map(String::as_str).unwrap_or("");
            let cell = pad_cell(
                raw,
                *width,
                aligns.get(i).copied().unwrap_or(Alignment::None),
            );
            spans.push(Span::styled(format!(" {cell} "), text_style));
            spans.push(Span::styled("│".to_string(), border));
        }
        self.emit_raw_line(spans);
    }
}

fn alert_label(kind: BlockQuoteKind, styles: UiStyles<'_>) -> (&'static str, Color) {
    match kind {
        BlockQuoteKind::Note => ("▲ NOTE", styles.primary()),
        BlockQuoteKind::Tip => ("▲ TIP", styles.success()),
        BlockQuoteKind::Important => ("▲ IMPORTANT", styles.accent()),
        BlockQuoteKind::Warning => ("▲ WARNING", styles.warning()),
        BlockQuoteKind::Caution => ("▲ CAUTION", styles.error()),
    }
}

fn table_rule(widths: &[usize], left: char, mid: char, right: char) -> String {
    let mut s = String::new();
    s.push(left);
    for (i, w) in widths.iter().enumerate() {
        if i > 0 {
            s.push(mid);
        }
        s.push_str(&"─".repeat(w + 2));
    }
    s.push(right);
    s
}

fn pad_cell(text: &str, width: usize, align: Alignment) -> String {
    // Truncate via the canonical width-aware helper (one source of truth), then
    // ALWAYS re-pad to exactly `width` columns: truncation can land at width-1
    // for wide (CJK/emoji) graphemes, which would otherwise leave one row a
    // column short and misalign the table's right border.
    let truncated;
    let text = if text.width() > width {
        truncated = coco_tui_ui::truncate::truncate_to_width(text, width);
        truncated.as_str()
    } else {
        text
    };
    let pad = width.saturating_sub(text.width());
    match align {
        Alignment::Right => format!("{}{text}", " ".repeat(pad)),
        Alignment::Center => {
            let left = pad / 2;
            let right = pad - left;
            format!("{}{text}{}", " ".repeat(left), " ".repeat(right))
        }
        _ => format!("{text}{}", " ".repeat(pad)),
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
