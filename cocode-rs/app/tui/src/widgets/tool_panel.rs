//! Tool execution panel widget.
//!
//! Displays currently running and recently completed tools.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::List;
use ratatui::widgets::ListItem;
use ratatui::widgets::Widget;

use unicode_width::UnicodeWidthStr;

use cocode_protocol::ToolName;

use crate::i18n::t;
use crate::state::BackgroundTask;
use crate::state::BackgroundTaskStatus;
use crate::state::McpToolCall;
use crate::state::StreamingToolUse;
use crate::state::ToolExecution;
use crate::state::ToolStatus;
use crate::theme::Theme;

/// Names of read-only "exploring" tools that get grouped when
/// consecutive and completed in the tool panel.
const EXPLORING_TOOLS: &[&str] = &[
    ToolName::Read.as_str(),
    ToolName::ReadManyFiles.as_str(),
    ToolName::Glob.as_str(),
    ToolName::Grep.as_str(),
    ToolName::LS.as_str(),
    ToolName::Lsp.as_str(),
];

/// Whether a tool name is an exploring tool.
fn is_exploring(name: &str) -> bool {
    EXPLORING_TOOLS.contains(&name)
}

/// Group consecutive completed exploring tools into collapsed cells.
///
/// Runs of 2+ consecutive completed exploring tools are replaced by a
/// single summary entry showing distinct tool types used in the run.
fn group_exploring_cells(
    tools: &[&ToolExecution],
    theme: &Theme,
    batch_counts: &std::collections::HashMap<&str, i32>,
) -> Vec<ListItem<'static>> {
    let check_parallel = |t: &ToolExecution| -> bool {
        t.batch_id
            .as_deref()
            .is_some_and(|bid| batch_counts.get(bid).copied().unwrap_or(0) > 1)
    };

    let mut items: Vec<ListItem<'static>> = Vec::new();
    let mut i = 0;
    while i < tools.len() {
        let t = tools[i];
        if matches!(t.status, ToolStatus::Completed) && is_exploring(&t.name) {
            // Count consecutive completed exploring tools
            let run_start = i;
            while i < tools.len()
                && matches!(tools[i].status, ToolStatus::Completed)
                && is_exploring(&tools[i].name)
            {
                i += 1;
            }
            let run_len = (i - run_start) as i32;
            if run_len >= 2 {
                // Collect distinct tool names in the run (preserving first-seen order)
                let mut names: Vec<&str> = Vec::with_capacity(EXPLORING_TOOLS.len());
                for t in &tools[run_start..i] {
                    let n = t.name.as_str();
                    if !names.contains(&n) {
                        names.push(n);
                    }
                }
                let label = names.join(", ");
                items.push(ListItem::new(Line::from(vec![
                    Span::raw(" "),
                    Span::raw("✓").fg(theme.tool_completed),
                    Span::raw(format!(" {} ", t!("tool.exploring_group", count = run_len))),
                    Span::raw(format!("({label})")).fg(theme.text_dim).italic(),
                ])));
            } else {
                // Single exploring tool — render normally
                items.push(ToolPanel::format_tool(t, theme, check_parallel(t)));
            }
        } else {
            items.push(ToolPanel::format_tool(t, theme, check_parallel(t)));
            i += 1;
        }
    }
    items
}

/// Tool panel widget showing tool execution status.
pub struct ToolPanel<'a> {
    tools: &'a [ToolExecution],
    theme: &'a Theme,
    max_display: usize,
    mcp_tools: &'a [McpToolCall],
    background_tasks: &'a [BackgroundTask],
    streaming_tools: &'a [StreamingToolUse],
}

impl<'a> ToolPanel<'a> {
    /// Create a new tool panel.
    pub fn new(tools: &'a [ToolExecution], theme: &'a Theme) -> Self {
        Self {
            tools,
            theme,
            max_display: 5,
            mcp_tools: &[],
            background_tasks: &[],
            streaming_tools: &[],
        }
    }

