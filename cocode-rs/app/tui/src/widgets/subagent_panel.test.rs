use super::*;
use crate::theme::Theme;
use cocode_protocol::AgentProgress;

fn create_test_subagents() -> Vec<SubagentInstance> {
    vec![
        SubagentInstance {
            id: "agent-1".to_string(),
            agent_type: "Explore".to_string(),
            description: "Searching for API endpoints".to_string(),
            status: SubagentStatus::Running,
            progress: Some(AgentProgress {
                message: Some("Reading files...".to_string()),
                current_step: Some(2),
                total_steps: Some(5),
            }),
            result: None,
            output_file: None,
        },
        SubagentInstance {
            id: "agent-2".to_string(),
            agent_type: "Plan".to_string(),
            description: "Creating implementation plan".to_string(),
            status: SubagentStatus::Completed,
            progress: None,
            result: Some("Plan created".to_string()),
            output_file: None,
        },
    ]
}

#[test]
fn test_panel_creation() {
    let theme = Theme::default();
    let subagents = create_test_subagents();
    let panel = SubagentPanel::new(&subagents, &theme);

    let area = Rect::new(0, 0, 50, 10);
    let mut buf = Buffer::empty(area);

    panel.render(area, &mut buf);

    let content: String = buf.content.iter().map(|c| c.symbol()).collect();
    assert!(content.contains("Subagents"));
}

#[test]
fn test_empty_panel() {
    let theme = Theme::default();
    let subagents: Vec<SubagentInstance> = vec![];
    let panel = SubagentPanel::new(&subagents, &theme);

    let area = Rect::new(0, 0, 50, 10);
    let mut buf = Buffer::empty(area);

    panel.render(area, &mut buf);

    // Should still render the border
    let content: String = buf.content.iter().map(|c| c.symbol()).collect();
    assert!(content.contains("Subagents"));
}

#[test]
fn test_max_display() {
    let theme = Theme::default();
    let mut subagents = create_test_subagents();
    // Add more subagents
    for i in 3..10 {
        subagents.push(SubagentInstance {
            id: format!("agent-{}", i),
            agent_type: "Test".to_string(),
            description: format!("Test agent {}", i),
            status: SubagentStatus::Running,
            progress: None,
            result: None,
            output_file: None,
        });
    }

    let panel = SubagentPanel::new(&subagents, &theme).max_display(3);

    let area = Rect::new(0, 0, 50, 15);
    let mut buf = Buffer::empty(area);

    panel.render(area, &mut buf);

    let content: String = buf.content.iter().map(|c| c.symbol()).collect();
    assert!(content.contains("more"));
}
