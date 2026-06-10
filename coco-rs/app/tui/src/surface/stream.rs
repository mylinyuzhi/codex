//! Native-surface live stream preparation.

use coco_tui_ui::style::UiStyles;
use ratatui::text::Line;

use crate::presentation::thinking::ThinkingDisplay;
use crate::presentation::thinking::ThinkingRenderInput;
use crate::presentation::thinking::estimate_reasoning_tokens;
use crate::presentation::thinking::render_thinking_block;
use crate::state::AppState;
use crate::surface::modal::SurfaceFramePlan;
use crate::surface::viewport::build_live_tail_lines;
use crate::terminal::STREAMING_LIVE_TAIL_CAP;
use crate::transcript::stream::PendingStreamPrefix;
use crate::transcript::stream::StreamHistoryWatermark;
use crate::transcript::stream::StreamRenderController;
use crate::transcript::stream::StreamRenderInput;
use crate::transcript::stream::streaming_cursor_line;
use coco_tui_ui::engine::history_insert::HistoryRows;
use coco_tui_ui::engine::history_insert::render_history_rows;

#[derive(Debug, Default, Clone)]
pub(crate) struct SurfaceStreamDriver {
    controller: StreamRenderController,
    /// Watermark of the stream rows already inserted into native scrollback
    /// (`None` until the first mid-stream stable commit). The anchored finalize
    /// (`transcript::emission::finalize_after_stream_prefix`) re-proves
    /// agreement at the SOURCE level, so no per-row fingerprint is retained —
    /// soundness rests on the markdown prefix-stability property pinned in
    /// `transcript::stream` tests.
    committed: Option<StreamHistoryWatermark>,
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
}

impl PreparedStreamAppend {
    pub(crate) fn expected_rows(&self) -> u16 {
        self.rows.height()
    }
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
        // A controller reset (`render_key_invalidated`: the source was
        // replaced or the render key changed) means a surviving watermark
        // describes rows of a DIFFERENT document. Event coalescing can hide a
        // turn boundary from this driver — `MessageAppended` and the next
        // turn's first deltas can fold into one draw, so this prepare never
        // observes the `streaming == None` gap that normally clears the
        // watermark. A length-only check could then falsely re-validate the
        // previous turn's watermark against the new turn's stable region and
        // silently skip the new turn's leading rows; identity, not size, is
        // the gate.
        let emitted_valid = !projection.render_key_invalidated
            && self.committed.is_some_and(|wm| {
                wm.render_key == projection.render_key
                    && wm.source_len <= projection.stable_source_len
                    && wm.line_len <= stable_line_len
                    && visible.get(..wm.source_len).is_some()
            });
        let emitted_rows_invalidated = self.committed.is_some() && !emitted_valid;
        let (emitted_line_len, emitted_source_len) = if emitted_valid {
            self.committed
                .map_or((0, 0), |wm| (wm.line_len, wm.source_len))
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
                tracing::debug!(
                    target: "tui::streaming",
                    appended_lines = stable_line_len - emitted_line_len,
                    prefix_lines = stable_line_len,
                    source_prefix_bytes = projection.stable_source_len,
                    append_rows_us = append_rows_elapsed.as_micros(),
                    "stream stable append prepared",
                );
                stream_append = Some(PreparedStreamAppend {
                    rows,
                    prefix: PendingStreamPrefix {
                        source_prefix: visible[..projection.stable_source_len].to_string(),
                        line_prefix_len: stable_line_len,
                        render_key: projection.render_key,
                    },
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
        self.committed = Some(append.prefix.watermark());
    }

    pub(crate) fn reset(&mut self) {
        self.controller.clear();
        self.committed = None;
    }
}

#[cfg(test)]
#[path = "stream.test.rs"]
mod tests;
