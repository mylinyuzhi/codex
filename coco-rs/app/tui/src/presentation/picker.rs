//! Shared picker view models and renderer helpers.

#[cfg(test)]
use std::ops::Range;

use ratatui::prelude::*;

#[cfg(test)]
use super::layout;
use super::styles::UiStyles;
use crate::i18n::t;
use crate::state::CopyPickerSelection;
use crate::state::CopyPickerState;
use crate::state::ExportState;
use crate::state::GlobalSearchState;
use crate::state::McpServerSelectState;
use crate::state::MemoryDialogRowKind;
use crate::state::MemoryDialogScope;
use crate::state::MemoryDialogState;
use crate::state::QuickOpenState;
use crate::state::SessionBrowserState;
use crate::state::SkillLockSource;
use crate::state::SkillOverrideState;
use crate::state::SkillRow;
use crate::state::SkillsDialogSource;
use crate::state::SkillsDialogState;
use crate::state::surface_payloads::skill_override_glyph_and_label;

#[derive(Debug)]
#[cfg(test)]
pub(crate) enum PickerRow<'a, T> {
    Blank,
    Header(&'a str),
    Entry { filtered_index: usize, item: &'a T },
}

#[derive(Debug)]
#[cfg(test)]
pub(crate) struct PickerListView<'a, T> {
    pub(crate) rows: Vec<PickerRow<'a, T>>,
    pub(crate) visible: Range<usize>,
}

#[cfg(test)]
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

#[cfg(test)]
pub(crate) trait SpanBgOpt<'a> {
    fn bg_opt(self, bg: Option<Color>) -> Span<'a>;
}

#[cfg(test)]
impl<'a> SpanBgOpt<'a> for Span<'a> {
    fn bg_opt(self, bg: Option<Color>) -> Span<'a> {
        if let Some(bg) = bg { self.bg(bg) } else { self }
    }
}

#[cfg(test)]
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

#[cfg(test)]
pub(crate) fn blank_line(width: usize) -> Line<'static> {
    Line::from(Span::raw(" ".repeat(width)))
}

#[cfg(test)]
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
    s: &SessionBrowserState,
    styles: UiStyles<'_>,
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

    (
        t!("dialog.title_sessions").to_string(),
        body,
        styles.primary(),
    )
}

pub(crate) fn quick_open_content(
    q: &QuickOpenState,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
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
        styles.primary(),
    )
}

pub(crate) fn global_search_content(
    g: &GlobalSearchState,
    styles: UiStyles<'_>,
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
        styles.primary(),
    )
}

pub(crate) fn export_content(e: &ExportState, styles: UiStyles<'_>) -> (String, String, Color) {
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
        styles.primary(),
    )
}

pub(crate) fn copy_picker_content(
    cp: &CopyPickerState,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    let lines = cp.full_text.matches('\n').count() + 1;
    let chars = cp.full_text.chars().count();
    let full_label = t!("copy.picker_full_response", chars = chars, lines = lines).to_string();
    let always_label = t!("copy.picker_always").to_string();

    let mut rows: Vec<String> = Vec::with_capacity(cp.option_count());
    rows.push(format!(
        "{} {}",
        copy_picker_marker(cp.selected, CopyPickerSelection::Full),
        full_label,
    ));
    for (i, block) in cp.code_blocks.iter().enumerate() {
        let lang = block.lang.as_deref().unwrap_or("text");
        let block_chars = block.code.chars().count();
        let preview = first_line_preview(&block.code, 60);
        let label = t!(
            "copy.picker_code_block",
            lang = lang,
            chars = block_chars,
            preview = preview.as_str(),
        )
        .to_string();
        rows.push(format!(
            "{} {}",
            copy_picker_marker(cp.selected, CopyPickerSelection::CodeBlock(i)),
            label,
        ));
    }
    rows.push(format!(
        "{} {}",
        copy_picker_marker(cp.selected, CopyPickerSelection::Always),
        always_label,
    ));

    let title = t!("dialog.title_copy_picker", age = cp.message_age + 1).to_string();
    let body = format!(
        "{}\n\n{}\n\n{}",
        t!("dialog.copy_picker_prompt"),
        rows.join("\n"),
        t!("dialog.hints_copy_picker"),
    );
    (title, body, styles.primary())
}

