use super::*;

fn make_tool(name: &str, status: ToolStatus) -> ToolExecution {
    ToolExecution {
        call_id: format!("call-{name}"),
        name: name.to_string(),
        status,
        progress: None,
        output: None,
    }
}

#[test]
fn test_tool_panel_empty() {
    let tools: Vec<ToolExecution> = vec![];
    let panel = ToolPanel::new(&tools);

    let area = Rect::new(0, 0, 40, 10);
    let mut buf = Buffer::empty(area);
    panel.render(area, &mut buf);

    // Should render nothing (empty tools)
}

#[test]
fn test_tool_panel_with_tools() {
    let tools = vec![
        make_tool("bash", ToolStatus::Running),
        make_tool("read", ToolStatus::Completed),
    ];
    let panel = ToolPanel::new(&tools);

    let area = Rect::new(0, 0, 40, 10);
    let mut buf = Buffer::empty(area);
    panel.render(area, &mut buf);

    let content: String = buf.content.iter().map(|c| c.symbol()).collect();
    assert!(content.contains("bash"));
    assert!(content.contains("read"));
}

#[test]
fn test_format_tool_running() {
    let tool = make_tool("test", ToolStatus::Running);
    let _item = ToolPanel::format_tool(&tool);
    // Item should be created successfully
}

#[test]
fn test_max_display() {
    let tools: Vec<_> = (0..10)
        .map(|i| make_tool(&format!("tool-{i}"), ToolStatus::Completed))
        .collect();
    let panel = ToolPanel::new(&tools).max_display(3);

    let area = Rect::new(0, 0, 40, 10);
    let mut buf = Buffer::empty(area);
    panel.render(area, &mut buf);

    // Should only show 3 most recent tools
}
