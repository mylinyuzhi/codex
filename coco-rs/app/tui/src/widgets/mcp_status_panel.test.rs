use super::McpStatusPanel;
use crate::state::session::McpServerStatus;

#[test]
fn should_display_gates_on_non_empty() {
    let empty: [McpServerStatus; 0] = [];
    assert!(!McpStatusPanel::should_display(&empty));

    let populated = vec![McpServerStatus {
        name: "fs".into(),
        connected: true,
        tool_count: 5,
    }];
    assert!(McpStatusPanel::should_display(&populated));
}
