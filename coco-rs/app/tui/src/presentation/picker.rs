//! Shared picker view models and renderer helpers.

use std::ops::Range;

use ratatui::prelude::*;

use super::layout;
use crate::i18n::t;
use crate::state::ExportOverlay;
use crate::state::GlobalSearchOverlay;
use crate::state::McpServerSelectOverlay;
use crate::state::MemoryDialogOverlay;
use crate::state::MemoryDialogScope;
use crate::state::QuickOpenOverlay;
use crate::state::SessionBrowserOverlay;
use crate::theme::Theme;

#[derive(Debug)]
pub(crate) enum PickerRow<'a, T> {
    Blank,
    Header(&'a str),
    Entry { filtered_index: usize, item: &'a T },
}

#[derive(Debug)]
pub(crate) struct PickerListView<'a, T> {
    pub(crate) rows: Vec<PickerRow<'a, T>>,
    pub(crate) visible: Range<usize>,
}

pub(crate) fn grouped_list<'a, 'b, T, F>(
    entries: &'b [&'a T],
    selected: Option<usize>,
    height: usize,
    group_label: F,
) -> PickerListView<'a, T>
where
    F: Fn(&'a T) -> &'a str,
{
    let mut rows = Vec::with_capacity(entries.len() + 8);
    let mut last_group: Option<&str> = None;
    for (filtered_index, entry) in entries.iter().copied().enumerate() {
        let group = group_label(entry);
        if last_group != Some(group) {
            if !rows.is_empty() {
                rows.push(PickerRow::Blank);
            }
            rows.push(PickerRow::Header(group));
            last_group = Some(group);
        }
        rows.push(PickerRow::Entry {
            filtered_index,
            item: entry,
        });
    }

    let selected_row = selected.and_then(|selected| {
        rows.iter().position(|row| {
            matches!(row, PickerRow::Entry { filtered_index, .. } if *filtered_index == selected)
        })
    });
    let visible = selected_row
        .map(|row| layout::visible_window(row, rows.len(), height))
        .unwrap_or(0..height.min(rows.len()));

    PickerListView { rows, visible }
}

pub(crate) trait SpanBgOpt<'a> {
    fn bg_opt(self, bg: Option<Color>) -> Span<'a>;
}

impl<'a> SpanBgOpt<'a> for Span<'a> {
    fn bg_opt(self, bg: Option<Color>) -> Span<'a> {
        if let Some(bg) = bg { self.bg(bg) } else { self }
    }
}

pub(crate) fn pad_line(mut line: Line<'static>, width: usize, bg: Option<Color>) -> Line<'static> {
    let used = line.width();
    if used < width {
        let pad = " ".repeat(width - used);
        let span = if let Some(bg) = bg {
            Span::raw(pad).bg(bg)
        } else {
            Span::raw(pad)
        };
        line.spans.push(span);
    }
    line
}

pub(crate) fn blank_line(width: usize) -> Line<'static> {
    Line::from(Span::raw(" ".repeat(width)))
}

pub(crate) fn collapse_hints(hints: &str, width: usize) -> String {
    let hints = hints.trim();
    if width == 0 {
        return String::new();
    }
    if layout::text_width(hints) <= width {
        return hints.to_string();
    }

    let mut collapsed = String::new();
    for part in hints.split("  ").filter(|part| !part.is_empty()) {
        let candidate = if collapsed.is_empty() {
            part.to_string()
        } else {
            format!("{collapsed}  {part}")
        };
        if layout::text_width(&candidate) > width {
            break;
        }
        collapsed = candidate;
    }

    if collapsed.is_empty() {
        layout::truncate_to_width(hints, width)
    } else {
        collapsed
    }
}

pub(crate) fn session_browser_content(
    s: &SessionBrowserOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    let filter_lower = s.filter.to_lowercase();
    let items: Vec<String> = s
        .sessions
        .iter()
        .filter(|sess| filter_lower.is_empty() || sess.label.to_lowercase().contains(&filter_lower))
        .enumerate()
        .map(|(i, session)| {
            format!(
                "{} {} — {}{} — {}",
                selected_marker(i, s.selected),
                session.label,
                session.message_count,
                t!("dialog.sessions_item_suffix"),
                session.created_at
            )
        })
        .collect();

    let body = if items.is_empty() {
        t!("dialog.no_saved_sessions").to_string()
    } else {
        format!(
            "{}\n\n{}\n\n{}",
            filter_line(
                &s.filter,
                t!("dialog.type_filter_sessions").as_ref(),
                FilterPrefix::Filter
            ),
            items.join("\n"),
            t!("dialog.hints_nav_resume_cancel")
        )
    };

    (t!("dialog.title_sessions").to_string(), body, theme.primary)
}

