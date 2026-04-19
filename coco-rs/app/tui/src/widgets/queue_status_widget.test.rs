use std::collections::VecDeque;

use super::QueueStatusWidget;

#[test]
fn should_display_gates_on_non_empty() {
    let empty: VecDeque<String> = VecDeque::new();
    assert!(!QueueStatusWidget::should_display(&empty));

    let mut filled = VecDeque::new();
    filled.push_back("next prompt".to_string());
    assert!(QueueStatusWidget::should_display(&filled));
}
