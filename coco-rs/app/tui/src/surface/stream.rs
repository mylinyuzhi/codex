//! Native-surface live stream preparation.

use coco_tui_ui::style::UiStyles;
use ratatui::text::Line;

use crate::presentation::thinking::ThinkingDisplay;
use crate::presentation::thinking::ThinkingRenderInput;
use crate::presentation::thinking::estimate_reasoning_tokens;
use crate::presentation::thinking::render_thinking_block;
use crate::state::AppState;
use crate::streaming::render_controller::StreamRenderController;
use crate::streaming::render_controller::StreamRenderInput;
use crate::streaming::render_controller::StreamRenderKey;
use crate::streaming::render_controller::streaming_cursor_line;
use crate::surface::line_fingerprint::RenderedLineFingerprint;
use crate::surface::line_fingerprint::fingerprint_lines;
use crate::surface::modal::SurfaceFramePlan;
use crate::surface::viewport::build_live_tail_lines;
use crate::terminal::STREAMING_LIVE_TAIL_CAP;
use coco_tui_ui::engine::history_insert::HistoryRows;
use coco_tui_ui::engine::history_insert::render_history_rows;

#[derive(Debug, Default, Clone)]
pub(crate) struct SurfaceStreamDriver {
    controller: StreamRenderController,
    committed: Option<EmittedStreamPrefix>,
}

