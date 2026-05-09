use std::collections::VecDeque;

use super::QueueStatusWidget;
use crate::state::session::QueuedCommandDisplay;

#[test]
fn should_display_gates_on_non_empty() {
    let empty: VecDeque<QueuedCommandDisplay> = VecDeque::new();
    assert!(!QueueStatusWidget::should_display(&empty));

    let mut filled = VecDeque::new();
    filled.push_back(QueuedCommandDisplay {
        id: "test-id".to_string(),
        preview: "next prompt".to_string(),
    });
    assert!(QueueStatusWidget::should_display(&filled));
}