pub(crate) fn quick_open_content(q: &QuickOpenOverlay, theme: &Theme) -> (String, String, Color) {
    let items: Vec<String> = q
        .files
        .iter()
        .enumerate()
        .take(15)
        .map(|(i, file)| format!("{} {file}", selected_marker(i, q.selected)))
        .collect();

    (
        t!("dialog.title_quick_open").to_string(),
        format!(
            "{}\n\n{}\n\n{}",
            filter_line(
                &q.filter,
                t!("dialog.type_file_name").as_ref(),
                FilterPrefix::Open
            ),
            items.join("\n"),
            t!("dialog.hints_enter_open_cancel")
        ),
        theme.primary,
    )
}

pub(crate) fn global_search_content(
    g: &GlobalSearchOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    let query_line = if g.query.is_empty() {
        t!("dialog.type_search").to_string()
    } else {
        t!("dialog.search_prefix", text = g.query.as_str()).to_string()
    };

    let results: Vec<String> = g
        .results
        .iter()
        .enumerate()
        .take(20)
        .map(|(i, r)| {
            let marker = if i as i32 == g.selected { "▸ " } else { "  " };
            format!("{marker}{}:{} {}", r.file, r.line_number, r.content.trim())
        })
        .collect();

    let status = if g.is_searching {
        format!("\n{}", t!("dialog.searching"))
    } else if g.results.is_empty() && !g.query.is_empty() {
        format!("\n{}", t!("dialog.no_results"))
    } else {
        String::new()
    };

    (
        t!("dialog.title_global_search").to_string(),
        format!(
            "{query_line}{status}\n\n{}\n\n{}",
            results.join("\n"),
            t!("dialog.esc_cancel")
        ),
        theme.primary,
    )
}

pub(crate) fn export_content(e: &ExportOverlay, theme: &Theme) -> (String, String, Color) {
    let items: Vec<String> = e
        .formats
        .iter()
        .enumerate()
        .map(|(i, fmt)| format!("{} {}", selected_marker(i, e.selected), fmt.label()))
        .collect();

    (
        t!("dialog.title_export").to_string(),
        format!(
            "{}\n\n{}\n\n{}",
            t!("dialog.select_format"),
            items.join("\n"),
            t!("dialog.hints_nav_export_cancel")
        ),
        theme.primary,
    )
}

pub(crate) fn memory_dialog_content(
    m: &MemoryDialogOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    let items: Vec<String> = m
        .entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            format!(
                "{} {} {}",
                selected_marker(i, m.selected),
                memory_scope_tag(entry.scope),
                entry.label
            )
        })
        .collect();

    let body = if items.is_empty() {
        t!("dialog.memory_no_files").to_string()
    } else {
        format!(
            "{}\n\n{}\n\n{}",
            t!("dialog.memory_select"),
            items.join("\n"),
            t!("dialog.hints_nav_select_cancel"),
        )
    };

    (t!("dialog.title_memory").to_string(), body, theme.primary)
}

pub(crate) fn mcp_server_select_content(
    ms: &McpServerSelectOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    let items: Vec<String> = ms
        .servers
        .iter()
        .map(|s| {
            let check = if s.selected { "[x]" } else { "[ ]" };
            format!(
                "  {check} {} ({})",
                s.name,
                t!("mcp.tools_count", count = s.tool_count)
            )
        })
        .collect();

    (
        t!("dialog.title_select_mcp_servers").to_string(),
        format!(
            "{}\n\n{}",
            t!("dialog.filter_prefix", text = ms.filter.as_str()),
            items.join("\n")
        ),
        theme.accent,
    )
}

#[derive(Debug, Clone, Copy)]
enum FilterPrefix {
    Filter,
    Open,
}

fn selected_marker(index: usize, selected: i32) -> &'static str {
    if index as i32 == selected { "▸" } else { " " }
}

fn memory_scope_tag(scope: MemoryDialogScope) -> &'static str {
    match scope {
        MemoryDialogScope::Managed => "[managed]",
        MemoryDialogScope::User => "[user]",
        MemoryDialogScope::Project => "[project]",
        MemoryDialogScope::ProjectLocal => "[project-local]",
        MemoryDialogScope::Subdir => "[subdir]",
    }
}

fn filter_line(filter: &str, empty_text: &str, prefix: FilterPrefix) -> String {
    if filter.is_empty() {
        empty_text.to_string()
    } else {
        match prefix {
            FilterPrefix::Filter => t!("dialog.filter_prefix", text = filter).to_string(),
            FilterPrefix::Open => t!("dialog.open_prefix", text = filter).to_string(),
        }
    }
}

#[cfg(test)]
#[path = "picker.test.rs"]
mod tests;
