//! Source-backed renderer for the active assistant stream.
//!
//! Streaming deltas append raw source quickly, but repaint cadence can be much
//! higher than semantic changes. This controller asks `coco-tui-markdown` for a
//! conservative stable source prefix, renders that full prefix authoritatively,
//! and only re-renders the mutable tail.

use std::hash::Hash;
use std::hash::Hasher;
use std::time::Instant;

use coco_tui_ui::display::SyntaxHighlighting;
use coco_tui_ui::style::UiStyles;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

use crate::widgets::chat::assistant_stream_lead_marker;
use crate::widgets::chat::render_assistant::CommittedAssistantMarkdownOptions;
use crate::widgets::chat::render_assistant::render_committed_assistant_markdown;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamRegion {
    MutableTail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum StreamRenderMode {
    CommittedStable,
    StreamingMutableTail,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct StreamRenderKey(u64);

impl StreamRenderKey {
    pub(crate) fn committed(input: StreamRenderInput<'_>) -> Self {
        Self::new(input, StreamRenderMode::CommittedStable)
    }

    fn new(input: StreamRenderInput<'_>, mode: StreamRenderMode) -> Self {
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

/// One frame's view of the stream render state, borrowing the controller's
/// cached line vectors. `stable_lines` is the authoritative committed-renderer
/// output for the stable source prefix; `tail_lines` is the mutable-tail
/// render. Consumers clone exactly the slices they need instead of receiving
/// (and re-cloning) a rebuilt concatenation every frame.
#[derive(Debug)]
pub(crate) struct StreamRenderProjection<'a> {
    pub(crate) stable_lines: &'a [Line<'static>],
    pub(crate) tail_lines: &'a [Line<'static>],
    pub(crate) stable_source_len: usize,
    pub(crate) render_key: StreamRenderKey,
    pub(crate) render_key_invalidated: bool,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct StreamRenderController {
    render_key: Option<StreamRenderKey>,
    source: String,
    stable_prefix_end: usize,
    stable_lines: Vec<Line<'static>>,
    tail_source_start: usize,
    tail_source: String,
    tail_lines: Vec<Line<'static>>,
}

impl StreamRenderController {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn render(&mut self, input: StreamRenderInput<'_>) -> Vec<Line<'static>> {
        let projection = self.render_projection(input);
        let mut lines =
            Vec::with_capacity(projection.stable_lines.len() + projection.tail_lines.len());
        lines.extend(projection.stable_lines.iter().cloned());
        lines.extend(projection.tail_lines.iter().cloned());
        lines
    }

    pub(crate) fn render_projection(
        &mut self,
        input: StreamRenderInput<'_>,
    ) -> StreamRenderProjection<'_> {
        if input.source.is_empty() {
            self.clear();
            return StreamRenderProjection {
                stable_lines: &[],
                tail_lines: &[],
                stable_source_len: 0,
                render_key: StreamRenderKey::default(),
                render_key_invalidated: false,
            };
        }

        let render_key = StreamRenderKey::committed(input);
        let render_reset =
            self.render_key != Some(render_key) || !input.source.starts_with(&self.source);
        if render_reset {
            self.reset_for_key(render_key, input.source);
        } else {
            self.source.push_str(&input.source[self.source.len()..]);
        }

        let stable_end = coco_tui_markdown::stable_prefix_end(&self.source);
        if stable_end > self.stable_prefix_end {
            self.stable_lines = render_committed_stable_region(&self.source[..stable_end], input);
            self.stable_prefix_end = stable_end;
        }

        let tail_source = &self.source[self.stable_prefix_end..];
        if self.tail_source_start != self.stable_prefix_end || self.tail_source != tail_source {
            self.tail_source_start = self.stable_prefix_end;
            self.tail_source.clear();
            self.tail_source.push_str(tail_source);
            self.tail_lines = render_markdown_region(
                &self.tail_source,
                input,
                StreamRegion::MutableTail,
                self.stable_lines.is_empty(),
            );
        }

        StreamRenderProjection {
            stable_lines: &self.stable_lines,
            tail_lines: &self.tail_lines,
            stable_source_len: self.stable_prefix_end,
            render_key,
            render_key_invalidated: render_reset,
        }
    }

    fn reset_for_key(&mut self, render_key: StreamRenderKey, source: &str) {
        self.render_key = Some(render_key);
        self.source.clear();
        self.source.push_str(source);
        self.stable_prefix_end = 0;
        self.stable_lines.clear();
        self.tail_source_start = 0;
        self.tail_source.clear();
        self.tail_lines.clear();
    }

    pub(crate) fn clear(&mut self) {
        self.render_key = None;
        self.source.clear();
        self.stable_prefix_end = 0;
        self.stable_lines.clear();
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
        StreamRenderMode::CommittedStable => opts,
        StreamRenderMode::StreamingMutableTail => opts.streaming(),
    }
}

fn render_committed_stable_region(
    source: &str,
    input: StreamRenderInput<'_>,
) -> Vec<Line<'static>> {
    if source.is_empty() {
        return Vec::new();
    }
    let started = Instant::now();
    let lines = render_committed_assistant_markdown(
        source,
        CommittedAssistantMarkdownOptions {
            styles: input.styles,
            width: input.width,
            syntax_highlighting: input.syntax_highlighting,
        },
    );
    let elapsed = started.elapsed();
    tracing::debug!(
        target: "tui::streaming",
        region = "stable",
        source_bytes = source.len(),
        lines = lines.len(),
        elapsed_us = elapsed.as_micros(),
        width = input.width,
        "render streaming markdown region",
    );
    lines
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
        StreamRegion::MutableTail => StreamRenderMode::StreamingMutableTail,
    };
    let opts = markdown_options(input, mode);
    let marker = include_marker.then(|| assistant_stream_lead_marker(input.styles));
    let started = Instant::now();
    let lines = coco_tui_markdown::render_markdown(source, opts, marker.as_ref());
    let elapsed = started.elapsed();
    match region {
        StreamRegion::MutableTail => tracing::trace!(
            target: "tui::streaming",
            region = ?region,
            source_bytes = source.len(),
            lines = lines.len(),
            elapsed_us = elapsed.as_micros(),
            width = input.width,
            "render streaming markdown region",
        ),
    }
    lines
}

pub(crate) fn streaming_cursor_line(styles: UiStyles<'_>) -> Line<'static> {
    Line::from(Span::raw("▌").fg(styles.accent()))
}

#[cfg(test)]
#[path = "render_controller.test.rs"]
mod tests;
