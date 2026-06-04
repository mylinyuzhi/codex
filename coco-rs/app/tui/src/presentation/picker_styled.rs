//! Styled renderers for the list dialogs migrated onto the reusable
//! `coco_tui_ui::widgets::render_select_list` widget. Each returns
//! `(title, body lines, border)` for the styled-modal path in
//! `surface/modal.rs`, replacing the monochrome `(String, String, Color)`
//! builders in `picker.rs`. The list rows get a colored `❯` cursor; chrome
//! (intro / filter / hint lines) is dim.

use ratatui::prelude::*;

use crate::i18n::t;
use crate::presentation::picker::first_line_preview;
use crate::presentation::picker::memory_row_kind_tag;
use crate::presentation::picker::memory_scope_tag;
use crate::state::CopyPickerSelection;
use crate::state::CopyPickerState;
use crate::state::ExportState;
use crate::state::GlobalSearchState;
use crate::state::MemoryDialogState;
use crate::state::QuickOpenState;
use crate::state::SessionBrowserState;
use crate::state::TeamRosterState;
use coco_tui_ui::style::UiStyles;
use coco_tui_ui::widgets::SelectItem;
use coco_tui_ui::widgets::SelectListStyle;
use coco_tui_ui::widgets::render_select_list;

/// A dim chrome line (intro / filter / hint).
fn dim_line(text: impl Into<String>, styles: UiStyles<'_>) -> Line<'static> {
    Line::from(Span::styled(text.into(), Style::default().fg(styles.dim())))
}

/// `SelectListStyle` for these dialogs: unnumbered, scrolls past `visible`.
fn list_style(visible: usize) -> SelectListStyle {
    SelectListStyle {
        numbered: false,
        visible_count: visible.max(1),
    }
}

/// `/export` — pick an export format. Flat single-select list.
pub(crate) fn export_lines(
    e: &ExportState,
    styles: UiStyles<'_>,
    list_budget: usize,
) -> (String, Vec<Line<'static>>, Color) {
    let items: Vec<SelectItem> = e
        .formats
        .iter()
        .map(|fmt| SelectItem::new(fmt.label()))
        .collect();
    let mut lines = vec![
        dim_line(t!("dialog.select_format"), styles),
        Line::default(),
    ];
    lines.extend(render_select_list(
        &items,
        e.selected.max(0) as usize,
        &list_style(list_budget),
        styles,
    ));
    lines.push(Line::default());
    lines.push(dim_line(t!("dialog.hints_nav_export_cancel"), styles));
    (
        t!("dialog.title_export").to_string(),
        lines,
        styles.primary(),
    )
}

/// Teams roster — the leader cycles a focused teammate's permission mode.
/// TS: `components/teams/TeamsDialog.tsx`. Rows list the running teammates;
/// the pill below shows the mode about to be applied on Enter.
pub(crate) fn team_roster_lines(
    r: &TeamRosterState,
    styles: UiStyles<'_>,
    list_budget: usize,
) -> (String, Vec<Line<'static>>, Color) {
    let title = t!("dialog.title_team_roster").to_string();
    if r.members.is_empty() {
        return (
            title,
            vec![dim_line(t!("dialog.team_roster_empty"), styles)],
            styles.primary(),
        );
    }
    let items: Vec<SelectItem> = r
        .members
        .iter()
        .map(|m| {
            // Each row shows the teammate's OWN current mode (TS renders
            // `teammate.mode` per row) so divergent modes are visible at a
            // glance.
            let mode = crate::update::permission_mode_label(m.mode);
            let label = if m.agent_type.is_empty() {
                format!("{} · {mode}", m.name)
            } else {
                format!("{} ({}) · {mode}", m.name, m.agent_type)
            };
            SelectItem::new(label)
        })
        .collect();
    let mut lines = vec![
        dim_line(
            t!("dialog.team_roster_team", team = r.team_name.as_str()),
            styles,
        ),
        Line::default(),
    ];
    lines.extend(render_select_list(
        &items,
        r.selected.min(items.len().saturating_sub(1)),
        &list_style(list_budget),
        styles,
    ));
    lines.push(Line::default());
    // The pill reflects the FOCUSED member's mode (the one Left/Right edits).
    let focused_mode = r
        .members
        .get(r.selected)
        .map(|m| m.mode)
        .unwrap_or(coco_types::PermissionMode::Default);
    lines.push(Line::from(vec![
        Span::styled(
            t!("dialog.team_roster_set_mode").to_string(),
            Style::default().fg(styles.dim()),
        ),
        Span::styled(
            crate::update::permission_mode_label(focused_mode),
            Style::default().fg(styles.accent()),
        ),
    ]));
    lines.push(dim_line(t!("dialog.hints_team_roster"), styles));
    (title, lines, styles.primary())
}

/// `/memory` — pick a memory file to edit. Flat single-select list.
pub(crate) fn memory_dialog_lines(
    m: &MemoryDialogState,
    styles: UiStyles<'_>,
    list_budget: usize,
) -> (String, Vec<Line<'static>>, Color) {
    let title = t!("dialog.title_memory").to_string();
    if m.entries.is_empty() {
        return (
            title,
            vec![dim_line(t!("dialog.memory_no_files"), styles)],
            styles.primary(),
        );
    }
    let items: Vec<SelectItem> = m
        .entries
        .iter()
        .map(|e| {
            SelectItem::new(format!(
                "{} {} {}",
                memory_row_kind_tag(e.row_kind),
                memory_scope_tag(e.scope),
                e.label
            ))
        })
        .collect();
    let mut lines = vec![
        dim_line(t!("dialog.memory_select"), styles),
        Line::default(),
    ];
    lines.extend(render_select_list(
        &items,
        m.selected.max(0) as usize,
        &list_style(list_budget),
        styles,
    ));
    lines.push(Line::default());
    lines.push(dim_line(t!("dialog.hints_nav_select_cancel"), styles));
    (title, lines, styles.primary())
}

/// `/quick-open` — fuzzy file picker. Filtered flat list, capped at 15 rows.
pub(crate) fn quick_open_lines(
    q: &QuickOpenState,
    styles: UiStyles<'_>,
    list_budget: usize,
) -> (String, Vec<Line<'static>>, Color) {
    let items: Vec<SelectItem> = q.files.iter().map(SelectItem::new).collect();
    let filter = if q.filter.is_empty() {
        dim_line(t!("dialog.type_file_name"), styles)
    } else {
        dim_line(t!("dialog.open_prefix", text = q.filter.as_str()), styles)
    };
    let mut lines = vec![filter, Line::default()];
    lines.extend(render_select_list(
        &items,
        q.selected.max(0) as usize,
        &list_style(list_budget),
        styles,
    ));
    lines.push(Line::default());
    lines.push(dim_line(t!("dialog.hints_enter_open_cancel"), styles));
    (
        t!("dialog.title_quick_open").to_string(),
        lines,
        styles.primary(),
    )
}

/// `/resume` — session browser. Filtered flat list.
pub(crate) fn session_browser_lines(
    s: &SessionBrowserState,
    styles: UiStyles<'_>,
    list_budget: usize,
) -> (String, Vec<Line<'static>>, Color) {
    let title = t!("dialog.title_sessions").to_string();
    let filter_lower = s.filter.to_lowercase();
    let items: Vec<SelectItem> = s
        .sessions
        .iter()
        .filter(|sess| filter_lower.is_empty() || sess.label.to_lowercase().contains(&filter_lower))
        .map(|session| {
            SelectItem::new(format!(
                "{} — {}{} — {}",
                session.label,
                session.message_count,
                t!("dialog.sessions_item_suffix"),
                session.created_at
            ))
        })
        .collect();
    if items.is_empty() {
        return (
            title,
            vec![dim_line(t!("dialog.no_saved_sessions"), styles)],
            styles.primary(),
        );
    }
    let filter = if s.filter.is_empty() {
        dim_line(t!("dialog.type_filter_sessions"), styles)
    } else {
        dim_line(t!("dialog.filter_prefix", text = s.filter.as_str()), styles)
    };
    let mut lines = vec![filter, Line::default()];
    lines.extend(render_select_list(
        &items,
        s.selected.max(0) as usize,
        &list_style(list_budget),
        styles,
    ));
    lines.push(Line::default());
    lines.push(dim_line(t!("dialog.hints_nav_resume_cancel"), styles));
    (title, lines, styles.primary())
}

/// `/search` — global search. Query line + status + flat result list (cap 20).
pub(crate) fn global_search_lines(
    g: &GlobalSearchState,
    styles: UiStyles<'_>,
    list_budget: usize,
) -> (String, Vec<Line<'static>>, Color) {
    let query_text = if g.query.is_empty() {
        t!("dialog.type_search").to_string()
    } else {
        t!("dialog.search_prefix", text = g.query.as_str()).to_string()
    };
    let mut lines = vec![dim_line(query_text, styles)];
    if g.is_searching {
        lines.push(dim_line(t!("dialog.searching"), styles));
    } else if g.results.is_empty() && !g.query.is_empty() {
        lines.push(dim_line(t!("dialog.no_results"), styles));
    }
    lines.push(Line::default());

    let items: Vec<SelectItem> = g
        .results
        .iter()
        .map(|r| SelectItem::new(format!("{}:{} {}", r.file, r.line_number, r.content.trim())))
        .collect();
    lines.extend(render_select_list(
        &items,
        g.selected.max(0) as usize,
        &list_style(list_budget),
        styles,
    ));
    lines.push(Line::default());
    lines.push(dim_line(t!("dialog.esc_cancel"), styles));
    (
        t!("dialog.title_global_search").to_string(),
        lines,
        styles.primary(),
    )
}

/// `/copy` — pick what to copy (full response, a code block, or "always").
/// The selection is an enum; map it to the flat row index.
pub(crate) fn copy_picker_lines(
    cp: &CopyPickerState,
    styles: UiStyles<'_>,
    list_budget: usize,
) -> (String, Vec<Line<'static>>, Color) {
    let full_lines = cp.full_text.matches('\n').count() + 1;
    let chars = cp.full_text.chars().count();
    let mut items = vec![SelectItem::new(
        t!(
            "copy.picker_full_response",
            chars = chars,
            lines = full_lines
        )
        .to_string(),
    )];
    for block in &cp.code_blocks {
        let lang = block.lang.as_deref().unwrap_or("text");
        let block_chars = block.code.chars().count();
        let preview = first_line_preview(&block.code, 60);
        items.push(SelectItem::new(
            t!(
                "copy.picker_code_block",
                lang = lang,
                chars = block_chars,
                preview = preview.as_str(),
            )
            .to_string(),
        ));
    }
    items.push(SelectItem::new(t!("copy.picker_always").to_string()));

    let selected = match cp.selected {
        CopyPickerSelection::Full => 0,
        CopyPickerSelection::CodeBlock(i) => 1 + i,
        CopyPickerSelection::Always => 1 + cp.code_blocks.len(),
    };

    let mut lines = vec![
        dim_line(t!("dialog.copy_picker_prompt"), styles),
        Line::default(),
    ];
    lines.extend(render_select_list(
        &items,
        selected,
        &list_style(list_budget),
        styles,
    ));
    lines.push(Line::default());
    lines.push(dim_line(t!("dialog.hints_copy_picker"), styles));
    (
        t!("dialog.title_copy_picker", age = cp.message_age + 1).to_string(),
        lines,
        styles.primary(),
    )
}

#[cfg(test)]
#[path = "picker_styled.test.rs"]
mod tests;
