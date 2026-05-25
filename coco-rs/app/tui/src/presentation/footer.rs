//! Footer/status-bar presentation model.

use std::collections::HashSet;

use coco_types::ModelRole;
use coco_types::PermissionMode;

use crate::i18n::t;
use crate::state::AppState;
use crate::state::ExitKey;
use crate::state::transcript_view::CellKind;
use crate::state::transcript_view::RenderedCell;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FooterTone {
    Primary,
    Dim,
    Border,
    Warning,
    Accent,
    Plan,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FooterSpan {
    pub(crate) text: String,
    pub(crate) tone: FooterTone,
    pub(crate) bold: bool,
}

impl FooterSpan {
    fn new(text: impl Into<String>, tone: FooterTone) -> Self {
        Self {
            text: text.into(),
            tone,
            bold: false,
        }
    }

    fn bold(text: impl Into<String>, tone: FooterTone) -> Self {
        Self {
            text: text.into(),
            tone,
            bold: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FooterView {
    ExitPrompt { key: ExitKey, text: String },
    Status { spans: Vec<FooterSpan> },
}

pub(crate) fn footer_view(state: &AppState) -> FooterView {
    if let Some(key) = state.ui.pending_exit_hint() {
        return FooterView::ExitPrompt {
            key,
            text: t!("status.exit_prompt", key = key.label()).to_string(),
        };
    }

    let mut spans = Vec::new();
    let (provider, model_id) = state
        .session
        .model_by_role
        .get(&ModelRole::Main)
        .map(|b| (b.provider.clone(), b.model_id.clone()))
        .unwrap_or_else(|| (state.session.provider.clone(), state.session.model.clone()));
    let model_display = if !provider.is_empty() && !model_id.is_empty() {
        format!("{provider}/{model_id}")
    } else if !model_id.is_empty() {
        model_id
    } else {
        provider
    };
    let has_model = !model_display.is_empty();
    if has_model {
        spans.push(FooterSpan::bold(
            format!(" {model_display}"),
            FooterTone::Primary,
        ));
        if state.session.fast_mode {
            spans.push(FooterSpan::new(" ⚡", FooterTone::Warning));
        }
    }

    let join = if has_model { " * " } else { " " };
    spans.push(FooterSpan::new(join, FooterTone::Dim));
    spans.push(FooterSpan::new(
        state.session.thinking_effort.to_string(),
        FooterTone::Dim,
    ));

    if let Some((mode_label, mode_tone)) =
        permission_mode_status_label(state.session.permission_mode)
    {
        separator(&mut spans);
        spans.push(FooterSpan::new(mode_label, mode_tone));
    }

    if let Some(hint) = state.ui.kb_handle.pending_display() {
        separator(&mut spans);
        spans.push(FooterSpan::bold(hint, FooterTone::Warning));
    }

    if let Some(warning) = state.ui.terminal_compatibility_warning.as_ref() {
        separator(&mut spans);
        spans.push(FooterSpan::bold(warning.clone(), FooterTone::Warning));
    }

    let tokens = &state.session.token_usage;
    separator(&mut spans);
    spans.push(FooterSpan::new(
        format!(
            "↑{} ↓{}",
            format_token_count(tokens.input_tokens),
            format_token_count(tokens.output_tokens)
        ),
        FooterTone::Dim,
    ));
    let cache_pct = if tokens.input_tokens > 0 {
        (tokens.cache_read_tokens.max(0) * 100 / tokens.input_tokens).clamp(0, 100)
    } else {
        0
    };
    spans.push(FooterSpan::new(
        format!(
            " · cache {}/{}%",
            format_token_count(tokens.cache_read_tokens),
            cache_pct
        ),
        FooterTone::Dim,
    ));

    let ctx_pct = if state.session.context_window_total > 0 {
        let used = state.session.context_window_used as i64;
        let total = state.session.context_window_total as i64;
        (used * 100 / total.max(1)).clamp(0, 100)
    } else {
        0
    };
    separator(&mut spans);
    spans.push(FooterSpan {
        text: format!("ctx {ctx_pct}%"),
        tone: if ctx_pct > 90 {
            FooterTone::Error
        } else if ctx_pct > 70 {
            FooterTone::Warning
        } else {
            FooterTone::Dim
        },
        bold: ctx_pct > 90,
    });

    let mcp_count = state.session.connected_mcp_count();
    if mcp_count > 0 {
        separator(&mut spans);
        spans.push(FooterSpan::new(
            t!("status.mcp", count = mcp_count).to_string(),
            FooterTone::Dim,
        ));
    }

    separator(&mut spans);
    spans.push(FooterSpan::new(
        transcript_count_status(state.session.transcript.cells()),
        FooterTone::Dim,
    ));

    FooterView::Status { spans }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct TranscriptCounts {
    users: usize,
    assistants: usize,
    tools: usize,
}

fn transcript_count_status(cells: &[RenderedCell]) -> String {
    let counts = transcript_counts(cells);
    if counts.tools > 0 {
        t!(
            "status.turn_counts_with_tools",
            users = counts.users,
            assistants = counts.assistants,
            tools = counts.tools
        )
        .to_string()
    } else {
        t!(
            "status.turn_counts",
            users = counts.users,
            assistants = counts.assistants
        )
        .to_string()
    }
}

fn transcript_counts(cells: &[RenderedCell]) -> TranscriptCounts {
    let mut seen = HashSet::new();
    let mut counts = TranscriptCounts::default();
    for cell in cells {
        if !seen.insert(cell.message_uuid) {
            continue;
        }
        match &cell.kind {
            CellKind::UserText { .. } | CellKind::UserAttachment => counts.users += 1,
            CellKind::AssistantText { .. }
            | CellKind::AssistantThinking { .. }
            | CellKind::AssistantRedactedThinking
            | CellKind::ToolUse { .. } => counts.assistants += 1,
            CellKind::ToolResult { .. } => counts.tools += 1,
            CellKind::Attachment
            | CellKind::Progress
            | CellKind::Tombstone
            | CellKind::System(_) => {}
        }
    }
    counts
}

fn separator(spans: &mut Vec<FooterSpan>) {
    spans.push(FooterSpan::new(" | ", FooterTone::Border));
}

fn permission_mode_status_label(mode: PermissionMode) -> Option<(String, FooterTone)> {
    let (key, tone) = match mode {
        PermissionMode::Default => return None,
        PermissionMode::AcceptEdits => ("permission_mode.status.accept_edits", FooterTone::Accent),
        PermissionMode::Plan => ("permission_mode.status.plan", FooterTone::Plan),
        PermissionMode::BypassPermissions => ("permission_mode.status.bypass", FooterTone::Error),
        PermissionMode::DontAsk => ("permission_mode.status.dont_ask", FooterTone::Error),
        PermissionMode::Auto => ("permission_mode.status.auto", FooterTone::Warning),
        PermissionMode::Bubble => ("permission_mode.status.bubble", FooterTone::Dim),
    };
    Some((t!(key).to_string(), tone))
}

pub(crate) fn format_token_count(count: i64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}K", count as f64 / 1_000.0)
    } else {
        format!("{count}")
    }
}

#[cfg(test)]
#[path = "footer.test.rs"]
mod tests;
