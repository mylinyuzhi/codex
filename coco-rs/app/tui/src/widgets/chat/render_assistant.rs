//! Assistant-side cell renderers — text (markdown), thinking
//! (collapsible), redacted thinking, tool-use invocation.
//!
//! Dispatches directly on `cell.kind` / `cell.source: Arc<Message>`.
//! All emitted lines are `Line<'static>` (owned spans).

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::Hash;
use std::hash::Hasher;

use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

use super::ChatWidget;
use crate::i18n::t;
use crate::presentation::thinking::ThinkingDisplay;
use crate::presentation::thinking::ThinkingRenderInput;
use crate::presentation::thinking::format_duration_seconds;
use crate::presentation::thinking::render_thinking_block;
use crate::state::transcript_view::CellKind;
use crate::state::transcript_view::RenderedCell;
use crate::tool_display::ToolNameTone;
use crate::tool_display::tool_name_tone;
use coco_tui_ui::constants;

/// Turn-boundary glyph at the start of each assistant text response.
/// TS `BLACK_CIRCLE` from `constants/figures.ts` picks `⏺` on macOS for
/// vertical alignment and `●` elsewhere; we standardise on `⏺` which
/// renders cleanly in modern Linux/macOS/Windows Terminal fonts and
/// keeps a consistent visual across platforms.
pub(crate) const ASSISTANT_DOT: &str = "⏺";

/// The shared turn-boundary marker for assistant text (finalized + streaming),
/// so both paths land the dot identically and the row cannot jump on finish.
pub(crate) fn assistant_lead_marker(color: ratatui::style::Color) -> coco_tui_markdown::LeadMarker {
    coco_tui_markdown::LeadMarker::new(ASSISTANT_DOT, color)
}

thread_local! {
    /// Content-addressed markdown memo for committed assistant text cells, keyed
    /// by a hash of every line-affecting render input (text + width + syntax +
    /// theme + body_indent + streaming). Keying on content rather than
    /// `message_uuid` is required for correctness: one assistant message derives
    /// into MULTIPLE `AssistantText` cells when its content interleaves blocks
    /// (Text / ToolCall / Text), and all of them share one `uuid` — a uuid-keyed
    /// map would make those sibling cells evict each other every frame, turning
    /// every render into a guaranteed miss.
    ///
    /// Reached on BOTH paths that route through `ChatWidget::build_lines_owned`:
    /// (1) native history emission — `render_finalized_history_lines` on
    /// reflow/resize/viewport changes plus once per binary-search suffix probe
    /// in `render_replay_history_lines`; and (2) the compatibility-fallback
    /// live-tail rebuild via `build_live_tail_lines`. It absorbs the repeated
    /// full-history suffix renders the replay binary search performs. Bounded so
    /// it can't grow without limit; because entries are content-keyed a stale
    /// one can never be served — at worst a removed message's entry is dead
    /// weight until the cap clear. It deliberately does NOT mirror the
    /// `reasoning_metadata` prune lifecycle (that exists for correctness, not
    /// memory).
    static COMMITTED_MD_MEMO: RefCell<HashMap<u64, Vec<Line<'static>>>> =
        RefCell::new(HashMap::new());
}

/// Soft cap on memo entries; cleared wholesale on overflow (cheap, rare).
const COMMITTED_MD_MEMO_CAP: usize = 4096;

#[cfg(any(test, feature = "testing"))]
pub(crate) fn clear_committed_markdown_memo_for_tests() {
    COMMITTED_MD_MEMO.with(|m| m.borrow_mut().clear());
}

fn render_assistant_text_memoized(w: &ChatWidget<'_>, text: &str) -> Vec<Line<'static>> {
    let opts = coco_tui_markdown::MarkdownOptions::new(w.styles, w.width, w.syntax_highlighting);
    let key = {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut h);
        opts.width.hash(&mut h);
        opts.syntax.is_enabled().hash(&mut h);
        opts.body_indent.hash(&mut h);
        opts.streaming.hash(&mut h);
        w.styles.theme_hash().hash(&mut h);
        h.finish()
    };
    if let Some(hit) = COMMITTED_MD_MEMO.with(|m| m.borrow().get(&key).cloned()) {
        return hit;
    }
    let marker = assistant_lead_marker(w.styles.assistant_message());
    let rendered = coco_tui_markdown::render_markdown(text, opts, Some(&marker));
    COMMITTED_MD_MEMO.with(|m| {
        let mut m = m.borrow_mut();
        if m.len() >= COMMITTED_MD_MEMO_CAP {
            m.clear();
        }
        m.insert(key, rendered.clone());
    });
    rendered
}