fn copy_picker_marker(selected: CopyPickerSelection, arm: CopyPickerSelection) -> &'static str {
    if selected == arm { "▸" } else { " " }
}

fn first_line_preview(text: &str, max: usize) -> String {
    let line = text.lines().next().unwrap_or("");
    let mut out = String::new();
    for (width, ch) in line.chars().enumerate() {
        if width + 1 > max {
            out.push('\u{2026}');
            break;
        }
        out.push(ch);
    }
    out
}

pub(crate) fn memory_dialog_content(
    m: &MemoryDialogState,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    let items: Vec<String> = m
        .entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            format!(
                "{} {} {} {}",
                selected_marker(i, m.selected),
                memory_row_kind_tag(entry.row_kind),
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

    (
        t!("dialog.title_memory").to_string(),
        body,
        styles.primary(),
    )
}

/// Render the editable 2.1.142 `/skills` overlay. TS parity: `uJ4`
/// (`cli_inner_pretty.js:476909`). Flat list, per-row 4-state
/// override cycle, inline source label, lock annotation, filter
/// input + sort toggle.
pub(crate) fn skills_dialog_content(
    s: &SkillsDialogState,
    styles: UiStyles<'_>,
) -> (String, String, Color) {
    let title = t!("dialog.title_skills").to_string();

    if s.rows.is_empty() {
        return (
            title,
            t!("dialog.skills_empty").to_string(),
            styles.primary(),
        );
    }

    let view = s.filtered_view();
    let mut body = String::new();

    // Subtitle: `<filtered>/<total> skills` when filter active, else
    // `<total> skills`. Drives the visible "20 skills" header.
    let total = s.rows.len();
    let total_noun = if total == 1 {
        t!("dialog.skills_noun_singular")
    } else {
        t!("dialog.skills_noun_plural")
    };
    if s.filter_query.is_empty() {
        body.push_str(&t!(
            "dialog.skills_subtitle",
            count = total.to_string().as_str(),
            noun = total_noun.as_ref()
        ));
    } else {
        body.push_str(&format!("{}/{} {}", view.len(), total, total_noun.as_ref()));
    }
    body.push_str(" · ");
    body.push_str(&hint_line(s));
    body.push('\n');

    // Filter input row — mirrors TS `DN` with placeholder "Search
    // skills…". Render the query in-line; downstream styling is
    // applied by the higher-level surface renderer.
    body.push('\n');
    body.push_str("⌕ ");
    if s.filter_query.is_empty() {
        body.push_str(&t!("dialog.skills_filter_placeholder"));
    } else {
        body.push_str(&s.filter_query);
    }
    body.push('\n');

    if view.is_empty() {
        body.push('\n');
        body.push_str(&t!(
            "dialog.skills_empty_filter",
            query = s.filter_query.as_str()
        ));
    } else {
        for (i, row_idx) in view.iter().enumerate() {
            body.push('\n');
            body.push_str(&render_skill_row(
                &s.rows[*row_idx],
                i == s.selected_filtered_idx,
                s.bytes_per_token,
            ));
        }
    }

    // Plugin footer (TS `cli_inner_pretty.js:477128-477133`): only
    // rendered when at least one plugin row is present.
    if s.has_plugin_rows() {
        body.push_str("\n\n");
        body.push_str(&t!("dialog.skills_plugin_footer"));
    }

    (title, body, styles.primary())
}

/// Format the "Space to cycle, Enter to save, …" hint line. Two
/// variants per TS `cli_inner_pretty.js:477080-477090`: select mode
/// shows the full ladder; filter-focused mode swaps in the filter
/// instructions.
fn hint_line(s: &SkillsDialogState) -> String {
    if s.filter_focused {
        return t!("dialog.skills_hint_filter_focused").to_string();
    }
    t!("dialog.skills_hint_select").to_string()
}

/// One skill row in the dialog. Format mirrors TS `sT5`
/// (`cli_inner_pretty.js:477137`):
///
/// ```text
///   ✓ on        | my-skill · user · 42 tok
///   🔒 on       | claude-api · built-in · 30 tok · locked by author
/// ```
fn render_skill_row(row: &SkillRow, focused: bool, bytes_per_token: i64) -> String {
    let (glyph, label) = row
        .lock
        .as_ref()
        .map(|l| ('\u{1F512}', state_label_for_lock(l.forced_value)))
        .unwrap_or_else(|| skill_override_glyph_and_label(row.pending));
    let cursor = if focused { '\u{276F}' } else { ' ' }; // ❯
    let tokens = if bytes_per_token > 0 {
        row.frontmatter_bytes / bytes_per_token
    } else {
        row.frontmatter_bytes / 4
    };
    let mut line = format!("{cursor} {glyph} {label:<9} {}", row.name);
    line.push_str(" · ");
    line.push_str(skills_source_label(row.source));
    if let Some(plugin) = &row.plugin_name {
        line.push_str(" · ");
        line.push_str(plugin);
    }
    line.push_str(" · ");
    line.push_str(&t!(
        "dialog.skills_token_suffix",
        tokens = tokens.to_string().as_str()
    ));
    if let Some(lock) = &row.lock {
        line.push_str(" · ");
        line.push_str(&t!(
            "dialog.skills_locked_by",
            source = lock_source_label(lock.source)
        ));
    }
    line
}

fn state_label_for_lock(state: SkillOverrideState) -> &'static str {
    let (_, label) = skill_override_glyph_and_label(state);
    label
}

