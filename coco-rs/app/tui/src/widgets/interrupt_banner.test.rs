use super::InterruptBanner;

#[test]
fn should_display_gates_on_was_interrupted() {
    assert!(!InterruptBanner::should_display(false));
    assert!(InterruptBanner::should_display(true));
}
