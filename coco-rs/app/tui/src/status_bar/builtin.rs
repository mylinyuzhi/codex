use std::collections::HashSet;

use coco_types::ModelRole;
use coco_types::PermissionMode;

use crate::i18n::t;
use crate::presentation::context_usage::render_context_usage;
use crate::state::AppState;
use crate::status_bar::StatusSpan;
use crate::status_bar::StatusTone;
use crate::transcript::cells::CellKind;
use crate::transcript::cells::RenderedCell;

pub(crate) fn built_in_status_spans(state: &AppState) -> Vec<StatusSpan> {
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
        spans.push(StatusSpan::bold(
            format!(" {model_display}"),
            StatusTone::Primary,
        ));
        if state.session.fast_mode {
            spans.push(StatusSpan::new(" ⚡", StatusTone::Warning));
        }
    }

    let join = if has_model { " * " } else { " " };
    spans.push(StatusSpan::new(join, StatusTone::Dim));
    spans.push(StatusSpan::new(
        state.session.thinking_effort.to_string(),
        StatusTone::Dim,
    ));

    if let Some((mode_label, mode_tone)) =
        permission_mode_status_label(state.session.permission_mode)
    {
        separator(&mut spans);
        spans.push(StatusSpan::new(mode_label, mode_tone));
    }

    if let Some(hint) = state.ui.kb_handle.pending_display() {
        separator(&mut spans);
        spans.push(StatusSpan::bold(hint, StatusTone::Warning));
    }

    if let Some(warning) = state.ui.terminal_compatibility_warning.as_ref() {
        separator(&mut spans);
        spans.push(StatusSpan::bold(warning.clone(), StatusTone::Warning));
    }

    let tokens = &state.session.token_usage;
    let usage_costs = state.session.session_usage.as_ref().map(|snapshot| {
        let input_cost = snapshot.totals.input_cost_usd
            + snapshot.totals.cache_read_cost_usd
            + snapshot.totals.cache_creation_cost_usd;
        let output_cost = snapshot.totals.output_cost_usd;
        let all_unpriced = snapshot.totals.request_count > 0
            && snapshot.totals.unpriced_request_count == snapshot.totals.request_count;
        (
            input_cost,
            output_cost,
            all_unpriced,
            snapshot.unpriced_models.len(),
        )
    });
    separator(&mut spans);
    spans.push(StatusSpan::new(
        match usage_costs {
            Some((_, _, true, _)) => format!(
                "↑{}/$? ↓{}/$?",
                format_token_count(tokens.input_tokens),
                format_token_count(tokens.output_tokens)
            ),
            Some((input_cost, output_cost, false, _)) => format!(
                "↑{}/{} ↓{}/{}",
                format_token_count(tokens.input_tokens),
                format_cost(input_cost),
                format_token_count(tokens.output_tokens),
                format_cost(output_cost)
            ),
            None => format!(
                "↑{} ↓{}",
                format_token_count(tokens.input_tokens),
                format_token_count(tokens.output_tokens)
            ),
        },
        StatusTone::Dim,
    ));
    let cache_pct = if tokens.input_tokens > 0 {
        (tokens.cache_read_tokens.max(0) * 100 / tokens.input_tokens).clamp(0, 100)
    } else {
        0
    };
    spans.push(StatusSpan::new(
        format!(
            " · cache {}/{}%",
            format_token_count(tokens.cache_read_tokens),
            cache_pct
        ),
        StatusTone::Dim,
    ));
    if let Some((_, _, false, unpriced_count)) = usage_costs
        && unpriced_count > 0
    {
        spans.push(StatusSpan::new(
            format!(" · unpriced {unpriced_count}"),
            StatusTone::Warning,
        ));
    }

    separator(&mut spans);
    if let Some(ctx_pct) = render_context_usage(state).map(|u| u.percent) {
        spans.push(StatusSpan {
            text: format!("ctx {ctx_pct}%"),
            tone: if ctx_pct > 90 {
                StatusTone::Error
            } else if ctx_pct > 70 {
                StatusTone::Warning
            } else {
                StatusTone::Dim
            },
            bold: ctx_pct > 90,
        });
    } else {
        spans.push(StatusSpan::new("ctx --", StatusTone::Dim));
    }

    let mcp_count = state.session.connected_mcp_count();
    if mcp_count > 0 {
        separator(&mut spans);
        spans.push(StatusSpan::new(
            t!("status.mcp", count = mcp_count).to_string(),
            StatusTone::Dim,
        ));
    }

    if state.session.lsp_active {
        separator(&mut spans);
        spans.push(StatusSpan::new("LSP", StatusTone::Dim));
    }

    separator(&mut spans);
    spans.push(StatusSpan::new(
        transcript_count_status(state.session.transcript.cells()),
        StatusTone::Dim,
    ));
    spans
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

fn separator(spans: &mut Vec<StatusSpan>) {
    spans.push(StatusSpan::new(" | ", StatusTone::Border));
}

fn permission_mode_status_label(mode: PermissionMode) -> Option<(String, StatusTone)> {
    let (key, tone) = match mode {
        PermissionMode::Default => return None,
        PermissionMode::AcceptEdits => ("permission_mode.status.accept_edits", StatusTone::Accent),
        PermissionMode::Plan => ("permission_mode.status.plan", StatusTone::Plan),
        PermissionMode::BypassPermissions => ("permission_mode.status.bypass", StatusTone::Error),
        PermissionMode::DontAsk => ("permission_mode.status.dont_ask", StatusTone::Error),
        PermissionMode::Auto => ("permission_mode.status.auto", StatusTone::Warning),
        PermissionMode::Bubble => ("permission_mode.status.bubble", StatusTone::Dim),
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

fn format_cost(cost_usd: f64) -> String {
    coco_messages::format_cost(cost_usd)
}