pub(super) fn try_render(
    w: &ChatWidget<'_>,
    cell: &RenderedCell,
    lines: &mut Vec<Line<'static>>,
) -> Option<()> {
    match &cell.kind {
        CellKind::AssistantText { text, .. } => {
            // TS parity: `AssistantTextMessage` renders the body with a
            // leading `BLACK_CIRCLE` turn marker on the first line. The
            // marker is a first-class renderer input (`LeadMarker`); the
            // renderer lands it at column 0 and keeps wrapped prose at the
            // body indent — no fragile first-span string-matching here.
            // Empty responses still get a marker-only line. Memoized by content
            // (see COMMITTED_MD_MEMO) so repeated history replays / fallback
            // rebuilds don't re-run pulldown + syntect.
            lines.extend(render_assistant_text_memoized(w, text));
            Some(())
        }
        CellKind::AssistantThinking { text } => {
            let meta = w
                .reasoning_metadata
                .and_then(|cache| cache.get(&cell.message_uuid));
            lines.extend(render_thinking_block(
                ThinkingRenderInput {
                    content: text,
                    duration_ms: meta.and_then(|m| m.duration_ms),
                    reasoning_tokens: meta.map(|m| m.reasoning_tokens),
                    toggle_hint: Some(&w.thinking_toggle_hint()),
                    display: if w.show_thinking {
                        ThinkingDisplay::Expanded {
                            max_body_lines: coco_tui_ui::constants::THINKING_PREVIEW_LINES,
                            truncated_hint: "…",
                        }
                    } else {
                        ThinkingDisplay::Collapsed
                    },
                },
                w.styles,
            ));
            Some(())
        }
        CellKind::AssistantRedactedThinking => {
            // ✻ (teardrop asterisk) signals "still thinking" — TS uses
            // this glyph for the redacted/in-flight variant so users
            // can tell at a glance the block isn't finalized.
            lines.push(Line::from(
                Span::raw(t!("chat.redacted_thinking").to_string())
                    .fg(w.styles.thinking())
                    .dim()
                    .italic(),
            ));
            Some(())
        }
        CellKind::ToolUse { call_id, tool_name } => {
            let input_preview = crate::state::derive::tool_call_header_preview_model(
                &cell.source,
                call_id,
                tool_name,
            );
            let preview_spans = crate::tool_display::render_tool_input_preview_spans(
                &input_preview,
                w.styles,
                w.syntax_highlighting,
                constants::TOOL_DESCRIPTION_MAX_CHARS as usize,
            );
            // Elapsed time badge: `(250ms)` / `(1.2s)` / `(3m 4s)`
            // tail-aligned after the preview. Sourced from the
            // matching ToolExecution by call_id so running tools tick
            // forward via SpinnerTick redraws and completed tools
            // freeze at their final duration.
            let elapsed_badge = w
                .tool_executions
                .iter()
                .find(|t| t.call_id == *call_id)
                .map(|t| format!(" ({})", format_duration_seconds(t.elapsed())))
                .unwrap_or_default();
            // Width-1 `●` in the tool-type color so the marker aligns with the
            // assistant `⏺` and the result `└` gutter at column 2 — an emoji
            // (`🔧`) is width-2 and its cell width is font-dependent, so it
            // drifts the whole row out of the gutter.
            let tone = tool_tone_color(tool_name_tone(tool_name), w.styles);
            let mut spans = vec![
                Span::raw("● ").fg(tone),
                Span::raw(tool_name.clone()).fg(tone).bold(),
            ];
            if !preview_spans.is_empty() {
                spans.push(Span::raw("(").fg(w.styles.text()));
                spans.extend(preview_spans);
                spans.push(Span::raw(")").fg(w.styles.text()));
            }
            spans.push(Span::raw(elapsed_badge).fg(w.styles.dim()).dim());
            lines.push(Line::from(spans));
            Some(())
        }
        _ => None,
    }
}

fn tool_tone_color(
    tone: ToolNameTone,
    styles: coco_tui_ui::style::UiStyles<'_>,
) -> ratatui::style::Color {
    match tone {
        ToolNameTone::ReadOnly => styles.success(),
        ToolNameTone::Shell => styles.primary(),
        ToolNameTone::Write => styles.warning(),
        ToolNameTone::Agent => styles.accent(),
        ToolNameTone::Plan => styles.plan(),
        ToolNameTone::Utility => styles.secondary(),
    }
}