/// Everything known about the stream rows already inserted into native
/// scrollback: the watermark (how much source / how many lines) plus the
/// per-line fingerprints of exactly those lines. Bundled in one struct so the
/// fingerprint vector can never desync from the watermark it describes.
#[derive(Debug, Clone)]
struct EmittedStreamPrefix {
    watermark: StreamHistoryWatermark,
    line_fingerprints: Vec<RenderedLineFingerprint>,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct PreparedLiveTail {
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) stream_append: Option<PreparedStreamAppend>,
    pub(crate) render_key_invalidated: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedStreamAppend {
    pub(crate) rows: HistoryRows,
    pub(crate) prefix: PendingStreamPrefix,
    watermark: StreamHistoryWatermark,
}

impl PreparedStreamAppend {
    pub(crate) fn expected_rows(&self) -> u16 {
        self.rows.height()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingStreamPrefix {
    pub(crate) source_prefix: String,
    pub(crate) source_prefix_len: usize,
    pub(crate) line_prefix_len: usize,
    pub(crate) render_key: StreamRenderKey,
    /// Fingerprints of the `line_prefix_len` rendered lines already inserted
    /// into native scrollback, in order. Finalize verifies the committed
    /// whole-message render against these before appending only the suffix.
    pub(crate) line_fingerprints: Vec<RenderedLineFingerprint>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct StreamHistoryWatermark {
    source_len: usize,
    line_len: usize,
    render_key: StreamRenderKey,
}

impl SurfaceStreamDriver {
    pub(crate) fn prepare(
        &mut self,
        state: &AppState,
        width: u16,
        plan: SurfaceFramePlan,
    ) -> PreparedLiveTail {
        let styles = UiStyles::new(&state.ui.theme);
        if width == 0 || plan.finalized_history_in_viewport() {
            self.committed = None;
            return PreparedLiveTail {
                lines: build_live_tail_lines(state, styles, width, plan),
                stream_append: None,
                render_key_invalidated: false,
            };
        }

        let Some(streaming) = state.ui.streaming.as_ref() else {
            self.controller.clear();
            self.committed = None;
            return PreparedLiveTail::default();
        };

        let visible = streaming.visible_content();
        let render_key_input = StreamRenderInput {
            source: visible,
            styles,
            width,
            syntax_highlighting: state.ui.display_settings.syntax_highlighting,
        };
        let projection = self.controller.render_projection(render_key_input);
        let stable_line_len = projection.stable_lines.len();
        let emitted_valid = self.committed.as_ref().is_some_and(|emitted| {
            emitted.watermark.render_key == projection.render_key
                && emitted.watermark.source_len <= projection.stable_source_len
                && emitted.watermark.line_len <= stable_line_len
                && visible.get(..emitted.watermark.source_len).is_some()
        });
        let emitted_rows_invalidated =
            self.committed.is_some() && !emitted_valid && projection.render_key_invalidated;
        let (emitted_line_len, emitted_source_len) = if emitted_valid {
            self.committed.as_ref().map_or((0, 0), |emitted| {
                (emitted.watermark.line_len, emitted.watermark.source_len)
            })
        } else {
            (0, 0)
        };

        let mut stream_append = None;
        let live_start_line = if stable_line_len > emitted_line_len
            && projection.stable_source_len > emitted_source_len
        {
            let append_rows_started = std::time::Instant::now();
            let rows =
                render_history_rows(projection.stable_lines[emitted_line_len..].to_vec(), width);
            let append_rows_elapsed = append_rows_started.elapsed();
            if !rows.is_empty() {
                // Incremental prefix fingerprint: extend the fingerprints of
                // the already-emitted lines with the delta only — O(delta)
                // per advance instead of re-fingerprinting the full prefix.
                let fingerprint_started = std::time::Instant::now();
                let mut line_fingerprints = if emitted_valid {
                    self.committed
                        .as_ref()
                        .map_or_else(Vec::new, |emitted| emitted.line_fingerprints.clone())
                } else {
                    Vec::new()
                };
                debug_assert_eq!(line_fingerprints.len(), emitted_line_len);
                line_fingerprints.extend(fingerprint_lines(
                    &projection.stable_lines[emitted_line_len..],
                ));
                tracing::debug!(
                    target: "tui::streaming",
                    appended_lines = stable_line_len - emitted_line_len,
                    prefix_lines = stable_line_len,
                    source_prefix_bytes = projection.stable_source_len,
                    append_rows_us = append_rows_elapsed.as_micros(),
                    fingerprint_us = fingerprint_started.elapsed().as_micros(),
                    "stream stable append prepared",
                );
                let watermark = StreamHistoryWatermark {
                    source_len: projection.stable_source_len,
                    line_len: stable_line_len,
                    render_key: projection.render_key,
                };
                stream_append = Some(PreparedStreamAppend {
                    rows,
                    prefix: PendingStreamPrefix {
                        source_prefix: visible[..projection.stable_source_len].to_string(),
                        source_prefix_len: projection.stable_source_len,
                        line_prefix_len: stable_line_len,
                        render_key: projection.render_key,
                        line_fingerprints,
                    },
                    watermark,
                });
            }
            stable_line_len
        } else {
            emitted_line_len
        };

        // Display-cap (applied via the final `drain` below): when the user is
        // not scrolling, only the last `cap` markdown rows can survive, so skip
        // cloning rows that the drain would immediately discard.
        let cap = STREAMING_LIVE_TAIL_CAP as usize;
        let stable_tail = &projection.stable_lines[live_start_line..];
        let markdown_len = stable_tail.len() + projection.tail_lines.len();
        let markdown_skip = if state.ui.user_scrolled {
            0
        } else {
            markdown_len.saturating_sub(cap)
        };
        let mut lines: Vec<Line<'static>> =
            Vec::with_capacity(markdown_len.saturating_sub(markdown_skip) + 1);
        lines.extend(stable_tail.iter().skip(markdown_skip).cloned());
        lines.extend(
            projection
                .tail_lines
                .iter()
                .skip(markdown_skip.saturating_sub(stable_tail.len()))
                .cloned(),
        );
        if !lines.is_empty() {
            lines.push(streaming_cursor_line(styles));
        }
        if state.ui.show_thinking && !streaming.thinking.is_empty() {
            lines.extend(render_thinking_block(
                ThinkingRenderInput {
                    content: "",
                    duration_ms: None,
                    reasoning_tokens: Some(estimate_reasoning_tokens(&streaming.thinking)),
                    toggle_hint: None,
                    display: ThinkingDisplay::Collapsed,
                },
                styles,
            ));
        }

        // Display-cap the streaming tail to a constant height so the inline
        // viewport stops growing (then collapsing) with streamed content. Under
        // Policy A, capped leading rows remain in streaming source state but are
        // not native-scrollable until the finalized assistant message appends.
        // Skipped while the user is scrolling so they can read the full
        // in-flight tail.
        if !state.ui.user_scrolled && lines.len() > cap {
            lines.drain(0..lines.len() - cap);
        }

        if emitted_rows_invalidated {
            self.committed = None;
        }
        PreparedLiveTail {
            lines,
            stream_append,
            render_key_invalidated: emitted_rows_invalidated,
        }
    }

    pub(crate) fn mark_stream_append_committed(&mut self, append: &PreparedStreamAppend) {
        self.committed = Some(EmittedStreamPrefix {
            watermark: append.watermark,
            line_fingerprints: append.prefix.line_fingerprints.clone(),
        });
    }

    pub(crate) fn reset(&mut self) {
        self.controller.clear();
        self.committed = None;
    }
}

#[cfg(test)]
#[path = "stream.test.rs"]
mod tests;
