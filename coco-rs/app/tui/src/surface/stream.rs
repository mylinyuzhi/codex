//! Native-surface live stream preparation and provisional history append.

use coco_tui_ui::style::UiStyles;
use ratatui::text::Line;
use sha2::Digest;
use sha2::Sha256;

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

#[derive(Debug, Default, Clone)]
pub(crate) struct SurfaceStreamDriver {
    controller: StreamRenderController,
    committed_prefix_digest: Sha256,
    pending_append_source: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct PreparedLiveTail {
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) stable_append: Option<ProvisionalStableAppend>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProvisionalStableAppend {
    pub(crate) prior_prefix_digest: [u8; 32],
    pub(crate) prefix_digest: [u8; 32],
    pub(crate) append_source: String,
    pub(crate) append_lines: Vec<Line<'static>>,
    pub(crate) append_line_fingerprints: Vec<RenderedLineFingerprint>,
    pub(crate) render_key: StreamRenderKey,
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
            self.pending_append_source = None;
            return PreparedLiveTail {
                lines: build_live_tail_lines(state, styles, width, plan),
                stable_append: None,
            };
        }

        let Some(streaming) = state.ui.streaming.as_ref() else {
            self.controller.render(StreamRenderInput {
                source: "",
                styles,
                width,
                syntax_highlighting: state.ui.display_settings.syntax_highlighting,
            });
            self.committed_prefix_digest = Sha256::new();
            self.pending_append_source = None;
            return PreparedLiveTail::default();
        };

        let visible = streaming.visible_content();
        let render_key_input = StreamRenderInput {
            source: visible,
            styles,
            width,
            syntax_highlighting: state.ui.display_settings.syntax_highlighting,
        };
        let frame = self.controller.render_live_frame(render_key_input);
        if frame.render_reset {
            self.committed_prefix_digest = Sha256::new();
        }
        let render_key = self.controller.render_key();
        self.pending_append_source = None;
        let stable_append = (!frame.stable_append_lines.is_empty()).then(|| {
            let prior_prefix_digest = digest_state(&self.committed_prefix_digest);
            let mut next = self.committed_prefix_digest.clone();
            next.update(frame.stable_append_source.as_bytes());
            let prefix_digest = digest_state(&next);
            self.pending_append_source = Some(frame.stable_append_source.clone());
            ProvisionalStableAppend {
                prior_prefix_digest,
                prefix_digest,
                append_source: frame.stable_append_source,
                append_line_fingerprints: fingerprint_lines(&frame.stable_append_lines),
                append_lines: frame.stable_append_lines.clone(),
                render_key: render_key.unwrap_or_default(),
            }
        });

        let mut lines = if stable_append.is_some() {
            frame.live_tail_lines[frame.stable_append_lines.len()..].to_vec()
        } else {
            frame.live_tail_lines
        };
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
        // viewport stops growing (then collapsing) with streamed content — the
        // residual content-region flicker. Keep the LAST `CAP` rows (the newest
        // text + cursor + collapsed-thinking, all appended at the bottom); the
        // dropped leading rows already live in `streaming.visible_content()` and
        // reach native scrollback via `stable_append` at the next markdown
        // boundary and definitively at finalize, so nothing is lost. This is a
        // VIEW cap only — the markdown commit boundary above is untouched, so a
        // streaming code fence/list is never split. Skipped while the user is
        // scrolling so they can read the full in-flight tail.
        let cap = STREAMING_LIVE_TAIL_CAP as usize;
        if !state.ui.user_scrolled && lines.len() > cap {
            lines.drain(0..lines.len() - cap);
        }

        PreparedLiveTail {
            lines,
            stable_append,
        }
    }

    pub(crate) fn mark_stable_appended(&mut self) {
        if let Some(source) = self.pending_append_source.take() {
            self.committed_prefix_digest.update(source.as_bytes());
        }
        self.controller.mark_stable_appended();
    }

    pub(crate) fn forget_stable_appended(&mut self) {
        self.committed_prefix_digest = Sha256::new();
        self.pending_append_source = None;
        self.controller.forget_stable_appended();
    }

    pub(crate) fn reset(&mut self) {
        self.controller.clear();
        self.committed_prefix_digest = Sha256::new();
        self.pending_append_source = None;
    }
}

fn digest_state(state: &Sha256) -> [u8; 32] {
    state.clone().finalize().into()
}

#[cfg(test)]
#[path = "stream.test.rs"]
mod tests;
