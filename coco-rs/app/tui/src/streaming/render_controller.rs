//! Source-backed renderer for the active assistant stream.
//!
//! Streaming deltas append raw source quickly, but repaint cadence can be much
//! higher than semantic changes. This controller keeps newline-terminated
//! blank-line-delimited source that is unlikely to change as rendered `Line`s
//! and only re-renders the mutable tail.

use std::hash::Hash;
use std::hash::Hasher;

use coco_tui_ui::display::SyntaxHighlighting;
use coco_tui_ui::style::UiStyles;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

use crate::widgets::chat::assistant_stream_lead_marker;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamRegion {
    Stable,
    MutableTail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum StreamRenderMode {
    FinalizedStable,
    StreamingMutableTail,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct StreamRenderKey(u64);

impl StreamRenderKey {
    pub(crate) fn new(input: StreamRenderInput<'_>, mode: StreamRenderMode) -> Self {
        let opts = markdown_options(input, mode);
        let mut h = std::collections::hash_map::DefaultHasher::new();
        opts.width.hash(&mut h);
        opts.syntax.is_enabled().hash(&mut h);
        opts.body_indent.hash(&mut h);
        opts.streaming.hash(&mut h);
        input.styles.theme_hash().hash(&mut h);
        Self(h.finish())
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct StreamRenderInput<'a> {
    pub(crate) source: &'a str,
    pub(crate) styles: UiStyles<'a>,
    pub(crate) width: u16,
    pub(crate) syntax_highlighting: SyntaxHighlighting,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct StreamRenderController {
    render_key: Option<StreamRenderKey>,
    source: String,
    stable_source_end: usize,
    stable_lines: Vec<Line<'static>>,
    appended_stable_source_end: usize,
    appended_stable_line_count: usize,
    tail_source_start: usize,
    tail_source: String,
    tail_lines: Vec<Line<'static>>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct StreamLiveRender {
    pub(crate) stable_append_source: String,
    pub(crate) stable_append_lines: Vec<Line<'static>>,
    pub(crate) live_tail_lines: Vec<Line<'static>>,
    pub(crate) render_reset: bool,
}

impl StreamRenderController {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn render(&mut self, input: StreamRenderInput<'_>) -> Vec<Line<'static>> {
        self.render_live_frame(input).live_tail_lines
    }

    pub(crate) fn render_live_frame(&mut self, input: StreamRenderInput<'_>) -> StreamLiveRender {
        if input.source.is_empty() {
            self.clear();
            return StreamLiveRender::default();
        }

        let render_key = StreamRenderKey::new(input, StreamRenderMode::FinalizedStable);
        let render_reset =
            self.render_key != Some(render_key) || !input.source.starts_with(&self.source);
        if render_reset {
            self.reset_for_key(render_key, input.source);
        } else {
            self.source.push_str(&input.source[self.source.len()..]);
        }

        let stable_end = stable_source_end(&self.source);
        if stable_end > self.stable_source_end {
            let source = self.source[self.stable_source_end..stable_end].to_string();
            let region = StreamRegion::Stable;
            let mut rendered =
                render_markdown_region(&source, input, region, self.stable_lines.is_empty());
            self.stable_lines.append(&mut rendered);
            self.stable_source_end = stable_end;
        }

        let tail_source = &self.source[self.stable_source_end..];
        if self.tail_source_start != self.stable_source_end || self.tail_source != tail_source {
            self.tail_source_start = self.stable_source_end;
            self.tail_source.clear();
            self.tail_source.push_str(tail_source);
            self.tail_lines = render_markdown_region(
                &self.tail_source,
                input,
                StreamRegion::MutableTail,
                self.stable_lines.is_empty(),
            );
        }

        let stable_append_lines = self.stable_lines[self.appended_stable_line_count..].to_vec();
        let stable_append_source =
            self.source[self.appended_stable_source_end..self.stable_source_end].to_string();
        let mut live_tail_lines = Vec::with_capacity(
            self.stable_lines
                .len()
                .saturating_sub(self.appended_stable_line_count)
                + self.tail_lines.len(),
        );
        live_tail_lines.extend(
            self.stable_lines[self.appended_stable_line_count..]
                .iter()
                .cloned(),
        );
        live_tail_lines.extend(self.tail_lines.iter().cloned());
        StreamLiveRender {
            stable_append_source,
            stable_append_lines,
            live_tail_lines,
            render_reset,
        }
    }

    pub(crate) fn mark_stable_appended(&mut self) {
        self.appended_stable_source_end = self.stable_source_end;
        self.appended_stable_line_count = self.stable_lines.len();
    }

    pub(crate) fn forget_stable_appended(&mut self) {
        self.appended_stable_source_end = 0;
        self.appended_stable_line_count = 0;
    }

    pub(crate) fn render_key(&self) -> Option<StreamRenderKey> {
        self.render_key
    }

    fn reset_for_key(&mut self, render_key: StreamRenderKey, source: &str) {
        self.render_key = Some(render_key);
        self.source.clear();
        self.source.push_str(source);
        self.stable_source_end = 0;
        self.stable_lines.clear();
        self.appended_stable_source_end = 0;
        self.appended_stable_line_count = 0;
        self.tail_source_start = 0;
        self.tail_source.clear();
        self.tail_lines.clear();
    }

    pub(crate) fn clear(&mut self) {
        self.render_key = None;
        self.source.clear();
        self.stable_source_end = 0;
        self.stable_lines.clear();
        self.appended_stable_source_end = 0;
        self.appended_stable_line_count = 0;
        self.tail_source_start = 0;
        self.tail_source.clear();
        self.tail_lines.clear();
    }
}

fn markdown_options(
    input: StreamRenderInput<'_>,
    mode: StreamRenderMode,
) -> coco_tui_markdown::MarkdownOptions<'_> {
    let opts = coco_tui_markdown::MarkdownOptions::new(
        input.styles,
        input.width,
        input.syntax_highlighting,
    );
    match mode {
        StreamRenderMode::FinalizedStable => opts,
        StreamRenderMode::StreamingMutableTail => opts.streaming(),
    }
}

fn render_markdown_region(
    source: &str,
    input: StreamRenderInput<'_>,
    region: StreamRegion,
    include_marker: bool,
) -> Vec<Line<'static>> {
    if source.is_empty() {
        return Vec::new();
    }
    let mode = match region {
        StreamRegion::Stable => StreamRenderMode::FinalizedStable,
        StreamRegion::MutableTail => StreamRenderMode::StreamingMutableTail,
    };
    let opts = markdown_options(input, mode);
    let marker = include_marker.then(|| assistant_stream_lead_marker(input.styles));
    tracing::trace!(
        target: "tui::streaming",
        region = ?region,
        source_bytes = source.len(),
        width = input.width,
        "render streaming markdown region",
    );
    coco_tui_markdown::render_markdown(source, opts, marker.as_ref())
}

fn stable_source_end(source: &str) -> usize {
    let Some(scan_end) = source.rfind('\n').map(|idx| idx + 1) else {
        return 0;
    };

    let mut offset = 0usize;
    let mut safe_end = 0usize;
    let mut fence_open: Option<FenceMarker> = None;
    for line in source[..scan_end].split_inclusive('\n') {
        let trimmed = line.trim();
        let mut closed_fence = false;
        if let Some(marker) = fence_marker(trimmed) {
            match fence_open {
                Some(open) if marker.closes(open) => {
                    fence_open = None;
                    closed_fence = true;
                }
                None => {
                    fence_open = Some(marker);
                }
                Some(_) => {}
            }
        }

        offset += line.len();
        if fence_open.is_none()
            && (trimmed.is_empty() || closed_fence || atx_heading_marker(trimmed))
            && stable_prefix_is_context_free(&source[..offset])
        {
            safe_end = offset;
        }
    }

    safe_end
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FenceMarker {
    ch: char,
    len: usize,
}

impl FenceMarker {
    fn closes(self, open: Self) -> bool {
        self.ch == open.ch && self.len >= open.len
    }
}

fn fence_marker(trimmed: &str) -> Option<FenceMarker> {
    let mut chars = trimmed.chars();
    let ch = chars.next()?;
    if ch != '`' && ch != '~' {
        return None;
    }
    let len = trimmed
        .chars()
        .take_while(|candidate| *candidate == ch)
        .count();
    (len >= 3).then_some(FenceMarker { ch, len })
}

fn atx_heading_marker(trimmed: &str) -> bool {
    let marker_len = trimmed.chars().take_while(|ch| *ch == '#').count();
    (1..=6).contains(&marker_len)
        && trimmed
            .chars()
            .nth(marker_len)
            .is_none_or(char::is_whitespace)
}

fn stable_prefix_is_context_free(prefix: &str) -> bool {
    // Link reference definitions are global in CommonMark, so later stream
    // bytes can change earlier shortcut/collapsed reference links. Hold any
    // bracketed prefix in the mutable tail rather than proving every variant.
    // Brackets inside fenced code are literal text and cannot be reinterpreted
    // by a later reference definition.
    let mut fence_open: Option<FenceMarker> = None;
    for line in prefix.split_inclusive('\n') {
        let trimmed = line.trim();
        if let Some(marker) = fence_marker(trimmed) {
            match fence_open {
                Some(open) if marker.closes(open) => {
                    fence_open = None;
                }
                None => {
                    fence_open = Some(marker);
                }
                Some(_) => {}
            }
            continue;
        }
        if fence_open.is_none() && line.contains('[') {
            return false;
        }
    }
    true
}

pub(crate) fn streaming_cursor_line(styles: UiStyles<'_>) -> Line<'static> {
    Line::from(Span::raw("▌").fg(styles.accent()))
}

#[cfg(test)]
#[path = "render_controller.test.rs"]
mod tests;
