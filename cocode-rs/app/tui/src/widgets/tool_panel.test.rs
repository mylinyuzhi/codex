use super::*;
use crate::theme::Theme;

fn make_tool(name: &str, status: ToolStatus) -> ToolExecution {
    ToolExecution {
        call_id: format!("call-{name}"),
        name: name.to_string(),
        status,
        progress: None,
        output: None,
        started_at: None,
        elapsed: None,
        batch_id: None,
    }
}

#[test]
fn test_tool_panel_empty() {
    let theme = Theme::default();
    let tools: Vec<ToolExecution> = vec![];
    let panel = ToolPanel::new(&tools, &theme);

    let area = Rect::new(0, 0, 40, 10);
    let mut buf = Buffer::empty(area);
    panel.render(area, &mut buf);

    // Should render nothing (empty tools)
}

#[test]
fn test_tool_panel_with_tools() {
    let theme = Theme::default();
    let tools = vec![
        make_tool("bash", ToolStatus::Running),
        make_tool("read", ToolStatus::Completed),
    ];
    let panel = ToolPanel::new(&tools, &theme);

    let area = Rect::new(0, 0, 40, 10);
    let mut buf = Buffer::empty(area);
    panel.render(area, &mut buf);

    let content: String = buf
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect();
    assert!(content.contains("bash"));
    assert!(content.contains("read"));
}

#[test]
fn test_format_tool_running() {
    let theme = Theme::default();
    let tool = make_tool("test", ToolStatus::Running);
    let _item = ToolPanel::format_tool(&tool, &theme, false);
    // Item should be created successfully
}

#[test]
fn test_max_display() {
    let theme = Theme::default();
    let tools: Vec<_> = (0..10)
        .map(|i| make_tool(&format!("tool-{i}"), ToolStatus::Completed))
        .collect();
    let panel = ToolPanel::new(&tools, &theme).max_display(3);

    let area = Rect::new(0, 0, 40, 10);
    let mut buf = Buffer::empty(area);
    panel.render(area, &mut buf);

    // Should only show 3 most recent tools
}

#[test]
fn test_parallel_tools_indicator() {
    let theme = Theme::default();
    let batch = Some("batch-1".to_string());
    let tools = vec![
        ToolExecution {
            call_id: "call-a".to_string(),
            name: "read".to_string(),
            status: ToolStatus::Running,
            progress: None,
            output: None,
            started_at: None,
            elapsed: None,
            batch_id: batch.clone(),
        },
        ToolExecution {
            call_id: "call-b".to_string(),
            name: "glob".to_string(),
            status: ToolStatus::Running,
            progress: None,
            output: None,
            started_at: None,
            elapsed: None,
            batch_id: batch.clone(),
        },
        ToolExecution {
            call_id: "call-c".to_string(),
            name: "grep".to_string(),
            status: ToolStatus::Running,
            progress: None,
            output: None,
            started_at: None,
            elapsed: None,
            batch_id: batch,
        },
    ];
    let panel = ToolPanel::new(&tools, &theme);

    let area = Rect::new(0, 0, 40, 10);
    let mut buf = Buffer::empty(area);
    panel.render(area, &mut buf);

    let content: String = buf
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect();
    // All 3 tools share the same batch_id → parallel indicator should appear
    assert!(
        content.contains('‖'),
        "Expected parallel indicator ‖ in: {content}"
    );
}