    /// Set the maximum number of tools to display.
    pub fn max_display(mut self, max: usize) -> Self {
        self.max_display = max;
        self
    }

    /// Set MCP tool calls to display alongside regular tools.
    pub fn mcp_tool_calls(mut self, mcp: &'a [McpToolCall]) -> Self {
        self.mcp_tools = mcp;
        self
    }

    /// Set background tasks to display.
    pub fn background_tasks(mut self, tasks: &'a [BackgroundTask]) -> Self {
        self.background_tasks = tasks;
        self
    }

    /// Set streaming tool uses (tools being built during streaming).
    pub fn streaming_tools(mut self, tools: &'a [StreamingToolUse]) -> Self {
        self.streaming_tools = tools;
        self
    }

    /// Format an MCP tool call for display.
    fn format_mcp_tool(tool: &McpToolCall, theme: &Theme) -> ListItem<'static> {
        let status_icon = match tool.status {
            ToolStatus::Running => Span::raw("⏳").fg(theme.tool_running),
            ToolStatus::Completed => Span::raw("✓").fg(theme.tool_completed),
            ToolStatus::Failed => Span::raw("✗").fg(theme.tool_error),
        };

        let secs = tool.started_at.elapsed().as_secs();
        let elapsed = if secs > 0 {
            Span::raw(format!(" {secs}s")).fg(theme.text_dim)
        } else {
            Span::raw("")
        };

        let line = Line::from(vec![
            status_icon,
            Span::raw(format!(" {}/", tool.server)).fg(theme.text_dim),
            Span::raw(tool.tool.clone()),
            elapsed,
        ]);
        ListItem::new(line)
    }

    /// Format a tool for display.
    ///
    /// If `is_parallel` is true, a parallel indicator is shown before the status icon.
    fn format_tool(tool: &ToolExecution, theme: &Theme, is_parallel: bool) -> ListItem<'static> {
        let parallel_prefix = if is_parallel {
            Span::raw("‖").fg(theme.text_dim)
        } else {
            Span::raw(" ")
        };
        let status_icon = match tool.status {
            ToolStatus::Running => Span::raw("⏳").fg(theme.tool_running),
            ToolStatus::Completed => Span::raw("✓").fg(theme.tool_completed),
            ToolStatus::Failed => Span::raw("✗").fg(theme.tool_error),
        };

        let name = Span::raw(format!(" {}", tool.name));

        // Show progress for running tools, or output preview for completed
        let progress = if let Some(ref p) = tool.progress {
            Span::raw(format!(" - {p}")).fg(theme.text_dim)
        } else if matches!(tool.status, ToolStatus::Completed | ToolStatus::Failed) {
            // Show first line of output as preview
            tool.output
                .as_deref()
                .and_then(|o| o.lines().next())
                .filter(|line| !line.is_empty())
                .map(|line| {
                    if UnicodeWidthStr::width(line) > 30 {
                        let end = line
                            .char_indices()
                            .take_while(|(i, _)| *i < 30)
                            .last()
                            .map_or(0, |(i, c)| i + c.len_utf8());
                        Span::raw(format!(" {}…", &line[..end])).fg(theme.text_dim)
                    } else {
                        Span::raw(format!(" {line}")).fg(theme.text_dim)
                    }
                })
                .unwrap_or_default()
        } else {
            Span::raw("")
        };

        // Show elapsed time for running and completed tools
        let elapsed = match tool.status {
            ToolStatus::Running => tool
                .started_at
                .map(|t| {
                    let secs = t.elapsed().as_secs();
                    if secs > 0 {
                        Span::raw(format!(" {secs}s")).fg(theme.text_dim)
                    } else {
                        Span::raw("")
                    }
                })
                .unwrap_or_else(|| Span::raw("")),
            ToolStatus::Completed | ToolStatus::Failed => tool
                .elapsed
                .map(|d| {
                    let ms = d.as_millis();
                    if ms < 1000 {
                        Span::raw(format!(" {ms}ms")).fg(theme.text_dim)
                    } else {
                        Span::raw(format!(" {:.1}s", d.as_secs_f64())).fg(theme.text_dim)
                    }
                })
                .unwrap_or_else(|| Span::raw("")),
        };

        let line = Line::from(vec![parallel_prefix, status_icon, name, progress, elapsed]);
        ListItem::new(line)
    }
}

