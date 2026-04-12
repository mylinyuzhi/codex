use super::*;

#[test]
fn test_assign_teammate_color_round_robin() {
    clear_teammate_colors();

    let c1 = assign_teammate_color("agent-1");
    let c2 = assign_teammate_color("agent-2");
    let c3 = assign_teammate_color("agent-3");

    assert_eq!(c1, AgentColorName::Blue);
    assert_eq!(c2, AgentColorName::Green);
    assert_eq!(c3, AgentColorName::Yellow);

    // Same agent gets same color
    assert_eq!(assign_teammate_color("agent-1"), AgentColorName::Blue);

    clear_teammate_colors();
}

#[test]
fn test_get_teammate_color() {
    clear_teammate_colors();

    assert!(get_teammate_color("unknown").is_none());

    assign_teammate_color("test-agent");
    assert_eq!(get_teammate_color("test-agent"), Some(AgentColorName::Blue));

    clear_teammate_colors();
}

#[test]
fn test_clear_teammate_colors() {
    clear_teammate_colors();
    assign_teammate_color("x");
    assert!(get_teammate_color("x").is_some());

    clear_teammate_colors();
    assert!(get_teammate_color("x").is_none());
}

#[test]
fn test_color_palette_wraps() {
    clear_teammate_colors();

    // Assign 8 colors (full palette)
    for i in 0..8 {
        assign_teammate_color(&format!("a-{i}"));
    }
    // 9th should wrap to first color
    let c9 = assign_teammate_color("a-8");
    assert_eq!(c9, AgentColorName::Blue);

    clear_teammate_colors();
}