fn skills_source_label(source: SkillsDialogSource) -> &'static str {
    // TS `xJ4` (`cli_inner_pretty.js:476897-476907`) — normalised
    // labels shown inline next to each row.
    source.label_lower()
}

fn lock_source_label(source: SkillLockSource) -> &'static str {
    match source {
        SkillLockSource::Policy => "policy",
        SkillLockSource::Flag => "flag",
        SkillLockSource::Author => "author",
        SkillLockSource::Plugin => "plugin",
    }
}

pub(crate) fn mcp_server_select_content(
    ms: &McpServerSelectState,
    styles: UiStyles<'_>,
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
        styles.accent(),
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
        MemoryDialogScope::ProjectConfig => "[project-config]",
        MemoryDialogScope::Subdir => "[subdir]",
        MemoryDialogScope::Imported => "[@-imported]",
        MemoryDialogScope::AutoMemFolder => "[auto-mem]",
        MemoryDialogScope::TeamMemFolder => "[team-mem]",
        MemoryDialogScope::AgentMemFolder => "[agent-mem]",
    }
}

fn memory_row_kind_tag(kind: MemoryDialogRowKind) -> &'static str {
    match kind {
        MemoryDialogRowKind::File {
            exists: true,
            read_only: false,
        } => "[file:exists]",
        MemoryDialogRowKind::File {
            exists: false,
            read_only: false,
        } => "[file:new]",
        MemoryDialogRowKind::File {
            read_only: true, ..
        } => "[file:read-only]",
        MemoryDialogRowKind::Folder { enabled: true } => "[folder:on]",
        MemoryDialogRowKind::Folder { enabled: false } => "[folder:off]",
        MemoryDialogRowKind::Toggle { enabled: true } => "[toggle:on]",
        MemoryDialogRowKind::Toggle { enabled: false } => "[toggle:off]",
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

/// Re-export of the dedicated `/agents` overlay renderer. The
/// canonical implementation lives in
/// [`super::agents_dialog::agents_dialog_content`]; surfacing it from
/// `picker::` keeps the `surface_content/pickers.rs` delegate using
/// the same one-step indirection style as `skills_dialog_content` /
/// `memory_dialog_content`.
pub(crate) use super::agents_dialog::agents_dialog_content;

#[cfg(test)]
#[path = "picker.test.rs"]
mod tests;