impl Widget for ToolPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let has_any = !self.tools.is_empty()
            || !self.mcp_tools.is_empty()
            || !self.background_tasks.is_empty()
            || !self.streaming_tools.is_empty();
        if area.height < 2 || !has_any {
            return;
        }

        // Take the most recent tools
        let display_tools: Vec<_> = self.tools.iter().rev().take(self.max_display).collect();

        // Determine which batch_ids appear more than once (= parallel tools)
        let mut batch_counts: std::collections::HashMap<&str, i32> =
            std::collections::HashMap::new();
        for t in &display_tools {
            if let Some(ref bid) = t.batch_id {
                *batch_counts.entry(bid.as_str()).or_default() += 1;
            }
        }

        // Reverse back to chronological order for grouping
        let chrono_tools: Vec<_> = display_tools.into_iter().rev().collect();

        // Group consecutive exploring tools, then render the rest normally
        let mut items: Vec<ListItem> =
            group_exploring_cells(&chrono_tools, self.theme, &batch_counts);

        // Show streaming tool uses (tools being built during streaming)
        for st in self.streaming_tools {
            if !st.name.is_empty() {
                items.push(ListItem::new(Line::from(vec![
                    Span::raw("⚡").fg(self.theme.tool_running),
                    Span::raw(format!(" {}", st.name)).italic(),
                    Span::raw("...").dim(),
                ])));
            }
        }

        // Append MCP tool calls
        for mcp in self.mcp_tools.iter().rev().take(3) {
            items.push(Self::format_mcp_tool(mcp, self.theme));
        }

        // Append background tasks
        for task in self.background_tasks.iter().rev().take(3) {
            let status_icon = match task.status {
                BackgroundTaskStatus::Running => Span::raw("◐").fg(self.theme.secondary),
                BackgroundTaskStatus::Completed => Span::raw("✓").fg(self.theme.tool_completed),
                BackgroundTaskStatus::Failed => Span::raw("✗").fg(self.theme.tool_error),
            };
            let secs = task.started_at.elapsed().as_secs();
            let elapsed = if secs > 0 {
                Span::raw(format!(" {secs}s")).fg(self.theme.text_dim)
            } else {
                Span::raw("")
            };
            let progress = task
                .progress
                .as_ref()
                .map(|p| Span::raw(format!(" {p}")).fg(self.theme.text_dim))
                .unwrap_or_default();
            let type_name = match &task.task_type {
                cocode_protocol::TaskType::Shell => "Shell",
                cocode_protocol::TaskType::Agent => "Agent",
                cocode_protocol::TaskType::FileOp => "FileOp",
                cocode_protocol::TaskType::Other(s) => s.as_str(),
            };
            let name = format!(" {type_name}");
            items.push(ListItem::new(Line::from(vec![
                status_icon,
                Span::raw(name).italic(),
                progress,
                elapsed,
            ])));
        }

        let running_count = self
            .tools
            .iter()
            .filter(|t| t.status == ToolStatus::Running)
            .count()
            + self
                .mcp_tools
                .iter()
                .filter(|t| t.status == ToolStatus::Running)
                .count()
            + self
                .background_tasks
                .iter()
                .filter(|t| t.status == BackgroundTaskStatus::Running)
                .count();

        let title = if running_count > 0 {
            format!(" {} ", t!("tool.title_running", count = running_count))
        } else {
            format!(" {} ", t!("tool.title"))
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(ratatui::style::Style::default().fg(self.theme.border))
            .title(title);

        let list = List::new(items).block(block);

        list.render(area, buf);
    }
}

#[cfg(test)]
#[path = "tool_panel.test.rs"]
mod tests;
