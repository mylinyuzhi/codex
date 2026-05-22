use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::state::TeamMemberEntry;
use crate::state::TeamMemberStatus;
use crate::theme::Theme;

use super::*;

fn test_members() -> Vec<TeamMemberEntry> {
    vec![
        TeamMemberEntry {
            agent_id: "lead-01".to_string(),
            name: Some("Leader".to_string()),
            agent_type: Some("general-purpose".to_string()),
            status: TeamMemberStatus::Active,
            is_leader: true,
        },
        TeamMemberEntry {
            agent_id: "worker-01".to_string(),
            name: Some("Alice".to_string()),
            agent_type: Some("explore".to_string()),
            status: TeamMemberStatus::Active,
            is_leader: false,
        },
        TeamMemberEntry {
            agent_id: "worker-02".to_string(),
            name: None,
            agent_type: Some("bash".to_string()),
            status: TeamMemberStatus::Idle,
            is_leader: false,
        },
    ]
}

#[test]
fn test_renders_without_panic() {
    let members = test_members();
    let theme = Theme::default();
    let panel = TeamPanel::new(&members, "alpha", &theme);

    let area = Rect::new(0, 0, 60, 10);
    let mut buf = Buffer::empty(area);
    panel.render(area, &mut buf);
}

#[test]
fn test_skips_tiny_area() {
    let members = test_members();
    let theme = Theme::default();
    let panel = TeamPanel::new(&members, "alpha", &theme);

    // Too small — should not panic.
    let area = Rect::new(0, 0, 5, 2);
    let mut buf = Buffer::empty(area);
    panel.render(area, &mut buf);
}

#[test]
fn test_empty_members() {
    let members: Vec<TeamMemberEntry> = vec![];
    let theme = Theme::default();
    let panel = TeamPanel::new(&members, "empty", &theme);

    let area = Rect::new(0, 0, 60, 10);
    let mut buf = Buffer::empty(area);
    panel.render(area, &mut buf);
}
