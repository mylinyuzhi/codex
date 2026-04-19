use std::collections::VecDeque;

use super::LocalCommandLog;

#[test]
fn should_display_gates_on_non_empty_deque() {
    let empty: VecDeque<String> = VecDeque::new();
    assert!(!LocalCommandLog::should_display(&empty));

    let mut filled = VecDeque::new();
    filled.push_back("echo hi".to_string());
    assert!(LocalCommandLog::should_display(&filled));
}
